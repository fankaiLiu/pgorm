//! SELECT query builder using the unified expression layer.

use crate::client::GenericClient;
use crate::error::{OrmError, OrmResult};
use crate::qb::expr::{Expr, ExprGroup};
use crate::qb::param::ParamList;
use crate::qb::traits::SqlQb;
use crate::row::FromRow;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// SELECT query builder with unified expression-based WHERE/HAVING.
#[derive(Clone, Debug)]
pub struct SelectQb {
    /// Table or FROM expression
    from_expr: String,
    /// SELECT columns (default ["*"])
    select_cols: Vec<String>,
    /// JOIN clauses
    join_clauses: Vec<String>,
    /// WHERE conditions
    where_group: ExprGroup,
    /// ORDER BY clauses
    order_clauses: Vec<String>,
    /// GROUP BY clause
    group_by: Option<String>,
    /// HAVING conditions
    having_group: ExprGroup,
    /// LIMIT
    limit: Option<i64>,
    /// OFFSET
    offset: Option<i64>,
    /// Build error
    build_error: Option<String>,
}

impl SelectQb {
    /// Create a new SELECT query builder for a table.
    pub fn new(table: &str) -> Self {
        Self {
            from_expr: table.to_string(),
            select_cols: vec!["*".to_string()],
            join_clauses: Vec::new(),
            where_group: ExprGroup::new(),
            order_clauses: Vec::new(),
            group_by: None,
            having_group: ExprGroup::new(),
            limit: None,
            offset: None,
            build_error: None,
        }
    }

    /// Create a SELECT query builder with a custom FROM expression.
    pub fn from(from_expr: &str) -> Self {
        Self {
            from_expr: from_expr.to_string(),
            select_cols: vec!["*".to_string()],
            join_clauses: Vec::new(),
            where_group: ExprGroup::new(),
            order_clauses: Vec::new(),
            group_by: None,
            having_group: ExprGroup::new(),
            limit: None,
            offset: None,
            build_error: None,
        }
    }

    // ==================== SELECT columns ====================

    /// Set SELECT columns (string form, supports complex expressions).
    pub fn select(mut self, cols: &str) -> Self {
        self.select_cols = vec![cols.to_string()];
        self
    }

    /// Set SELECT columns (array form).
    pub fn select_cols(mut self, cols: &[&str]) -> Self {
        self.select_cols = cols.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Append one SELECT column.
    pub fn add_select(mut self, col: &str) -> Self {
        if self.select_cols.len() == 1 && self.select_cols[0] == "*" {
            self.select_cols[0] = col.to_string();
        } else {
            self.select_cols.push(col.to_string());
        }
        self
    }

    /// Append multiple SELECT columns.
    pub fn add_select_cols(mut self, cols: &[&str]) -> Self {
        for col in cols {
            if self.select_cols.len() == 1 && self.select_cols[0] == "*" {
                self.select_cols[0] = col.to_string();
            } else {
                self.select_cols.push(col.to_string());
            }
        }
        self
    }

    // ==================== JOIN ====================

    /// Add INNER JOIN.
    pub fn inner_join(mut self, table: &str, on: &str) -> Self {
        self.join_clauses.push(format!("INNER JOIN {} ON {}", table, on));
        self
    }

    /// Add LEFT JOIN.
    pub fn left_join(mut self, table: &str, on: &str) -> Self {
        self.join_clauses.push(format!("LEFT JOIN {} ON {}", table, on));
        self
    }

    /// Add RIGHT JOIN.
    pub fn right_join(mut self, table: &str, on: &str) -> Self {
        self.join_clauses.push(format!("RIGHT JOIN {} ON {}", table, on));
        self
    }

    /// Add FULL OUTER JOIN.
    pub fn full_join(mut self, table: &str, on: &str) -> Self {
        self.join_clauses.push(format!("FULL OUTER JOIN {} ON {}", table, on));
        self
    }

    // ==================== WHERE conditions (consuming builder) ====================

    /// Add WHERE: column = value
    pub fn eq<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.eq(column, value);
        self
    }

    /// Add WHERE: column != value
    pub fn ne<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.ne(column, value);
        self
    }

    /// Add WHERE: column > value
    pub fn gt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.gt(column, value);
        self
    }

    /// Add WHERE: column >= value
    pub fn gte<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.gte(column, value);
        self
    }

    /// Add WHERE: column < value
    pub fn lt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.lt(column, value);
        self
    }

    /// Add WHERE: column <= value
    pub fn lte<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.where_group.lte(column, value);
        self
    }

    /// Add WHERE: column LIKE pattern
    pub fn like<T: ToSql + Send + Sync + 'static>(mut self, column: &str, pattern: T) -> Self {
        self.where_group.like(column, pattern);
        self
    }

    /// Add WHERE: column ILIKE pattern (case-insensitive)
    pub fn ilike<T: ToSql + Send + Sync + 'static>(mut self, column: &str, pattern: T) -> Self {
        self.where_group.ilike(column, pattern);
        self
    }

    /// Add WHERE: column NOT LIKE pattern
    pub fn not_like<T: ToSql + Send + Sync + 'static>(mut self, column: &str, pattern: T) -> Self {
        self.where_group.not_like(column, pattern);
        self
    }

    /// Add WHERE: column NOT ILIKE pattern
    pub fn not_ilike<T: ToSql + Send + Sync + 'static>(mut self, column: &str, pattern: T) -> Self {
        self.where_group.not_ilike(column, pattern);
        self
    }

    /// Add WHERE: column IS NULL
    pub fn is_null(mut self, column: &str) -> Self {
        self.where_group.is_null(column);
        self
    }

    /// Add WHERE: column IS NOT NULL
    pub fn is_not_null(mut self, column: &str) -> Self {
        self.where_group.is_not_null(column);
        self
    }

    /// Add WHERE: column IN (values...)
    pub fn in_list<T: ToSql + Send + Sync + 'static>(mut self, column: &str, values: Vec<T>) -> Self {
        self.where_group.in_list(column, values);
        self
    }

    /// Add WHERE: column NOT IN (values...)
    pub fn not_in<T: ToSql + Send + Sync + 'static>(mut self, column: &str, values: Vec<T>) -> Self {
        self.where_group.not_in(column, values);
        self
    }

    /// Add WHERE: column BETWEEN from AND to
    pub fn between<T: ToSql + Send + Sync + 'static>(mut self, column: &str, from: T, to: T) -> Self {
        self.where_group.between(column, from, to);
        self
    }

    /// Add WHERE: column NOT BETWEEN from AND to
    pub fn not_between<T: ToSql + Send + Sync + 'static>(mut self, column: &str, from: T, to: T) -> Self {
        self.where_group.not_between(column, from, to);
        self
    }

    /// Add a raw WHERE condition without params.
    pub fn raw(mut self, sql: &str) -> Self {
        self.where_group.raw(sql);
        self
    }

    /// Add a WHERE condition with `?` placeholders.
    pub fn where_template<T: ToSql + Send + Sync + 'static>(mut self, sql: &str, values: Vec<T>) -> Self {
        self.where_group.template(sql, values);
        self
    }

    /// Add multi-column ILIKE search (OR).
    pub fn multi_ilike<T: ToSql + Send + Sync + Clone + 'static>(mut self, columns: &[&str], pattern: T) -> Self {
        self.where_group.multi_ilike(columns, pattern);
        self
    }

    /// Add a custom expression.
    pub fn and_expr(mut self, expr: Expr) -> Self {
        self.where_group.and_expr(expr);
        self
    }

    // ==================== Optional value helpers ====================

    /// Add WHERE if value is Some: column = value
    pub fn eq_opt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: Option<T>) -> Self {
        self.where_group.eq_opt(column, value);
        self
    }

    /// Add WHERE if value is Some: column LIKE pattern
    pub fn like_opt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, pattern: Option<T>) -> Self {
        self.where_group.like_opt(column, pattern);
        self
    }

    /// Add WHERE if value is Some: column ILIKE pattern
    pub fn ilike_opt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, pattern: Option<T>) -> Self {
        self.where_group.ilike_opt(column, pattern);
        self
    }

    /// Add WHERE if value is Some: column > value
    pub fn gt_opt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: Option<T>) -> Self {
        self.where_group.gt_opt(column, value);
        self
    }

    /// Add WHERE if value is Some: column >= value
    pub fn gte_opt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: Option<T>) -> Self {
        self.where_group.gte_opt(column, value);
        self
    }

    /// Add WHERE if value is Some: column < value
    pub fn lt_opt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: Option<T>) -> Self {
        self.where_group.lt_opt(column, value);
        self
    }

    /// Add WHERE if value is Some: column <= value
    pub fn lte_opt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: Option<T>) -> Self {
        self.where_group.lte_opt(column, value);
        self
    }

    /// Add WHERE if values is Some and non-empty: column IN (values...)
    pub fn in_opt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, values: Option<Vec<T>>) -> Self {
        self.where_group.in_opt(column, values);
        self
    }

    /// Add multi-column ILIKE if pattern is Some.
    pub fn multi_ilike_opt<T: ToSql + Send + Sync + Clone + 'static>(mut self, columns: &[&str], pattern: Option<T>) -> Self {
        self.where_group.multi_ilike_opt(columns, pattern);
        self
    }

    // ==================== Ordering & Grouping ====================

    /// Add ORDER BY clause.
    pub fn order_by(mut self, clause: &str) -> Self {
        self.order_clauses.push(clause.to_string());
        self
    }

    /// Add ORDER BY column ASC.
    pub fn order_by_asc(mut self, column: &str) -> Self {
        self.order_clauses.push(format!("{} ASC", column));
        self
    }

    /// Add ORDER BY column DESC.
    pub fn order_by_desc(mut self, column: &str) -> Self {
        self.order_clauses.push(format!("{} DESC", column));
        self
    }

    /// Set GROUP BY clause.
    pub fn group_by(mut self, clause: &str) -> Self {
        self.group_by = Some(clause.to_string());
        self
    }

    /// Add HAVING condition: column = value
    pub fn having_eq<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.having_group.eq(column, value);
        self
    }

    /// Add HAVING condition: column > value
    pub fn having_gt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.having_group.gt(column, value);
        self
    }

    /// Add HAVING condition: column >= value
    pub fn having_gte<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.having_group.gte(column, value);
        self
    }

    /// Add HAVING condition: column < value
    pub fn having_lt<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.having_group.lt(column, value);
        self
    }

    /// Add HAVING condition: column <= value
    pub fn having_lte<T: ToSql + Send + Sync + 'static>(mut self, column: &str, value: T) -> Self {
        self.having_group.lte(column, value);
        self
    }

    /// Add HAVING condition with template.
    pub fn having_template<T: ToSql + Send + Sync + 'static>(mut self, sql: &str, values: Vec<T>) -> Self {
        self.having_group.template(sql, values);
        self
    }

    // ==================== Pagination ====================

    /// Set LIMIT.
    pub fn limit(mut self, n: i64) -> Self {
        self.limit = Some(n);
        self
    }

    /// Set OFFSET.
    pub fn offset(mut self, n: i64) -> Self {
        self.offset = Some(n);
        self
    }

    /// Pagination helper.
    ///
    /// `page` is 1-based (clamped to >= 1).
    /// `per_page` is clamped to >= 1.
    pub fn paginate(mut self, page: i64, per_page: i64) -> Self {
        let p = page.max(1);
        let size = per_page.max(1);
        self.limit = Some(size);
        self.offset = Some((p - 1) * size);
        self
    }

    /// Set page (1-based).
    pub fn page(mut self, page: i64) -> Self {
        let p = page.max(1);
        let per_page = self.limit.unwrap_or(10);
        self.offset = Some((p - 1) * per_page);
        self
    }

    /// Set per page (items per page).
    pub fn per_page(mut self, per_page: i64) -> Self {
        let size = per_page.max(1);
        self.limit = Some(size);
        self
    }

    // ==================== Build helpers ====================

    /// Build the main SELECT SQL.
    fn build_select_sql(&self, is_count: bool) -> (String, ParamList) {
        let mut params = ParamList::new();

        let select_part = if is_count {
            "COUNT(*)".to_string()
        } else {
            self.select_cols.join(", ")
        };

        let mut sql = format!("SELECT {} FROM {}", select_part, self.from_expr);

        // JOINs
        for join in &self.join_clauses {
            sql.push(' ');
            sql.push_str(join);
        }

        // WHERE
        let (where_sql, where_params) = self.where_group.build();
        if !where_sql.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
        }
        params.extend(&where_params);

        // GROUP BY
        if let Some(ref group) = self.group_by {
            sql.push_str(" GROUP BY ");
            sql.push_str(group);
        }

        // HAVING
        if !self.having_group.is_empty() {
            let (having_sql, having_params) = self.having_group.build_with_offset(params.len());
            if !having_sql.is_empty() {
                sql.push_str(" HAVING ");
                sql.push_str(&having_sql);
            }
            params.extend(&having_params);
        }

        // ORDER BY, LIMIT, OFFSET (not for COUNT)
        if !is_count {
            if !self.order_clauses.is_empty() {
                sql.push_str(" ORDER BY ");
                sql.push_str(&self.order_clauses.join(", "));
            }

            if let Some(limit) = self.limit {
                sql.push_str(&format!(" LIMIT {}", limit));
            }

            if let Some(offset) = self.offset {
                sql.push_str(&format!(" OFFSET {}", offset));
            }
        }

        (sql, params)
    }

    /// Build COUNT SQL for complex queries with GROUP BY/HAVING.
    fn build_count_sql(&self) -> (String, ParamList) {
        if self.group_by.is_some() || !self.having_group.is_empty() {
            // Wrap in subquery
            let mut params = ParamList::new();

            let mut inner_sql = format!("SELECT 1 FROM {}", self.from_expr);

            for join in &self.join_clauses {
                inner_sql.push(' ');
                inner_sql.push_str(join);
            }

            let (where_sql, where_params) = self.where_group.build();
            if !where_sql.is_empty() {
                inner_sql.push_str(" WHERE ");
                inner_sql.push_str(&where_sql);
            }
            params.extend(&where_params);

            if let Some(ref group) = self.group_by {
                inner_sql.push_str(" GROUP BY ");
                inner_sql.push_str(group);
            }

            if !self.having_group.is_empty() {
                let (having_sql, having_params) = self.having_group.build_with_offset(params.len());
                if !having_sql.is_empty() {
                    inner_sql.push_str(" HAVING ");
                    inner_sql.push_str(&having_sql);
                }
                params.extend(&having_params);
            }

            let sql = format!("SELECT COUNT(*) FROM ({}) AS t", inner_sql);
            (sql, params)
        } else {
            self.build_select_sql(true)
        }
    }

    /// Get the built SQL string (for debugging).
    pub fn to_sql(&self) -> String {
        self.build_select_sql(false).0
    }

    /// Get the COUNT SQL string (for debugging).
    pub fn to_count_sql(&self) -> String {
        self.build_count_sql().0
    }

    // ==================== Execution ====================

    /// Execute COUNT query.
    pub async fn count(&self, conn: &impl GenericClient) -> OrmResult<i64> {
        self.validate()?;
        let (sql, params) = self.build_count_sql();
        let params_ref = params.as_refs();
        let row = conn.query_one(&sql, &params_ref).await?;
        Ok(row.get(0))
    }
}

impl SqlQb for SelectQb {
    fn build_sql(&self) -> String {
        self.build_select_sql(false).0
    }

    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        // This method can't return proper references since we build params on the fly.
        // The actual execution methods build params themselves.
        vec![]
    }

    fn validate(&self) -> OrmResult<()> {
        if let Some(ref err) = self.build_error {
            return Err(OrmError::Validation(err.clone()));
        }
        Ok(())
    }

    fn query(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Vec<Row>>> + Send {
        async move {
            self.validate()?;
            let (sql, params) = self.build_select_sql(false);
            let params_ref = params.as_refs();
            conn.query(&sql, &params_ref).await
        }
    }

    fn query_opt(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Option<Row>>> + Send {
        async move {
            self.validate()?;
            let (sql, params) = self.build_select_sql(false);
            let params_ref = params.as_refs();
            conn.query_opt(&sql, &params_ref).await
        }
    }

    fn query_one(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Row>> + Send {
        async move {
            self.validate()?;
            let (sql, params) = self.build_select_sql(false);
            let params_ref = params.as_refs();
            conn.query_one(&sql, &params_ref).await
        }
    }

    fn fetch_all<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Vec<T>>> + Send {
        async move {
            let rows = self.query(conn).await?;
            rows.iter().map(T::from_row).collect()
        }
    }

    fn fetch_opt<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<Option<T>>> + Send {
        async move {
            let row = self.query_opt(conn).await?;
            row.as_ref().map(T::from_row).transpose()
        }
    }

    fn fetch_one<T: FromRow>(
        &self,
        conn: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<T>> + Send {
        async move {
            let row = self.query_one(conn).await?;
            T::from_row(&row)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_select() {
        let qb = SelectQb::new("users");
        assert_eq!(qb.to_sql(), "SELECT * FROM users");
    }

    #[test]
    fn test_select_with_columns() {
        let qb = SelectQb::new("users").select("id, name, email");
        assert_eq!(qb.to_sql(), "SELECT id, name, email FROM users");
    }

    #[test]
    fn test_select_with_where() {
        let qb = SelectQb::new("users")
            .eq("status", "active")
            .gt("age", 18i32);
        assert_eq!(qb.to_sql(), "SELECT * FROM users WHERE status = $1 AND age > $2");
    }

    #[test]
    fn test_select_with_join() {
        let qb = SelectQb::new("users u")
            .inner_join("orders o", "u.id = o.user_id")
            .eq("u.status", "active");
        assert_eq!(
            qb.to_sql(),
            "SELECT * FROM users u INNER JOIN orders o ON u.id = o.user_id WHERE u.status = $1"
        );
    }

    #[test]
    fn test_select_with_order_and_limit() {
        let qb = SelectQb::new("users")
            .order_by("created_at DESC")
            .limit(10)
            .offset(20);
        assert_eq!(
            qb.to_sql(),
            "SELECT * FROM users ORDER BY created_at DESC LIMIT 10 OFFSET 20"
        );
    }

    #[test]
    fn test_select_with_group_by() {
        let qb = SelectQb::new("orders")
            .select("user_id, COUNT(*) as order_count")
            .group_by("user_id")
            .having_gt("COUNT(*)", 5i64);
        let sql = qb.to_sql();
        assert!(sql.contains("GROUP BY user_id"));
        assert!(sql.contains("HAVING COUNT(*) > $1"));
    }

    #[test]
    fn test_count_sql() {
        let qb = SelectQb::new("users").eq("status", "active");
        assert_eq!(qb.to_count_sql(), "SELECT COUNT(*) FROM users WHERE status = $1");
    }

    #[test]
    fn test_count_with_group_by() {
        let qb = SelectQb::new("orders")
            .group_by("user_id")
            .having_gt("COUNT(*)", 5i64);
        let sql = qb.to_count_sql();
        assert!(sql.starts_with("SELECT COUNT(*) FROM ("));
        assert!(sql.contains("GROUP BY user_id"));
    }

    #[test]
    fn test_paginate() {
        let qb = SelectQb::new("users").paginate(2, 10);
        assert_eq!(qb.to_sql(), "SELECT * FROM users LIMIT 10 OFFSET 10");
    }

    #[test]
    fn test_in_list() {
        let qb = SelectQb::new("users").in_list("id", vec![1i64, 2, 3]);
        assert_eq!(qb.to_sql(), "SELECT * FROM users WHERE id IN ($1, $2, $3)");
    }

    #[test]
    fn test_between() {
        let qb = SelectQb::new("users").between("age", 18i32, 65i32);
        assert_eq!(qb.to_sql(), "SELECT * FROM users WHERE age BETWEEN $1 AND $2");
    }

    #[test]
    fn test_multi_ilike() {
        let qb = SelectQb::new("users").multi_ilike(&["name", "email"], "%test%");
        let sql = qb.to_sql();
        assert!(sql.contains("name ILIKE $1 OR email ILIKE $2"));
    }

    #[test]
    fn test_optional_conditions() {
        let status: Option<&str> = Some("active");
        let name: Option<&str> = None;

        let qb = SelectQb::new("users")
            .eq_opt("status", status)
            .eq_opt("name", name);

        assert_eq!(qb.to_sql(), "SELECT * FROM users WHERE status = $1");
    }
}
