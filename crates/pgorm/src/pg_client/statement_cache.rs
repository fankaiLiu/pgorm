use crate::error::OrmError;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_postgres::Statement;

/// Statement cache statistics.
#[derive(Debug, Clone, Copy, Default)]
pub struct StmtCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub size: usize,
    pub capacity: usize,
}

impl StmtCacheStats {
    /// Cache hit ratio (0.0 – 1.0). Returns 0.0 if no lookups have occurred.
    pub fn hit_ratio(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

#[derive(Debug)]
pub(super) struct StatementCache {
    inner: Mutex<StatementCacheInner>,
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
}

#[derive(Debug)]
struct StatementCacheInner {
    capacity: usize,
    map: HashMap<String, CacheEntry>,
    generation: u64,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    stmt: Statement,
    last_access: u64,
}

impl StatementCacheInner {
    fn touch(&mut self, key: &str) -> Option<Statement> {
        let entry = self.map.get_mut(key)?;
        self.generation += 1;
        entry.last_access = self.generation;
        Some(entry.stmt.clone())
    }

    fn evict_if_needed(&mut self) -> u64 {
        if self.capacity == 0 {
            let evicted = self.map.len() as u64;
            self.map.clear();
            return evicted;
        }

        let mut evicted = 0u64;
        while self.map.len() > self.capacity {
            // Find the entry with the smallest last_access (LRU).
            let oldest_key = self
                .map
                .iter()
                .min_by_key(|(_, e)| e.last_access)
                .map(|(k, _)| k.clone());

            if let Some(key) = oldest_key {
                self.map.remove(&key);
                evicted += 1;
            } else {
                break;
            }
        }
        evicted
    }
}

impl StatementCache {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(StatementCacheInner {
                capacity,
                map: HashMap::with_capacity(capacity),
                generation: 0,
            }),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    pub(super) fn get(&self, key: &str) -> Option<Statement> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match inner.touch(key) {
            Some(stmt) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(stmt)
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    pub(super) fn insert_if_absent(&self, key: String, stmt: Statement) -> Statement {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(existing) = inner.touch(&key) {
            return existing;
        }

        inner.generation += 1;
        let access = inner.generation;
        inner.map.insert(
            key,
            CacheEntry {
                stmt: stmt.clone(),
                last_access: access,
            },
        );
        let evicted = inner.evict_if_needed();
        if evicted > 0 {
            self.evictions.fetch_add(evicted, Ordering::Relaxed);
        }
        stmt
    }

    pub(super) fn remove(&self, key: &str) -> Option<Statement> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.map.remove(key).map(|e| e.stmt)
    }

    pub(super) fn stats(&self) -> StmtCacheStats {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        StmtCacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            size: inner.map.len(),
            capacity: inner.capacity,
        }
    }
}

#[derive(Debug)]
pub(super) enum StmtCacheProbe {
    Disabled,
    Hit(Statement),
    Miss,
}

impl StmtCacheProbe {
    /// Populate query context fields based on the cache probe result.
    pub(super) fn populate_context(&self, ctx: &mut crate::monitor::QueryContext) {
        match self {
            StmtCacheProbe::Disabled => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "disabled".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "false".to_string());
            }
            StmtCacheProbe::Hit(_) => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "hit".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
            StmtCacheProbe::Miss => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "miss".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
        }
    }
}

/// PostgreSQL SQLSTATE: feature_not_supported (class 0A)
const SQLSTATE_FEATURE_NOT_SUPPORTED: &str = "0A000";
/// PostgreSQL SQLSTATE: invalid_sql_statement_name (class 26)
const SQLSTATE_INVALID_SQL_STATEMENT_NAME: &str = "26000";

/// Check whether a query error is retryable by re-preparing the statement.
///
/// This returns `true` for:
/// - `0A000` with "cached plan must not change result type" (schema changed under a cached plan)
/// - `26000` invalid_sql_statement_name (stale prepared statement reference)
pub(super) fn is_retryable_prepared_error(err: &OrmError) -> bool {
    let OrmError::Query(e) = err else {
        return false;
    };
    let Some(db_err) = e.as_db_error() else {
        return false;
    };

    match db_err.code().code() {
        SQLSTATE_FEATURE_NOT_SUPPORTED => db_err
            .message()
            .to_ascii_lowercase()
            .contains("cached plan must not change result type"),
        SQLSTATE_INVALID_SQL_STATEMENT_NAME => true,
        _ => false,
    }
}
