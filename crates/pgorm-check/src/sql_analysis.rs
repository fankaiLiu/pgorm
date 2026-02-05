use crate::sql_lint::{ParseResult, StatementKind};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeVarRef {
    pub catalog: Option<String>,
    pub schema: Option<String>,
    pub table: String,
    pub alias: Option<String>,
    pub location: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRefFull {
    /// Name parts, excluding `*`.
    pub parts: Vec<String>,
    pub has_star: bool,
    pub location: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetColumn {
    pub name: String,
    pub location: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnConflictAnalysis {
    pub constraint_name: Option<String>,
    pub inference_columns: Vec<TargetColumn>,
    pub has_inference_expressions: bool,
    pub update_set_columns: Vec<TargetColumn>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertAnalysis {
    pub target: Option<RangeVarRef>,
    pub columns: Vec<TargetColumn>,
    pub on_conflict: Option<OnConflictAnalysis>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateAnalysis {
    pub target: Option<RangeVarRef>,
    pub set_columns: Vec<TargetColumn>,
}

#[derive(Debug, Clone)]
pub struct SqlAnalysis {
    pub parse_result: ParseResult,
    pub statement_kind: Option<StatementKind>,

    pub cte_names: HashSet<String>,
    pub range_vars: Vec<RangeVarRef>,
    pub table_names: Vec<String>,
    pub column_refs: Vec<ColumnRefFull>,

    pub select_has_limit: Option<bool>,
    pub select_has_star: Option<bool>,
    pub delete_has_where: Option<bool>,
    pub update_has_where: Option<bool>,

    pub insert: Option<InsertAnalysis>,
    pub update: Option<UpdateAnalysis>,
}

fn location_opt(loc: i32) -> Option<i32> {
    if loc < 0 { None } else { Some(loc) }
}

pub fn analyze_sql(sql: &str) -> SqlAnalysis {
    match pg_query::parse(sql) {
        Ok(parsed) => {
            let cte_names: HashSet<String> = parsed.cte_names.into_iter().collect();

            let mut statement_kind: Option<StatementKind> = None;
            let mut select_has_limit: Option<bool> = None;
            let mut delete_has_where: Option<bool> = None;
            let mut update_has_where: Option<bool> = None;
            let mut insert: Option<InsertAnalysis> = None;
            let mut update: Option<UpdateAnalysis> = None;

            let stmt_node = parsed
                .protobuf
                .stmts
                .first()
                .and_then(|s| s.stmt.as_ref())
                .and_then(|s| s.node.as_ref());

            if let Some(stmt) = stmt_node {
                use pg_query::NodeEnum;
                statement_kind = match stmt {
                    NodeEnum::SelectStmt(select) => {
                        select_has_limit =
                            Some(select.limit_count.is_some() || select.limit_offset.is_some());
                        Some(StatementKind::Select)
                    }
                    NodeEnum::InsertStmt(insert_stmt) => {
                        insert = Some(analyze_insert(insert_stmt));
                        Some(StatementKind::Insert)
                    }
                    NodeEnum::UpdateStmt(update_stmt) => {
                        update_has_where = Some(update_stmt.where_clause.is_some());
                        update = Some(analyze_update(update_stmt));
                        Some(StatementKind::Update)
                    }
                    NodeEnum::DeleteStmt(delete_stmt) => {
                        delete_has_where = Some(delete_stmt.where_clause.is_some());
                        Some(StatementKind::Delete)
                    }
                    NodeEnum::CreateStmt(_) => Some(StatementKind::CreateTable),
                    NodeEnum::AlterTableStmt(_) => Some(StatementKind::AlterTable),
                    NodeEnum::DropStmt(_) => Some(StatementKind::DropTable),
                    NodeEnum::IndexStmt(_) => Some(StatementKind::CreateIndex),
                    NodeEnum::TruncateStmt(_) => Some(StatementKind::Truncate),
                    NodeEnum::TransactionStmt(t) => match t.kind() {
                        pg_query::protobuf::TransactionStmtKind::TransStmtBegin => {
                            Some(StatementKind::Begin)
                        }
                        pg_query::protobuf::TransactionStmtKind::TransStmtCommit => {
                            Some(StatementKind::Commit)
                        }
                        pg_query::protobuf::TransactionStmtKind::TransStmtRollback => {
                            Some(StatementKind::Rollback)
                        }
                        _ => Some(StatementKind::Other),
                    },
                    _ => Some(StatementKind::Other),
                };
            }

            let mut range_vars = Vec::new();
            let mut table_names: Vec<String> = Vec::new();
            let mut column_refs = Vec::new();
            let mut has_star = false;

            for (node, _depth, _context, _has_filter_columns) in parsed.protobuf.nodes() {
                if let pg_query::NodeRef::RangeVar(v) = node {
                    let r = RangeVarRef {
                        catalog: if v.catalogname.is_empty() {
                            None
                        } else {
                            Some(v.catalogname.to_string())
                        },
                        schema: if v.schemaname.is_empty() {
                            None
                        } else {
                            Some(v.schemaname.to_string())
                        },
                        table: v.relname.to_string(),
                        alias: v.alias.as_ref().map(|a| a.aliasname.to_string()),
                        location: location_opt(v.location),
                    };

                    range_vars.push(r.clone());

                    // Skip CTE references.
                    if cte_names.contains(&r.table) {
                        continue;
                    }

                    let name = match &r.schema {
                        Some(s) => format!("{s}.{}", r.table),
                        None => r.table.clone(),
                    };
                    if !table_names.contains(&name) {
                        table_names.push(name);
                    }
                }

                if let pg_query::NodeRef::ColumnRef(c) = node {
                    let mut parts: Vec<String> = Vec::new();
                    let mut star = false;

                    for f in &c.fields {
                        match f.node.as_ref() {
                            Some(pg_query::NodeEnum::String(s)) => parts.push(s.sval.clone()),
                            Some(pg_query::NodeEnum::AStar(_)) => star = true,
                            _ => {}
                        }
                    }

                    if star {
                        has_star = true;
                    }

                    let col_ref = ColumnRefFull {
                        parts,
                        has_star: star,
                        location: location_opt(c.location),
                    };

                    if !column_refs.contains(&col_ref) {
                        column_refs.push(col_ref);
                    }
                }
            }

            let select_has_star = if statement_kind == Some(StatementKind::Select) {
                Some(has_star)
            } else {
                None
            };

            SqlAnalysis {
                parse_result: ParseResult {
                    valid: true,
                    error: None,
                    error_location: None,
                },
                statement_kind,
                cte_names,
                range_vars,
                table_names,
                column_refs,
                select_has_limit,
                select_has_star,
                delete_has_where,
                update_has_where,
                insert,
                update,
            }
        }
        Err(e) => {
            let error_str = e.to_string();
            let location = extract_error_location(&error_str);
            SqlAnalysis {
                parse_result: ParseResult {
                    valid: false,
                    error: Some(error_str),
                    error_location: location,
                },
                statement_kind: None,
                cte_names: HashSet::new(),
                range_vars: Vec::new(),
                table_names: Vec::new(),
                column_refs: Vec::new(),
                select_has_limit: None,
                select_has_star: None,
                delete_has_where: None,
                update_has_where: None,
                insert: None,
                update: None,
            }
        }
    }
}

fn analyze_insert(insert_stmt: &pg_query::protobuf::InsertStmt) -> InsertAnalysis {
    let target = insert_stmt.relation.as_ref().map(range_var_from_protobuf);

    let mut columns: Vec<TargetColumn> = Vec::new();
    for c in &insert_stmt.cols {
        if let Some(pg_query::NodeEnum::ResTarget(rt)) = c.node.as_ref() {
            if rt.name.is_empty() {
                continue;
            }
            let col = TargetColumn {
                name: rt.name.clone(),
                location: location_opt(rt.location),
            };
            if !columns.contains(&col) {
                columns.push(col);
            }
        }
    }

    let on_conflict = insert_stmt
        .on_conflict_clause
        .as_deref()
        .map(analyze_on_conflict);

    InsertAnalysis {
        target,
        columns,
        on_conflict,
    }
}

fn analyze_update(update_stmt: &pg_query::protobuf::UpdateStmt) -> UpdateAnalysis {
    let target = update_stmt.relation.as_ref().map(range_var_from_protobuf);

    let mut set_columns: Vec<TargetColumn> = Vec::new();
    for n in &update_stmt.target_list {
        if let Some(pg_query::NodeEnum::ResTarget(rt)) = n.node.as_ref() {
            if rt.name.is_empty() {
                continue;
            }
            let col = TargetColumn {
                name: rt.name.clone(),
                location: location_opt(rt.location),
            };
            if !set_columns.contains(&col) {
                set_columns.push(col);
            }
        }
    }

    UpdateAnalysis {
        target,
        set_columns,
    }
}

fn analyze_on_conflict(clause: &pg_query::protobuf::OnConflictClause) -> OnConflictAnalysis {
    let mut constraint_name: Option<String> = None;
    let mut inference_columns: Vec<TargetColumn> = Vec::new();
    let mut has_inference_expressions = false;

    if let Some(infer) = clause.infer.as_deref() {
        if !infer.conname.is_empty() {
            constraint_name = Some(infer.conname.clone());
        }

        for n in &infer.index_elems {
            if let Some(pg_query::NodeEnum::IndexElem(e)) = n.node.as_ref() {
                if !e.name.is_empty() && e.expr.is_none() {
                    let col = TargetColumn {
                        name: e.name.clone(),
                        location: None,
                    };
                    if !inference_columns.contains(&col) {
                        inference_columns.push(col);
                    }
                } else {
                    has_inference_expressions = true;
                }
            }
        }
    }

    let mut update_set_columns: Vec<TargetColumn> = Vec::new();
    for n in &clause.target_list {
        if let Some(pg_query::NodeEnum::ResTarget(rt)) = n.node.as_ref() {
            if rt.name.is_empty() {
                continue;
            }
            let col = TargetColumn {
                name: rt.name.clone(),
                location: location_opt(rt.location),
            };
            if !update_set_columns.contains(&col) {
                update_set_columns.push(col);
            }
        }
    }

    OnConflictAnalysis {
        constraint_name,
        inference_columns,
        has_inference_expressions,
        update_set_columns,
    }
}

fn range_var_from_protobuf(v: &pg_query::protobuf::RangeVar) -> RangeVarRef {
    RangeVarRef {
        catalog: if v.catalogname.is_empty() {
            None
        } else {
            Some(v.catalogname.clone())
        },
        schema: if v.schemaname.is_empty() {
            None
        } else {
            Some(v.schemaname.clone())
        },
        table: v.relname.clone(),
        alias: v.alias.as_ref().map(|a| a.aliasname.clone()),
        location: location_opt(v.location),
    }
}

/// Extract error location from pg_query error message.
fn extract_error_location(error: &str) -> Option<usize> {
    if let Some(pos) = error.rfind("position ") {
        let after_pos = &error[pos + 9..];
        let num_str: String = after_pos
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        return num_str.parse().ok();
    }
    None
}

#[derive(Debug)]
struct SqlParseCacheInner {
    map: HashMap<String, ParseCacheEntry>,
    generation: u64,
}

#[derive(Debug, Clone)]
struct ParseCacheEntry {
    analysis: Arc<SqlAnalysis>,
    last_access: u64,
}

impl SqlParseCacheInner {
    fn touch(&mut self, key: &str) -> Option<Arc<SqlAnalysis>> {
        let entry = self.map.get_mut(key)?;
        self.generation += 1;
        entry.last_access = self.generation;
        Some(Arc::clone(&entry.analysis))
    }

    fn evict_lru(&mut self, capacity: usize) -> u64 {
        let mut evicted = 0u64;
        while self.map.len() > capacity {
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

/// SQL parse cache statistics.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub size: usize,
    pub capacity: usize,
}

impl ParseCacheStats {
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

/// Thread-safe SQL parse cache (simple LRU).
#[derive(Debug)]
pub struct SqlParseCache {
    capacity: usize,
    inner: Mutex<SqlParseCacheInner>,
    hits: std::sync::atomic::AtomicU64,
    misses: std::sync::atomic::AtomicU64,
    evictions: std::sync::atomic::AtomicU64,
}

impl Default for SqlParseCache {
    fn default() -> Self {
        Self::new(256)
    }
}

impl SqlParseCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            inner: Mutex::new(SqlParseCacheInner {
                map: HashMap::with_capacity(capacity),
                generation: 0,
            }),
            hits: std::sync::atomic::AtomicU64::new(0),
            misses: std::sync::atomic::AtomicU64::new(0),
            evictions: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn analyze(&self, sql: &str) -> Arc<SqlAnalysis> {
        use std::sync::atomic::Ordering;

        if self.capacity == 0 {
            return Arc::new(analyze_sql(sql));
        }

        {
            let mut inner = self.inner.lock().expect("sql parse cache mutex poisoned");
            if let Some(found) = inner.touch(sql) {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return found;
            }
        }

        self.misses.fetch_add(1, Ordering::Relaxed);

        // Parse outside the lock to reduce contention.
        let analysis = Arc::new(analyze_sql(sql));

        let mut inner = self.inner.lock().expect("sql parse cache mutex poisoned");
        // Double-check: another thread may have inserted while we parsed.
        if let Some(found) = inner.touch(sql) {
            return found;
        }

        inner.generation += 1;
        let access = inner.generation;
        inner.map.insert(
            sql.to_string(),
            ParseCacheEntry {
                analysis: Arc::clone(&analysis),
                last_access: access,
            },
        );
        let evicted = inner.evict_lru(self.capacity);
        if evicted > 0 {
            self.evictions.fetch_add(evicted, Ordering::Relaxed);
        }

        analysis
    }

    /// Get cache statistics.
    pub fn stats(&self) -> ParseCacheStats {
        use std::sync::atomic::Ordering;
        let inner = self.inner.lock().expect("sql parse cache mutex poisoned");
        ParseCacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            size: inner.map.len(),
            capacity: self.capacity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sql_parse_cache_hits_and_eviction() {
        let cache = SqlParseCache::new(2);

        let a1 = cache.analyze("SELECT 1");
        let a2 = cache.analyze("SELECT 1");
        assert!(Arc::ptr_eq(&a1, &a2));

        let _b = cache.analyze("SELECT 2");
        let _c = cache.analyze("SELECT 3"); // evicts least-recently used

        let a3 = cache.analyze("SELECT 1");
        assert!(!Arc::ptr_eq(&a1, &a3));
    }

    #[test]
    fn test_analyze_insert_update_on_conflict_columns() {
        let insert_sql = "INSERT INTO users (id, name) VALUES (1, 'a') ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name";
        let analysis = analyze_sql(insert_sql);
        assert!(analysis.parse_result.valid);
        assert_eq!(analysis.statement_kind, Some(StatementKind::Insert));

        let insert = analysis.insert.as_ref().expect("insert analysis");
        assert_eq!(
            insert
                .columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["id", "name"]
        );

        let oc = insert.on_conflict.as_ref().expect("on conflict analysis");
        assert_eq!(
            oc.inference_columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["id"]
        );
        assert_eq!(
            oc.update_set_columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["name"]
        );

        let update_sql = "UPDATE users SET name = 'b', age = 2 WHERE id = 1";
        let analysis = analyze_sql(update_sql);
        assert!(analysis.parse_result.valid);
        assert_eq!(analysis.statement_kind, Some(StatementKind::Update));

        let update = analysis.update.as_ref().expect("update analysis");
        assert_eq!(
            update
                .set_columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["name", "age"]
        );
    }
}
