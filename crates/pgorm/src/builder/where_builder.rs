//! Shared WHERE clause builder for SELECT, UPDATE, DELETE.

use tokio_postgres::types::ToSql;

/// Reusable WHERE clause builder.
///
/// This struct manages WHERE conditions and their parameters, providing
/// a consistent API across QueryBuilder, UpdateBuilder, and DeleteBuilder.
pub struct WhereBuilder {
    /// WHERE conditions (without leading AND)
    conditions: Vec<String>,
    /// Parameter values
    params: Vec<Box<dyn ToSql + Sync + Send>>,
    /// Current parameter counter (starts from offset)
    param_count: usize,
    /// Build error (validated at runtime)
    build_error: Option<String>,
}

impl WhereBuilder {
    /// Create a new WhereBuilder with param numbering starting at 1.
    pub fn new() -> Self {
        Self::with_offset(0)
    }

    /// Create a new WhereBuilder with param numbering starting after `offset`.
    ///
    /// For example, `with_offset(2)` means the first param will be `$3`.
    pub fn with_offset(offset: usize) -> Self {
        Self {
            conditions: Vec::new(),
            params: Vec::new(),
            param_count: offset,
            build_error: None,
        }
    }

    /// Set the parameter offset. Useful for UpdateBuilder where SET params come first.
    pub fn set_offset(&mut self, offset: usize) {
        self.param_count = offset;
    }

    /// Get current parameter count.
    pub fn param_count(&self) -> usize {
        self.param_count
    }

    /// Check if any conditions have been added.
    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
    }

    /// Get the build error, if any.
    pub fn build_error(&self) -> Option<&str> {
        self.build_error.as_deref()
    }

    /// Build the WHERE clause string (without "WHERE" prefix).
    pub fn build_clause(&self) -> String {
        self.conditions.join(" AND ")
    }

    /// Get parameter references for tokio-postgres.
    pub fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params
            .iter()
            .map(|v| &**v as &(dyn ToSql + Sync))
            .collect()
    }

    // ==================== Basic Conditions ====================

    fn add_condition<T>(&mut self, sql_template: &str, value: T)
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.param_count += 1;
        let placeholder = format!("${}", self.param_count);
        let condition = sql_template.replacen('$', &placeholder, 1);
        self.conditions.push(condition);
        self.params.push(Box::new(value));
    }

    /// Add AND equality condition.
    pub fn and_eq<T>(&mut self, col: &str, val: T)
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} = $", col), val);
    }

    /// Add AND not-equal condition.
    pub fn and_ne<T>(&mut self, col: &str, val: T)
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} != $", col), val);
    }

    /// Add AND LIKE condition.
    pub fn and_like<T>(&mut self, col: &str, val: T)
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} LIKE $", col), val);
    }

    /// Add AND ILIKE condition.
    pub fn and_ilike<T>(&mut self, col: &str, val: T)
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} ILIKE $", col), val);
    }

    /// Add AND > condition.
    pub fn and_gt<T>(&mut self, col: &str, val: T)
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} > $", col), val);
    }

    /// Add AND >= condition.
    pub fn and_gte<T>(&mut self, col: &str, val: T)
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} >= $", col), val);
    }

    /// Add AND < condition.
    pub fn and_lt<T>(&mut self, col: &str, val: T)
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} < $", col), val);
    }

    /// Add AND <= condition.
    pub fn and_lte<T>(&mut self, col: &str, val: T)
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.add_condition(&format!("{} <= $", col), val);
    }

    /// Add AND IS NULL condition.
    pub fn and_is_null(&mut self, col: &str) {
        self.conditions.push(format!("{} IS NULL", col));
    }

    /// Add AND IS NOT NULL condition.
    pub fn and_is_not_null(&mut self, col: &str) {
        self.conditions.push(format!("{} IS NOT NULL", col));
    }

    /// Add AND IN (...) condition.
    pub fn and_in<T>(&mut self, col: &str, values: Vec<T>)
    where
        T: ToSql + Sync + Send + 'static,
    {
        if values.is_empty() {
            self.conditions.push("1=0".to_string());
            return;
        }

        let mut placeholders = Vec::new();
        for value in values {
            self.param_count += 1;
            placeholders.push(format!("${}", self.param_count));
            self.params.push(Box::new(value));
        }

        self.conditions
            .push(format!("{} IN ({})", col, placeholders.join(", ")));
    }

    /// Add AND NOT IN (...) condition.
    pub fn and_not_in<T>(&mut self, col: &str, values: Vec<T>)
    where
        T: ToSql + Sync + Send + 'static,
    {
        if values.is_empty() {
            return;
        }

        let mut placeholders = Vec::new();
        for value in values {
            self.param_count += 1;
            placeholders.push(format!("${}", self.param_count));
            self.params.push(Box::new(value));
        }

        self.conditions
            .push(format!("{} NOT IN ({})", col, placeholders.join(", ")));
    }

    /// Add AND BETWEEN condition.
    pub fn and_between<T>(&mut self, col: &str, from: T, to: T)
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.param_count += 1;
        let p1 = format!("${}", self.param_count);
        self.params.push(Box::new(from));

        self.param_count += 1;
        let p2 = format!("${}", self.param_count);
        self.params.push(Box::new(to));

        self.conditions
            .push(format!("{} BETWEEN {} AND {}", col, p1, p2));
    }

    /// Add a raw WHERE condition without params.
    ///
    /// # Safety
    ///
    /// This directly concatenates SQL. The caller must ensure safety.
    pub fn and_raw(&mut self, sql: &str) {
        self.conditions.push(sql.to_string());
    }

    /// Add a complex condition with multiple params using `?` placeholders.
    pub fn and_where<T>(&mut self, sql_template: &str, values: Vec<T>)
    where
        T: ToSql + Sync + Send + 'static,
    {
        let placeholder_count = sql_template.matches('?').count();
        if placeholder_count != values.len() {
            self.build_error = Some(format!(
                "WhereBuilder param mismatch: template '{}' has {} '?', but {} values provided",
                sql_template,
                placeholder_count,
                values.len()
            ));
            return;
        }

        let mut final_sql = sql_template.to_string();
        for v in values {
            self.param_count += 1;
            let placeholder = format!("${}", self.param_count);
            final_sql = final_sql.replacen('?', &placeholder, 1);
            self.params.push(Box::new(v));
        }
        self.conditions.push(format!("({})", final_sql));
    }

    /// Add multi-column ILIKE search (OR).
    pub fn and_multi_ilike<T>(&mut self, columns: &[&str], pattern: T)
    where
        T: ToSql + Sync + Send + Clone + 'static,
    {
        if columns.is_empty() {
            return;
        }

        let mut conditions = Vec::new();
        for col in columns {
            self.param_count += 1;
            conditions.push(format!("{} ILIKE ${}", col, self.param_count));
            self.params.push(Box::new(pattern.clone()));
        }

        self.conditions
            .push(format!("({})", conditions.join(" OR ")));
    }

    // ==================== Option-friendly helpers ====================

    pub fn and_eq_opt<T>(&mut self, col: &str, val: Option<T>)
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_eq(col, v);
        }
    }

    pub fn and_like_opt<T>(&mut self, col: &str, val: Option<T>)
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_like(col, v);
        }
    }

    pub fn and_ilike_opt<T>(&mut self, col: &str, val: Option<T>)
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_ilike(col, v);
        }
    }

    pub fn and_gt_opt<T>(&mut self, col: &str, val: Option<T>)
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_gt(col, v);
        }
    }

    pub fn and_gte_opt<T>(&mut self, col: &str, val: Option<T>)
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_gte(col, v);
        }
    }

    pub fn and_lt_opt<T>(&mut self, col: &str, val: Option<T>)
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_lt(col, v);
        }
    }

    pub fn and_lte_opt<T>(&mut self, col: &str, val: Option<T>)
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = val {
            self.and_lte(col, v);
        }
    }

    pub fn and_in_opt<T>(&mut self, col: &str, values: Option<Vec<T>>)
    where
        T: ToSql + Sync + Send + 'static,
    {
        if let Some(v) = values {
            self.and_in(col, v);
        }
    }

    pub fn and_multi_ilike_opt<T>(&mut self, columns: &[&str], pattern: Option<T>)
    where
        T: ToSql + Sync + Send + Clone + 'static,
    {
        if let Some(p) = pattern {
            self.and_multi_ilike(columns, p);
        }
    }
}

impl Default for WhereBuilder {
    fn default() -> Self {
        Self::new()
    }
}
