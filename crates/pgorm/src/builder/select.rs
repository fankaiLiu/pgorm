use super::traits::SqlBuilder;
use crate::client::GenericClient;
use crate::error::{OrmError, OrmResult};
use tokio_postgres::types::ToSql;

/// Structured SELECT query builder.
pub struct QueryBuilder {
    /// Main table expression
    table: String,
    /// SELECT columns (default ["*"])
    select_cols: Vec<String>,
    /// JOIN clauses
    join_clauses: Vec<String>,
    /// WHERE conditions (without leading AND)
    where_conditions: Vec<String>,
    /// ORDER BY clauses
    order_clauses: Vec<String>,
    /// GROUP BY clause
    group_by: Option<String>,
    /// HAVING conditions
    having_conditions: Vec<String>,
    /// LIMIT
    limit: Option<i64>,
    /// OFFSET
    offset: Option<i64>,
    /// Params
    params: Vec<Box<dyn ToSql + Sync + Send>>,
    /// Current param counter
    param_count: usize,
    /// Build error (validated at runtime)
    build_error: Option<String>,
}

impl QueryBuilder {
    /// Create a new query builder.
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            select_cols: vec!["*".to_string()],
            join_clauses: Vec::new(),
            where_conditions: Vec::new(),
            order_clauses: Vec::new(),
            group_by: None,
            having_conditions: Vec::new(),
            limit: None,
            offset: None,
            params: Vec::new(),
            param_count: 0,
            build_error: None,
        }
    }

    /// Set SELECT columns (string form, supports complex expressions).
    pub fn select(&mut self, cols: &str) -> &mut Self {
        self.select_cols = vec![cols.to_string()];
        self
    }

    /// Set SELECT columns (array form, good for constants).
    pub fn select_cols(&mut self, cols: &[&str]) -> &mut Self {
        self.select_cols = cols.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Append one SELECT column.
    pub fn add_select(&mut self, col: &str) -> &mut Self {
        if self.select_cols.len() == 1 && self.select_cols[0] == "*" {
            self.select_cols[0] = col.to_string();
        } else {
            self.select_cols.push(col.to_string());
        }
        self
    }

    /// Append multiple SELECT columns.
    pub fn add_select_cols(&mut self, cols: &[&str]) -> &mut Self {
        for col in cols {
            self.add_select(col);
        }
        self
    }

    /// Add INNER JOIN.
    pub fn inner_join(&mut self, table: &str, on: &str) -> &mut Self {
        self.join_clauses
            .push(format!("INNER JOIN {} ON {}", table, on));
        self
    }

    /// Add LEFT JOIN.
    pub fn left_join(&mut self, table: &str, on: &str) -> &mut Self {
        self.join_clauses
            .push(format!("LEFT JOIN {} ON {}", table, on));
        self
    }

    /// Add RIGHT JOIN.
    pub fn right_join(&mut self, table: &str, on: &str) -> &mut Self {
        self.join_clauses
            .push(format!("RIGHT JOIN {} ON {}", table, on));
        self
    }

    /// Add FULL OUTER JOIN.
    pub fn full_join(&mut self, table: &str, on: &str) -> &mut Self {
        self.join_clauses
            .push(format!("FULL OUTER JOIN {} ON {}", table, on));
        self
    }

    // ==================== Conditions ====================

    fn add_condition<T>(&mut self, sql_template: &str, value: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.param_count += 1;
        let placeholder = format!("${}", self.param_count);
        let condition = sql_template.replacen('$', &placeholder, 1);
        self.where_conditions.push(condition);
        self.params.push(Box::new(value));
        self
    }

    /// Add AND equality condition.
    pub fn and_eq<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} = $", col), val)
    }

    /// Add AND not-equal condition.
    pub fn and_ne<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} != $", col), val)
    }

    /// Add AND LIKE condition.
    pub fn and_like<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} LIKE $", col), val)
    }

    /// Add AND ILIKE condition.
    pub fn and_ilike<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} ILIKE $", col), val)
    }

    /// Add AND > condition.
    pub fn and_gt<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} > $", col), val)
    }

    /// Add AND >= condition.
    pub fn and_gte<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} >= $", col), val)
    }

    /// Add AND < condition.
    pub fn and_lt<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} < $", col), val)
    }

    /// Add AND <= condition.
    pub fn and_lte<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} <= $", col), val)
    }

    /// Add AND IS NULL condition.
    pub fn and_is_null(&mut self, col: &str) -> &mut Self {
        self.where_conditions.push(format!("{} IS NULL", col));
        self
    }

    /// Add AND IS NOT NULL condition.
    pub fn and_is_not_null(&mut self, col: &str) -> &mut Self {
        self.where_conditions.push(format!("{} IS NOT NULL", col));
        self
    }

    /// Add AND IN (...) condition.
    pub fn and_in<T>(&mut self, col: &str, values: Vec<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if values.is_empty() {
            self.where_conditions.push("1=0".to_string());
            return self;
        }

        let mut placeholders = Vec::new();
        for value in values {
            self.param_count += 1;
            placeholders.push(format!("${}", self.param_count));
            self.params.push(Box::new(value));
        }

        self.where_conditions
            .push(format!("{} IN ({})", col, placeholders.join(", ")));
        self
    }

    /// Add AND NOT IN (...) condition.
    pub fn and_not_in<T>(&mut self, col: &str, values: Vec<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if values.is_empty() {
            return self;
        }

        let mut placeholders = Vec::new();
        for value in values {
            self.param_count += 1;
            placeholders.push(format!("${}", self.param_count));
            self.params.push(Box::new(value));
        }

        self.where_conditions.push(format!(
            "{} NOT IN ({})",
            col,
            placeholders.join(", ")
        ));
        self
    }

    /// Add AND BETWEEN condition.
    pub fn and_between<T>(&mut self, col: &str, from: T, to: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.param_count += 1;
        let p1 = format!("${}", self.param_count);
        self.params.push(Box::new(from));

        self.param_count += 1;
        let p2 = format!("${}", self.param_count);
        self.params.push(Box::new(to));

        self.where_conditions
            .push(format!("{} BETWEEN {} AND {}", col, p1, p2));
        self
    }

    /// Add a raw WHERE condition without params.
    ///
    /// # Safety
    ///
    /// This directly concatenates SQL. The caller must ensure safety.
    pub fn and_raw(&mut self, sql: &str) -> &mut Self {
        self.where_conditions.push(sql.to_string());
        self
    }

    /// Add a complex condition with multiple params using `?` placeholders.
    pub fn and_where<T>(&mut self, sql_template: &str, values: Vec<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        let placeholder_count = sql_template.matches('?').count();
        if placeholder_count != values.len() {
            self.build_error = Some(format!(
                "QueryBuilder param mismatch: template '{}' has {} '?', but {} values provided",
                sql_template,
                placeholder_count,
                values.len()
            ));
            // Do NOT modify state if there is a mismatch.
            return self;
        }

        let mut final_sql = sql_template.to_string();
        for v in values {
            self.param_count += 1;
            let placeholder = format!("${}", self.param_count);
            final_sql = final_sql.replacen('?', &placeholder, 1);
            self.params.push(Box::new(v));
        }
        self.where_conditions.push(format!("({})", final_sql));
        self
    }

    /// Add multi-column ILIKE search (OR).
    pub fn and_multi_ilike<T>(&mut self, columns: &[&str], pattern: T) -> &mut Self
    where
        T: ToSql + Sync + Send + Clone + 'static,
    {
        if columns.is_empty() {
            return self;
        }

        let mut conditions = Vec::new();
        for col in columns {
            self.param_count += 1;
            conditions.push(format!("{} ILIKE ${}", col, self.param_count));
            self.params.push(Box::new(pattern.clone()));
        }

        self.where_conditions
            .push(format!("({})", conditions.join(" OR ")));
        self
    }

    // ==================== Option-friendly helpers ====================

    pub fn and_eq_opt<T>(&mut self, col: &str, val: Option<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_eq(col, v);
        }
        self
    }

    pub fn and_like_opt<T>(&mut self, col: &str, val: Option<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_like(col, v);
        }
        self
    }

    pub fn and_ilike_opt<T>(&mut self, col: &str, val: Option<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_ilike(col, v);
        }
        self
    }

    pub fn and_gt_opt<T>(&mut self, col: &str, val: Option<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_gt(col, v);
        }
        self
    }

    pub fn and_gte_opt<T>(&mut self, col: &str, val: Option<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_gte(col, v);
        }
        self
    }

    pub fn and_lt_opt<T>(&mut self, col: &str, val: Option<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_lt(col, v);
        }
        self
    }

    pub fn and_lte_opt<T>(&mut self, col: &str, val: Option<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_lte(col, v);
        }
        self
    }

    pub fn and_in_opt<T>(&mut self, col: &str, values: Option<Vec<T>>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = values {
            self.and_in(col, v);
        }
        self
    }

    pub fn and_multi_ilike_opt<T>(&mut self, columns: &[&str], pattern: Option<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + Clone + 'static,
    {
        if let Some(p) = pattern {
            self.and_multi_ilike(columns, p);
        }
        self
    }

    // ==================== Ordering & pagination ====================

    pub fn order_by(&mut self, clause: &str) -> &mut Self {
        self.order_clauses.push(clause.to_string());
        self
    }

    pub fn group_by(&mut self, clause: &str) -> &mut Self {
        self.group_by = Some(clause.to_string());
        self
    }

    /// Add HAVING condition with a single param using `?` placeholder.
    pub fn having<T>(&mut self, sql_template: &str, value: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        let placeholder_count = sql_template.matches('?').count();
        if placeholder_count != 1 {
            self.build_error = Some(format!(
                "QueryBuilder having mismatch: template '{}' has {} '?', but 1 value provided",
                sql_template, placeholder_count
            ));
            return self;
        }

        self.param_count += 1;
        let placeholder = format!("${}", self.param_count);
        let condition = sql_template.replacen('?', &placeholder, 1);
        self.having_conditions.push(condition);
        self.params.push(Box::new(value));
        self
    }

    pub fn limit(&mut self, limit: i64) -> &mut Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(&mut self, offset: i64) -> &mut Self {
        self.offset = Some(offset);
        self
    }

    /// Pagination helper.
    ///
    /// `page` is 1-based (clamped to >= 1).
    /// `per_page` is clamped to >= 1.
    pub fn paginate(&mut self, page: i64, per_page: i64) -> &mut Self {
        let p = if page < 1 { 1 } else { page };
        let size = if per_page < 1 { 1 } else { per_page };
        self.limit = Some(size);
        self.offset = Some((p - 1) * size);
        self
    }

    // ==================== SQL build ====================

    fn build_sql_internal(&self, is_count: bool) -> String {
        let mut sql = if is_count {
            format!("SELECT COUNT(*) FROM {}", self.table)
        } else {
            format!("SELECT {} FROM {}", self.select_cols.join(", "), self.table)
        };

        for join in &self.join_clauses {
            sql.push(' ');
            sql.push_str(join);
        }

        if !self.where_conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.where_conditions.join(" AND "));
        }

        if let Some(ref group) = self.group_by {
            sql.push_str(" GROUP BY ");
            sql.push_str(group);
        }

        if !self.having_conditions.is_empty() {
            sql.push_str(" HAVING ");
            sql.push_str(&self.having_conditions.join(" AND "));
        }

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

        sql
    }

    /// Build COUNT SQL explicitly.
    pub fn to_count_sql(&self) -> String {
        if self.group_by.is_some() || !self.having_conditions.is_empty() {
            let mut sql = format!("SELECT 1 FROM {}", self.table);

            for join in &self.join_clauses {
                sql.push(' ');
                sql.push_str(join);
            }

            if !self.where_conditions.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&self.where_conditions.join(" AND "));
            }

            if let Some(ref group) = self.group_by {
                sql.push_str(" GROUP BY ");
                sql.push_str(group);
            }

            if !self.having_conditions.is_empty() {
                sql.push_str(" HAVING ");
                sql.push_str(&self.having_conditions.join(" AND "));
            }

            format!("SELECT COUNT(*) FROM ({}) AS t", sql)
        } else {
            self.build_sql_internal(true)
        }
    }

    /// Build query object for manual execution.
    pub fn build(&self) -> BuiltQuery<'_> {
        BuiltQuery {
            sql: self.build_sql_internal(false),
            params: &self.params,
        }
    }

    /// Build COUNT query object.
    pub fn build_count(&self) -> BuiltQuery<'_> {
        BuiltQuery {
            sql: self.to_count_sql(),
            params: &self.params,
        }
    }

    /// Execute COUNT query.
    pub async fn count(&self, conn: &impl GenericClient) -> OrmResult<i64> {
        self.validate()?;
        let sql = self.to_count_sql();
        let params = self.params_ref();
        let row = conn.query_one(&sql, &params).await?;
        Ok(row.get(0))
    }
}

impl SqlBuilder for QueryBuilder {
    fn build_sql(&self) -> String {
        self.build_sql_internal(false)
    }

    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params
            .iter()
            .map(|v| &**v as &(dyn ToSql + Sync))
            .collect()
    }

    fn validate(&self) -> OrmResult<()> {
        if let Some(err) = &self.build_error {
            return Err(OrmError::Validation(err.clone()));
        }
        Ok(())
    }
}

/// Built query holding SQL and param references.
pub struct BuiltQuery<'a> {
    sql: String,
    params: &'a [Box<dyn ToSql + Sync + Send>],
}

impl BuiltQuery<'_> {
    pub fn sql(&self) -> &str {
        &self.sql
    }

    pub fn params(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params
            .iter()
            .map(|b| &**b as &(dyn ToSql + Sync))
            .collect()
    }
}
