use crate::error::OrmError;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use tokio_postgres::Statement;

#[derive(Debug)]
pub(super) struct StatementCache {
    inner: Mutex<StatementCacheInner>,
}

#[derive(Debug)]
struct StatementCacheInner {
    capacity: usize,
    map: HashMap<String, Statement>,
    order: VecDeque<String>,
}

impl StatementCache {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(StatementCacheInner {
                capacity,
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    pub(super) fn get(&self, key: &str) -> Option<Statement> {
        let mut inner = self.inner.lock().unwrap();
        let stmt = inner.map.get(key).cloned()?;
        inner.touch(key);
        Some(stmt)
    }

    pub(super) fn insert_if_absent(&self, key: String, stmt: Statement) -> Statement {
        let mut inner = self.inner.lock().unwrap();

        if let Some(existing) = inner.map.get(&key).cloned() {
            inner.touch(&key);
            return existing;
        }

        inner.map.insert(key.clone(), stmt.clone());
        inner.order.push_back(key);
        inner.evict_if_needed();
        stmt
    }

    pub(super) fn remove(&self, key: &str) -> Option<Statement> {
        let mut inner = self.inner.lock().unwrap();
        let removed = inner.map.remove(key);
        if removed.is_some() {
            inner.remove_from_order(key);
        }
        removed
    }
}

impl StatementCacheInner {
    fn touch(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k.as_str() == key) {
            if let Some(k) = self.order.remove(pos) {
                self.order.push_back(k);
            }
        }
    }

    fn remove_from_order(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k.as_str() == key) {
            let _ = self.order.remove(pos);
        }
    }

    fn evict_if_needed(&mut self) {
        if self.capacity == 0 {
            self.map.clear();
            self.order.clear();
            return;
        }

        while self.map.len() > self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            let _ = self.map.remove(&oldest);
        }
    }
}

#[derive(Debug)]
pub(super) enum StmtCacheProbe {
    Disabled,
    Hit(Statement),
    Miss,
}

pub(super) fn is_retryable_prepared_error(err: &OrmError) -> bool {
    let OrmError::Query(e) = err else {
        return false;
    };
    let Some(db_err) = e.as_db_error() else {
        return false;
    };

    match db_err.code().code() {
        // "cached plan must not change result type" (e.g. after schema change)
        "0A000" => db_err
            .message()
            .to_ascii_lowercase()
            .contains("cached plan must not change result type"),
        // invalid_sql_statement_name
        "26000" => true,
        _ => false,
    }
}
