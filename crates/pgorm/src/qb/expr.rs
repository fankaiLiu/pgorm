//! Unified expression layer for WHERE/HAVING/ON CONFLICT conditions.
//!
//! This module provides the `Expr` enum which supports:
//! - AND/OR/NOT grouping
//! - Various comparison operators (eq, ne, gt, lt, like, etc.)
//! - Template expressions with `?` placeholders
//! - Raw SQL fragments
//!
//! The key feature is that `Expr::build()` generates SQL with correct `$n` placeholders
//! without any string replacement - the parameter indices are computed at build time.

use crate::qb::param::{Param, ParamList};
use tokio_postgres::types::ToSql;

/// Expression node for building WHERE/HAVING clauses.
///
/// This enum supports composable boolean expressions with proper placeholder handling.
#[derive(Clone, Debug)]
pub enum Expr {
    /// AND group: all conditions must be true.
    And(Vec<Expr>),

    /// OR group: at least one condition must be true.
    Or(Vec<Expr>),

    /// NOT: negate the inner expression.
    Not(Box<Expr>),

    /// Simple comparison: column op $n
    Compare {
        column: String,
        op: &'static str,
        value: Param,
    },

    /// NULL check: column IS NULL or column IS NOT NULL
    NullCheck {
        column: String,
        is_null: bool,
    },

    /// IN list: column IN ($1, $2, ...) or column NOT IN (...)
    InList {
        column: String,
        values: Vec<Param>,
        negated: bool,
    },

    /// BETWEEN: column BETWEEN $n AND $m
    Between {
        column: String,
        from: Param,
        to: Param,
        negated: bool,
    },

    /// Template with `?` placeholders that get replaced with `$n`.
    /// Example: `Template { sql: "a = ? OR b = ?", params: [1, 2] }` -> `a = $1 OR b = $2`
    Template {
        sql: String,
        params: Vec<Param>,
    },

    /// Raw SQL fragment without parameters.
    Raw(String),

    /// Always true (used for empty NOT IN lists).
    True,

    /// Always false (used for empty IN lists).
    False,
}

impl Expr {
    /// Create an AND expression from a list of expressions.
    pub fn and(exprs: Vec<Expr>) -> Self {
        Expr::And(exprs)
    }

    /// Create an OR expression from a list of expressions.
    pub fn or(exprs: Vec<Expr>) -> Self {
        Expr::Or(exprs)
    }

    /// Create a NOT expression.
    pub fn not(expr: Expr) -> Self {
        Expr::Not(Box::new(expr))
    }

    /// Create an equality condition: column = value
    pub fn eq<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, value: T) -> Self {
        Expr::Compare {
            column: column.into(),
            op: "=",
            value: Param::new(value),
        }
    }

    /// Create an inequality condition: column != value
    pub fn ne<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, value: T) -> Self {
        Expr::Compare {
            column: column.into(),
            op: "!=",
            value: Param::new(value),
        }
    }

    /// Create a greater-than condition: column > value
    pub fn gt<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, value: T) -> Self {
        Expr::Compare {
            column: column.into(),
            op: ">",
            value: Param::new(value),
        }
    }

    /// Create a greater-than-or-equal condition: column >= value
    pub fn gte<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, value: T) -> Self {
        Expr::Compare {
            column: column.into(),
            op: ">=",
            value: Param::new(value),
        }
    }

    /// Create a less-than condition: column < value
    pub fn lt<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, value: T) -> Self {
        Expr::Compare {
            column: column.into(),
            op: "<",
            value: Param::new(value),
        }
    }

    /// Create a less-than-or-equal condition: column <= value
    pub fn lte<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, value: T) -> Self {
        Expr::Compare {
            column: column.into(),
            op: "<=",
            value: Param::new(value),
        }
    }

    /// Create a LIKE condition: column LIKE pattern
    pub fn like<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, pattern: T) -> Self {
        Expr::Compare {
            column: column.into(),
            op: "LIKE",
            value: Param::new(pattern),
        }
    }

    /// Create an ILIKE condition: column ILIKE pattern (case-insensitive)
    pub fn ilike<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, pattern: T) -> Self {
        Expr::Compare {
            column: column.into(),
            op: "ILIKE",
            value: Param::new(pattern),
        }
    }

    /// Create a NOT LIKE condition: column NOT LIKE pattern
    pub fn not_like<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, pattern: T) -> Self {
        Expr::Compare {
            column: column.into(),
            op: "NOT LIKE",
            value: Param::new(pattern),
        }
    }

    /// Create a NOT ILIKE condition: column NOT ILIKE pattern
    pub fn not_ilike<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, pattern: T) -> Self {
        Expr::Compare {
            column: column.into(),
            op: "NOT ILIKE",
            value: Param::new(pattern),
        }
    }

    /// Create an IS NULL condition: column IS NULL
    pub fn is_null(column: impl Into<String>) -> Self {
        Expr::NullCheck {
            column: column.into(),
            is_null: true,
        }
    }

    /// Create an IS NOT NULL condition: column IS NOT NULL
    pub fn is_not_null(column: impl Into<String>) -> Self {
        Expr::NullCheck {
            column: column.into(),
            is_null: false,
        }
    }

    /// Create an IN condition: column IN (values...)
    pub fn in_list<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, values: Vec<T>) -> Self {
        if values.is_empty() {
            return Expr::False;
        }
        Expr::InList {
            column: column.into(),
            values: values.into_iter().map(Param::new).collect(),
            negated: false,
        }
    }

    /// Create a NOT IN condition: column NOT IN (values...)
    pub fn not_in<T: ToSql + Send + Sync + 'static>(column: impl Into<String>, values: Vec<T>) -> Self {
        if values.is_empty() {
            return Expr::True;
        }
        Expr::InList {
            column: column.into(),
            values: values.into_iter().map(Param::new).collect(),
            negated: true,
        }
    }

    /// Create a BETWEEN condition: column BETWEEN from AND to
    pub fn between<T: ToSql + Send + Sync + 'static>(
        column: impl Into<String>,
        from: T,
        to: T,
    ) -> Self {
        Expr::Between {
            column: column.into(),
            from: Param::new(from),
            to: Param::new(to),
            negated: false,
        }
    }

    /// Create a NOT BETWEEN condition: column NOT BETWEEN from AND to
    pub fn not_between<T: ToSql + Send + Sync + 'static>(
        column: impl Into<String>,
        from: T,
        to: T,
    ) -> Self {
        Expr::Between {
            column: column.into(),
            from: Param::new(from),
            to: Param::new(to),
            negated: true,
        }
    }

    /// Create a template expression with `?` placeholders.
    ///
    /// # Example
    /// ```ignore
    /// Expr::template("a = ? OR b = ?", vec![Param::new(1), Param::new(2)])
    /// ```
    pub fn template(sql: impl Into<String>, params: Vec<Param>) -> Self {
        Expr::Template {
            sql: sql.into(),
            params,
        }
    }

    /// Create a template expression from values.
    ///
    /// # Example
    /// ```ignore
    /// Expr::template_values("a = ? OR b = ?", vec![1i32, 2i32])
    /// ```
    pub fn template_values<T: ToSql + Send + Sync + 'static>(
        sql: impl Into<String>,
        values: Vec<T>,
    ) -> Self {
        Expr::Template {
            sql: sql.into(),
            params: values.into_iter().map(Param::new).collect(),
        }
    }

    /// Create a raw SQL fragment.
    pub fn raw(sql: impl Into<String>) -> Self {
        Expr::Raw(sql.into())
    }

    /// Check if this expression is empty (contains no conditions).
    pub fn is_empty(&self) -> bool {
        match self {
            Expr::And(exprs) | Expr::Or(exprs) => exprs.is_empty() || exprs.iter().all(|e| e.is_empty()),
            Expr::Not(inner) => inner.is_empty(),
            _ => false,
        }
    }

    /// Build the SQL fragment with proper `$n` placeholders.
    ///
    /// Returns the SQL string and collects parameters into the provided ParamList.
    pub fn build(&self, params: &mut ParamList) -> String {
        match self {
            Expr::And(exprs) => {
                if exprs.is_empty() {
                    return String::new();
                }
                let parts: Vec<String> = exprs
                    .iter()
                    .filter(|e| !e.is_empty())
                    .map(|e| {
                        let sql = e.build(params);
                        // Wrap OR groups in parentheses
                        if matches!(e, Expr::Or(_)) && !sql.is_empty() {
                            format!("({})", sql)
                        } else {
                            sql
                        }
                    })
                    .filter(|s| !s.is_empty())
                    .collect();
                parts.join(" AND ")
            }
            Expr::Or(exprs) => {
                if exprs.is_empty() {
                    return String::new();
                }
                let parts: Vec<String> = exprs
                    .iter()
                    .filter(|e| !e.is_empty())
                    .map(|e| {
                        let sql = e.build(params);
                        // Wrap AND groups in parentheses
                        if matches!(e, Expr::And(_)) && !sql.is_empty() {
                            format!("({})", sql)
                        } else {
                            sql
                        }
                    })
                    .filter(|s| !s.is_empty())
                    .collect();
                parts.join(" OR ")
            }
            Expr::Not(inner) => {
                let sql = inner.build(params);
                if sql.is_empty() {
                    String::new()
                } else {
                    format!("NOT ({})", sql)
                }
            }
            Expr::Compare { column, op, value } => {
                let idx = params.push_param(value.clone());
                format!("{} {} ${}", column, op, idx)
            }
            Expr::NullCheck { column, is_null } => {
                if *is_null {
                    format!("{} IS NULL", column)
                } else {
                    format!("{} IS NOT NULL", column)
                }
            }
            Expr::InList { column, values, negated } => {
                if values.is_empty() {
                    // Should not happen as we return True/False in constructors
                    return if *negated { "1=1".to_string() } else { "1=0".to_string() };
                }
                let placeholders: Vec<String> = values
                    .iter()
                    .map(|v| {
                        let idx = params.push_param(v.clone());
                        format!("${}", idx)
                    })
                    .collect();
                let op = if *negated { "NOT IN" } else { "IN" };
                format!("{} {} ({})", column, op, placeholders.join(", "))
            }
            Expr::Between { column, from, to, negated } => {
                let idx1 = params.push_param(from.clone());
                let idx2 = params.push_param(to.clone());
                let op = if *negated { "NOT BETWEEN" } else { "BETWEEN" };
                format!("{} {} ${} AND ${}", column, op, idx1, idx2)
            }
            Expr::Template { sql, params: template_params } => {
                // Replace `?` with `$n`
                let mut result = String::new();
                let mut placeholder_idx = 0;
                for ch in sql.chars() {
                    if ch == '?' {
                        if placeholder_idx < template_params.len() {
                            let param_idx = params.push_param(template_params[placeholder_idx].clone());
                            result.push('$');
                            result.push_str(&param_idx.to_string());
                            placeholder_idx += 1;
                        } else {
                            result.push(ch);
                        }
                    } else {
                        result.push(ch);
                    }
                }
                result
            }
            Expr::Raw(sql) => sql.clone(),
            Expr::True => "1=1".to_string(),
            Expr::False => "1=0".to_string(),
        }
    }
}

/// A builder for constructing WHERE/HAVING clauses incrementally.
///
/// This is the main interface for building conditions in query builders.
#[derive(Clone, Debug, Default)]
pub struct ExprGroup {
    /// The list of expressions to be ANDed together.
    exprs: Vec<Expr>,
}

impl ExprGroup {
    /// Create a new empty expression group.
    pub fn new() -> Self {
        Self { exprs: Vec::new() }
    }

    /// Check if the group is empty.
    pub fn is_empty(&self) -> bool {
        self.exprs.is_empty()
    }

    /// Add an expression to be ANDed.
    pub fn and_expr(&mut self, expr: Expr) {
        self.exprs.push(expr);
    }

    /// Add a condition: column = value
    pub fn eq<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, value: T) {
        self.exprs.push(Expr::eq(column, value));
    }

    /// Add a condition: column != value
    pub fn ne<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, value: T) {
        self.exprs.push(Expr::ne(column, value));
    }

    /// Add a condition: column > value
    pub fn gt<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, value: T) {
        self.exprs.push(Expr::gt(column, value));
    }

    /// Add a condition: column >= value
    pub fn gte<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, value: T) {
        self.exprs.push(Expr::gte(column, value));
    }

    /// Add a condition: column < value
    pub fn lt<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, value: T) {
        self.exprs.push(Expr::lt(column, value));
    }

    /// Add a condition: column <= value
    pub fn lte<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, value: T) {
        self.exprs.push(Expr::lte(column, value));
    }

    /// Add a condition: column LIKE pattern
    pub fn like<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, pattern: T) {
        self.exprs.push(Expr::like(column, pattern));
    }

    /// Add a condition: column ILIKE pattern (case-insensitive)
    pub fn ilike<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, pattern: T) {
        self.exprs.push(Expr::ilike(column, pattern));
    }

    /// Add a condition: column NOT LIKE pattern
    pub fn not_like<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, pattern: T) {
        self.exprs.push(Expr::not_like(column, pattern));
    }

    /// Add a condition: column NOT ILIKE pattern
    pub fn not_ilike<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, pattern: T) {
        self.exprs.push(Expr::not_ilike(column, pattern));
    }

    /// Add a condition: column IS NULL
    pub fn is_null(&mut self, column: &str) {
        self.exprs.push(Expr::is_null(column));
    }

    /// Add a condition: column IS NOT NULL
    pub fn is_not_null(&mut self, column: &str) {
        self.exprs.push(Expr::is_not_null(column));
    }

    /// Add a condition: column IN (values...)
    pub fn in_list<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, values: Vec<T>) {
        self.exprs.push(Expr::in_list(column, values));
    }

    /// Add a condition: column NOT IN (values...)
    pub fn not_in<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, values: Vec<T>) {
        self.exprs.push(Expr::not_in(column, values));
    }

    /// Add a condition: column BETWEEN from AND to
    pub fn between<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, from: T, to: T) {
        self.exprs.push(Expr::between(column, from, to));
    }

    /// Add a condition: column NOT BETWEEN from AND to
    pub fn not_between<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, from: T, to: T) {
        self.exprs.push(Expr::not_between(column, from, to));
    }

    /// Add a raw SQL condition.
    pub fn raw(&mut self, sql: &str) {
        self.exprs.push(Expr::raw(sql));
    }

    /// Add a template condition with `?` placeholders.
    pub fn template<T: ToSql + Send + Sync + 'static>(&mut self, sql: &str, values: Vec<T>) {
        self.exprs.push(Expr::template_values(sql, values));
    }

    /// Add multiple columns with OR ILIKE pattern.
    pub fn multi_ilike<T: ToSql + Send + Sync + Clone + 'static>(
        &mut self,
        columns: &[&str],
        pattern: T,
    ) {
        if columns.is_empty() {
            return;
        }
        let or_exprs: Vec<Expr> = columns
            .iter()
            .map(|col| Expr::ilike(*col, pattern.clone()))
            .collect();
        self.exprs.push(Expr::Or(or_exprs));
    }

    // ========== Optional value methods ==========

    /// Add a condition if value is Some: column = value
    pub fn eq_opt<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, value: Option<T>) {
        if let Some(v) = value {
            self.eq(column, v);
        }
    }

    /// Add a condition if value is Some: column LIKE pattern
    pub fn like_opt<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, pattern: Option<T>) {
        if let Some(v) = pattern {
            self.like(column, v);
        }
    }

    /// Add a condition if value is Some: column ILIKE pattern
    pub fn ilike_opt<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, pattern: Option<T>) {
        if let Some(v) = pattern {
            self.ilike(column, v);
        }
    }

    /// Add a condition if value is Some: column > value
    pub fn gt_opt<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, value: Option<T>) {
        if let Some(v) = value {
            self.gt(column, v);
        }
    }

    /// Add a condition if value is Some: column >= value
    pub fn gte_opt<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, value: Option<T>) {
        if let Some(v) = value {
            self.gte(column, v);
        }
    }

    /// Add a condition if value is Some: column < value
    pub fn lt_opt<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, value: Option<T>) {
        if let Some(v) = value {
            self.lt(column, v);
        }
    }

    /// Add a condition if value is Some: column <= value
    pub fn lte_opt<T: ToSql + Send + Sync + 'static>(&mut self, column: &str, value: Option<T>) {
        if let Some(v) = value {
            self.lte(column, v);
        }
    }

    /// Add a condition if values is Some and non-empty: column IN (values...)
    pub fn in_opt<T: ToSql + Send + Sync + 'static>(
        &mut self,
        column: &str,
        values: Option<Vec<T>>,
    ) {
        if let Some(v) = values {
            if !v.is_empty() {
                self.in_list(column, v);
            }
        }
    }

    /// Add multiple columns with OR ILIKE if pattern is Some.
    pub fn multi_ilike_opt<T: ToSql + Send + Sync + Clone + 'static>(
        &mut self,
        columns: &[&str],
        pattern: Option<T>,
    ) {
        if let Some(p) = pattern {
            self.multi_ilike(columns, p);
        }
    }

    /// Build the WHERE clause content (without the "WHERE" keyword).
    ///
    /// Returns the SQL string and a ParamList containing all parameters.
    pub fn build(&self) -> (String, ParamList) {
        let mut params = ParamList::new();
        if self.exprs.is_empty() {
            return (String::new(), params);
        }

        let root = Expr::And(self.exprs.clone());
        let sql = root.build(&mut params);
        (sql, params)
    }

    /// Build with a parameter offset (for UPDATE SET + WHERE).
    ///
    /// The offset is the number of parameters already used before the WHERE clause.
    pub fn build_with_offset(&self, offset: usize) -> (String, ParamList) {
        let mut params = ParamList::new();
        if self.exprs.is_empty() {
            return (String::new(), params);
        }

        let root = Expr::And(self.exprs.clone());
        let sql = root.build(&mut params);

        // Adjust the $n in the SQL to account for offset
        if offset > 0 && !sql.is_empty() {
            let adjusted = adjust_placeholders(&sql, offset);
            (adjusted, params)
        } else {
            (sql, params)
        }
    }

    /// Get all expressions.
    pub fn exprs(&self) -> &[Expr] {
        &self.exprs
    }

    /// Take all expressions (consuming the group).
    pub fn into_exprs(self) -> Vec<Expr> {
        self.exprs
    }
}

/// Adjust placeholder numbers in SQL by adding an offset.
///
/// For example, with offset=3: `$1 AND $2` becomes `$4 AND $5`
fn adjust_placeholders(sql: &str, offset: usize) -> String {
    let mut result = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            // Read the number
            let mut num_str = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_ascii_digit() {
                    num_str.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            if let Ok(old_idx) = num_str.parse::<usize>() {
                result.push('$');
                result.push_str(&(old_idx + offset).to_string());
            } else {
                result.push('$');
                result.push_str(&num_str);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_eq() {
        let expr = Expr::eq("name", "alice");
        let mut params = ParamList::new();
        let sql = expr.build(&mut params);
        assert_eq!(sql, "name = $1");
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn test_and_group() {
        let expr = Expr::and(vec![
            Expr::eq("status", "active"),
            Expr::gt("age", 18i32),
        ]);
        let mut params = ParamList::new();
        let sql = expr.build(&mut params);
        assert_eq!(sql, "status = $1 AND age > $2");
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_or_group() {
        let expr = Expr::or(vec![
            Expr::eq("role", "admin"),
            Expr::eq("role", "superuser"),
        ]);
        let mut params = ParamList::new();
        let sql = expr.build(&mut params);
        assert_eq!(sql, "role = $1 OR role = $2");
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_nested_and_or() {
        let expr = Expr::and(vec![
            Expr::eq("status", "active"),
            Expr::or(vec![
                Expr::eq("role", "admin"),
                Expr::eq("role", "superuser"),
            ]),
        ]);
        let mut params = ParamList::new();
        let sql = expr.build(&mut params);
        assert_eq!(sql, "status = $1 AND (role = $2 OR role = $3)");
        assert_eq!(params.len(), 3);
    }

    #[test]
    fn test_in_list() {
        let expr = Expr::in_list("id", vec![1i32, 2, 3]);
        let mut params = ParamList::new();
        let sql = expr.build(&mut params);
        assert_eq!(sql, "id IN ($1, $2, $3)");
        assert_eq!(params.len(), 3);
    }

    #[test]
    fn test_empty_in_list() {
        let expr = Expr::in_list::<i32>("id", vec![]);
        let mut params = ParamList::new();
        let sql = expr.build(&mut params);
        assert_eq!(sql, "1=0");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_empty_not_in_list() {
        let expr = Expr::not_in::<i32>("id", vec![]);
        let mut params = ParamList::new();
        let sql = expr.build(&mut params);
        assert_eq!(sql, "1=1");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_between() {
        let expr = Expr::between("age", 18i32, 65i32);
        let mut params = ParamList::new();
        let sql = expr.build(&mut params);
        assert_eq!(sql, "age BETWEEN $1 AND $2");
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_template() {
        let expr = Expr::template_values("a = ? OR b = ?", vec![1i32, 2i32]);
        let mut params = ParamList::new();
        let sql = expr.build(&mut params);
        assert_eq!(sql, "a = $1 OR b = $2");
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_null_check() {
        let expr = Expr::is_null("deleted_at");
        let mut params = ParamList::new();
        let sql = expr.build(&mut params);
        assert_eq!(sql, "deleted_at IS NULL");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_not() {
        let expr = Expr::not(Expr::eq("banned", true));
        let mut params = ParamList::new();
        let sql = expr.build(&mut params);
        assert_eq!(sql, "NOT (banned = $1)");
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn test_expr_group() {
        let mut group = ExprGroup::new();
        group.eq("status", "active");
        group.gt("age", 18i32);
        group.in_list("role", vec!["admin", "user"]);

        let (sql, params) = group.build();
        assert_eq!(sql, "status = $1 AND age > $2 AND role IN ($3, $4)");
        assert_eq!(params.len(), 4);
    }

    #[test]
    fn test_adjust_placeholders() {
        let sql = "$1 AND $2 AND $10";
        let adjusted = adjust_placeholders(sql, 5);
        assert_eq!(adjusted, "$6 AND $7 AND $15");
    }

    #[test]
    fn test_build_with_offset() {
        let mut group = ExprGroup::new();
        group.eq("name", "alice");
        group.gt("age", 18i32);

        let (sql, params) = group.build_with_offset(3);
        assert_eq!(sql, "name = $4 AND age > $5");
        assert_eq!(params.len(), 2);
    }
}
