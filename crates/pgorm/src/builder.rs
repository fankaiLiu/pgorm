//! Query builder types for dynamic WHERE, ORDER BY, and pagination.
//!
//! This module provides structured builders for constructing SQL clauses safely:
//! - [`WhereExpr`]: Boolean expression tree for WHERE clauses (AND/OR/NOT/grouping)
//! - [`OrderBy`]: Structured ORDER BY builder with nulls handling
//! - [`Pagination`]: LIMIT/OFFSET builder
//! - [`Ident`]: Safe SQL identifier handling

use crate::condition::Condition;
use crate::error::{OrmError, OrmResult};
use crate::sql::Sql;

// ==================== Ident: Safe SQL identifier ====================

/// A part of a SQL identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentPart {
    /// Unquoted identifier: must match `[A-Za-z_][A-Za-z0-9_$]*`
    Unquoted(String),
    /// Quoted identifier: allows any characters except NUL, output with double quotes
    Quoted(String),
}

/// A SQL identifier (column, table, or schema name).
///
/// Supports dotted notation (e.g., `schema.table.column`) and quoted identifiers
/// (e.g., `"CamelCase"."User"`).
///
/// # Example
/// ```ignore
/// use pgorm::builder::Ident;
///
/// let ident = Ident::parse("public.users")?;
/// let quoted = Ident::parse(r#""CamelCase"."UserTable""#)?;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub parts: Vec<IdentPart>,
}

impl Ident {
    /// Create a simple unquoted identifier.
    pub fn new(name: &str) -> OrmResult<Self> {
        Self::parse(name)
    }

    /// Create a quoted identifier.
    pub fn quoted(name: &str) -> OrmResult<Self> {
        if name.contains('\0') {
            return Err(OrmError::validation("Identifier cannot contain NUL character"));
        }
        Ok(Self {
            parts: vec![IdentPart::Quoted(name.to_string())],
        })
    }

    /// Parse an identifier string, supporting dotted and quoted forms.
    ///
    /// - Dotted: `schema.table.column`
    /// - Quoted: `"CamelCase"."UserTable"`
    /// - Mixed: `public."UserTable".id`
    pub fn parse(s: &str) -> OrmResult<Self> {
        if s.is_empty() {
            return Err(OrmError::validation("Identifier cannot be empty"));
        }

        let mut parts = Vec::new();
        let mut chars = s.chars().peekable();

        while chars.peek().is_some() {
            // Skip leading dot (except at start)
            if !parts.is_empty() {
                match chars.next() {
                    Some('.') => {}
                    Some(c) => {
                        return Err(OrmError::validation(format!(
                            "Expected '.' between identifier parts, got '{}'",
                            c
                        )));
                    }
                    None => break,
                }
            }

            // Check for quoted identifier
            if chars.peek() == Some(&'"') {
                chars.next(); // consume opening quote
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('"') => {
                            // Check for escaped quote ""
                            if chars.peek() == Some(&'"') {
                                chars.next();
                                name.push('"');
                            } else {
                                break;
                            }
                        }
                        Some('\0') => {
                            return Err(OrmError::validation(
                                "Identifier cannot contain NUL character",
                            ));
                        }
                        Some(c) => name.push(c),
                        None => {
                            return Err(OrmError::validation("Unclosed quoted identifier"));
                        }
                    }
                }
                if name.is_empty() {
                    return Err(OrmError::validation("Empty quoted identifier"));
                }
                parts.push(IdentPart::Quoted(name));
            } else {
                // Unquoted identifier
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '.' {
                        break;
                    }
                    if name.is_empty() {
                        // First character must be letter or underscore
                        if c == '_' || c.is_ascii_alphabetic() {
                            name.push(c);
                            chars.next();
                        } else {
                            return Err(OrmError::validation(format!(
                                "Invalid identifier start character: '{}'",
                                c
                            )));
                        }
                    } else {
                        // Subsequent characters: letter, digit, underscore, or $
                        if c == '_' || c == '$' || c.is_ascii_alphanumeric() {
                            name.push(c);
                            chars.next();
                        } else {
                            return Err(OrmError::validation(format!(
                                "Invalid character in identifier: '{}'",
                                c
                            )));
                        }
                    }
                }
                if name.is_empty() {
                    return Err(OrmError::validation("Empty identifier segment"));
                }
                parts.push(IdentPart::Unquoted(name));
            }
        }

        if parts.is_empty() {
            return Err(OrmError::validation("Empty identifier"));
        }

        Ok(Self { parts })
    }

    /// Render the identifier as SQL.
    pub fn to_sql(&self) -> String {
        self.parts
            .iter()
            .map(|p| match p {
                IdentPart::Unquoted(s) => s.clone(),
                IdentPart::Quoted(s) => format!("\"{}\"", s.replace('"', "\"\"")),
            })
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Append this identifier to a SQL builder.
    pub fn append_to_sql(&self, sql: &mut Sql) {
        sql.push(&self.to_sql());
    }
}

// ==================== WhereExpr: Boolean expression tree ====================

/// A WHERE clause expression tree supporting AND/OR/NOT/grouping.
///
/// # Example
/// ```ignore
/// use pgorm::builder::WhereExpr;
/// use pgorm::Condition;
///
/// let expr = WhereExpr::And(vec![
///     WhereExpr::Atom(Condition::eq("status", "active")),
///     WhereExpr::Or(vec![
///         WhereExpr::Atom(Condition::eq("role", "admin")),
///         WhereExpr::Atom(Condition::eq("role", "owner")),
///     ]),
/// ]);
/// ```
#[derive(Debug, Clone)]
pub enum WhereExpr {
    /// A single atomic condition.
    Atom(Condition),
    /// Conjunction of expressions (AND).
    And(Vec<WhereExpr>),
    /// Disjunction of expressions (OR).
    Or(Vec<WhereExpr>),
    /// Negation of an expression (NOT).
    Not(Box<WhereExpr>),
    /// Raw SQL expression (escape hatch - use with caution).
    ///
    /// **Warning**: This bypasses SQL injection protection. Only use with
    /// trusted, hardcoded SQL strings.
    Raw(String),
}

impl WhereExpr {
    /// Create an atomic condition expression.
    pub fn atom(condition: Condition) -> Self {
        WhereExpr::Atom(condition)
    }

    /// Create an AND expression from multiple sub-expressions.
    pub fn and(exprs: Vec<WhereExpr>) -> Self {
        WhereExpr::And(exprs)
    }

    /// Create an OR expression from multiple sub-expressions.
    pub fn or(exprs: Vec<WhereExpr>) -> Self {
        WhereExpr::Or(exprs)
    }

    /// Create a NOT expression.
    pub fn not(expr: WhereExpr) -> Self {
        WhereExpr::Not(Box::new(expr))
    }

    /// Create a raw SQL expression.
    ///
    /// **Warning**: This bypasses SQL injection protection. Only use with
    /// trusted, hardcoded SQL strings.
    pub fn raw(sql: impl Into<String>) -> Self {
        WhereExpr::Raw(sql.into())
    }

    /// Combine this expression with another using AND.
    pub fn and_with(self, other: WhereExpr) -> WhereExpr {
        match self {
            WhereExpr::And(mut exprs) => {
                exprs.push(other);
                WhereExpr::And(exprs)
            }
            _ => WhereExpr::And(vec![self, other]),
        }
    }

    /// Combine this expression with another using OR.
    pub fn or_with(self, other: WhereExpr) -> WhereExpr {
        match self {
            WhereExpr::Or(mut exprs) => {
                exprs.push(other);
                WhereExpr::Or(exprs)
            }
            _ => WhereExpr::Or(vec![self, other]),
        }
    }

    /// Check if this expression is empty (evaluates to TRUE).
    pub fn is_empty(&self) -> bool {
        match self {
            WhereExpr::And(exprs) => exprs.is_empty(),
            WhereExpr::Or(exprs) => exprs.is_empty(),
            _ => false,
        }
    }

    /// Append this expression to a SQL builder.
    ///
    /// Parentheses are added around compound expressions to ensure correct precedence.
    pub fn append_to_sql(&self, sql: &mut Sql) {
        match self {
            WhereExpr::Atom(cond) => {
                cond.append_to_sql(sql);
            }
            WhereExpr::And(exprs) => {
                if exprs.is_empty() {
                    // Empty AND is TRUE
                    sql.push("TRUE");
                } else if exprs.len() == 1 {
                    exprs[0].append_to_sql(sql);
                } else {
                    sql.push("(");
                    for (i, expr) in exprs.iter().enumerate() {
                        if i > 0 {
                            sql.push(" AND ");
                        }
                        expr.append_to_sql(sql);
                    }
                    sql.push(")");
                }
            }
            WhereExpr::Or(exprs) => {
                if exprs.is_empty() {
                    // Empty OR is FALSE
                    sql.push("FALSE");
                } else if exprs.len() == 1 {
                    exprs[0].append_to_sql(sql);
                } else {
                    sql.push("(");
                    for (i, expr) in exprs.iter().enumerate() {
                        if i > 0 {
                            sql.push(" OR ");
                        }
                        expr.append_to_sql(sql);
                    }
                    sql.push(")");
                }
            }
            WhereExpr::Not(expr) => {
                sql.push("(NOT ");
                expr.append_to_sql(sql);
                sql.push(")");
            }
            WhereExpr::Raw(s) => {
                sql.push(s);
            }
        }
    }
}

impl From<Condition> for WhereExpr {
    fn from(cond: Condition) -> Self {
        WhereExpr::Atom(cond)
    }
}

// ==================== OrderBy: Structured ORDER BY builder ====================

/// Sort direction for ORDER BY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

impl SortDir {
    fn to_sql(&self) -> &'static str {
        match self {
            SortDir::Asc => "ASC",
            SortDir::Desc => "DESC",
        }
    }
}

/// NULLS ordering for ORDER BY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NullsOrder {
    First,
    Last,
}

impl NullsOrder {
    fn to_sql(&self) -> &'static str {
        match self {
            NullsOrder::First => "NULLS FIRST",
            NullsOrder::Last => "NULLS LAST",
        }
    }
}

/// A single ORDER BY item.
#[derive(Debug, Clone)]
pub struct OrderItem {
    /// The column to sort by.
    pub column: String,
    /// Sort direction (ASC/DESC).
    pub dir: SortDir,
    /// Optional NULLS ordering.
    pub nulls: Option<NullsOrder>,
}

impl OrderItem {
    /// Create a new order item.
    pub fn new(column: impl Into<String>, dir: SortDir) -> Self {
        Self {
            column: column.into(),
            dir,
            nulls: None,
        }
    }

    /// Set NULLS ordering.
    pub fn nulls(mut self, order: NullsOrder) -> Self {
        self.nulls = Some(order);
        self
    }

    fn to_sql(&self) -> OrmResult<String> {
        // Validate the column name using Ident
        let ident = Ident::parse(&self.column)?;
        let mut s = ident.to_sql();
        s.push(' ');
        s.push_str(self.dir.to_sql());
        if let Some(nulls) = &self.nulls {
            s.push(' ');
            s.push_str(nulls.to_sql());
        }
        Ok(s)
    }
}

/// ORDER BY clause builder.
///
/// # Example
/// ```ignore
/// use pgorm::builder::{OrderBy, SortDir, NullsOrder};
///
/// let order = OrderBy::new()
///     .asc("created_at")
///     .desc("priority")
///     .with_nulls("last_login", SortDir::Desc, NullsOrder::Last);
/// ```
#[derive(Debug, Clone, Default)]
pub struct OrderBy {
    items: Vec<OrderItem>,
}

impl OrderBy {
    /// Create a new empty OrderBy builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an ascending sort.
    pub fn asc(mut self, column: impl Into<String>) -> Self {
        self.items.push(OrderItem::new(column, SortDir::Asc));
        self
    }

    /// Add a descending sort.
    pub fn desc(mut self, column: impl Into<String>) -> Self {
        self.items.push(OrderItem::new(column, SortDir::Desc));
        self
    }

    /// Add a sort with custom direction and nulls ordering.
    pub fn with_nulls(
        mut self,
        column: impl Into<String>,
        dir: SortDir,
        nulls: NullsOrder,
    ) -> Self {
        self.items.push(OrderItem::new(column, dir).nulls(nulls));
        self
    }

    /// Add a custom order item.
    pub fn add(mut self, item: OrderItem) -> Self {
        self.items.push(item);
        self
    }

    /// Check if this OrderBy is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Append this ORDER BY clause to a SQL builder.
    ///
    /// Does nothing if the OrderBy is empty.
    pub fn append_to_sql(&self, sql: &mut Sql) -> OrmResult<()> {
        if self.items.is_empty() {
            return Ok(());
        }
        sql.push(" ORDER BY ");
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                sql.push(", ");
            }
            sql.push(&item.to_sql()?);
        }
        Ok(())
    }

    /// Build the ORDER BY clause as a string.
    pub fn to_sql(&self) -> OrmResult<String> {
        if self.items.is_empty() {
            return Ok(String::new());
        }
        let parts: Result<Vec<_>, _> = self.items.iter().map(|i| i.to_sql()).collect();
        Ok(format!("ORDER BY {}", parts?.join(", ")))
    }
}

// ==================== Pagination: LIMIT/OFFSET builder ====================

/// Pagination configuration for LIMIT/OFFSET.
///
/// # Example
/// ```ignore
/// use pgorm::builder::Pagination;
///
/// // Direct limit/offset
/// let pag = Pagination::new().limit(10).offset(20);
///
/// // Page-based (page 3 with 25 items per page)
/// let pag = Pagination::page(3, 25)?;
/// ```
#[derive(Debug, Clone, Default)]
pub struct Pagination {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Pagination {
    /// Create a new empty pagination (no limit/offset).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create pagination from page number and page size.
    ///
    /// Page numbers start at 1. Returns error if page < 1.
    pub fn page(page: i64, per_page: i64) -> OrmResult<Self> {
        if page < 1 {
            return Err(OrmError::validation(format!(
                "page must be >= 1, got {}",
                page
            )));
        }
        Ok(Self {
            limit: Some(per_page),
            offset: Some((page - 1) * per_page),
        })
    }

    /// Set the limit.
    pub fn limit(mut self, n: i64) -> Self {
        self.limit = Some(n);
        self
    }

    /// Set the offset.
    pub fn offset(mut self, n: i64) -> Self {
        self.offset = Some(n);
        self
    }

    /// Check if pagination is set.
    pub fn is_empty(&self) -> bool {
        self.limit.is_none() && self.offset.is_none()
    }

    /// Append LIMIT/OFFSET to a SQL builder with bound parameters.
    pub fn append_to_sql(&self, sql: &mut Sql) {
        if let Some(limit) = self.limit {
            sql.push(" LIMIT ");
            sql.push_bind(limit);
        }
        if let Some(offset) = self.offset {
            sql.push(" OFFSET ");
            sql.push_bind(offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Ident tests ====================

    #[test]
    fn ident_simple() {
        let ident = Ident::parse("users").unwrap();
        assert_eq!(ident.to_sql(), "users");
    }

    #[test]
    fn ident_dotted() {
        let ident = Ident::parse("public.users").unwrap();
        assert_eq!(ident.to_sql(), "public.users");
    }

    #[test]
    fn ident_three_parts() {
        let ident = Ident::parse("schema.table.column").unwrap();
        assert_eq!(ident.to_sql(), "schema.table.column");
    }

    #[test]
    fn ident_quoted() {
        let ident = Ident::parse(r#""CamelCase""#).unwrap();
        assert_eq!(ident.to_sql(), r#""CamelCase""#);
    }

    #[test]
    fn ident_quoted_with_escape() {
        let ident = Ident::parse(r#""has""quote""#).unwrap();
        assert_eq!(ident.to_sql(), r#""has""quote""#);
    }

    #[test]
    fn ident_mixed_quoted_unquoted() {
        let ident = Ident::parse(r#"public."UserTable".id"#).unwrap();
        assert_eq!(ident.to_sql(), r#"public."UserTable".id"#);
    }

    #[test]
    fn ident_with_dollar() {
        let ident = Ident::parse("my_var$1").unwrap();
        assert_eq!(ident.to_sql(), "my_var$1");
    }

    #[test]
    fn ident_rejects_empty() {
        assert!(Ident::parse("").is_err());
    }

    #[test]
    fn ident_rejects_start_digit() {
        assert!(Ident::parse("1table").is_err());
    }

    #[test]
    fn ident_rejects_space() {
        assert!(Ident::parse("my table").is_err());
    }

    #[test]
    fn ident_rejects_double_dot() {
        assert!(Ident::parse("schema..table").is_err());
    }

    #[test]
    fn ident_rejects_unclosed_quote() {
        assert!(Ident::parse(r#""unclosed"#).is_err());
    }

    // ==================== WhereExpr tests ====================

    #[test]
    fn where_atom() {
        let expr = WhereExpr::atom(Condition::eq("status", "active"));
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "status = $1");
    }

    #[test]
    fn where_and() {
        let expr = WhereExpr::And(vec![
            WhereExpr::Atom(Condition::eq("a", 1_i32)),
            WhereExpr::Atom(Condition::eq("b", 2_i32)),
        ]);
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "(a = $1 AND b = $2)");
    }

    #[test]
    fn where_or() {
        let expr = WhereExpr::Or(vec![
            WhereExpr::Atom(Condition::eq("role", "admin")),
            WhereExpr::Atom(Condition::eq("role", "owner")),
        ]);
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "(role = $1 OR role = $2)");
    }

    #[test]
    fn where_not() {
        let expr = WhereExpr::Not(Box::new(WhereExpr::Atom(Condition::eq("deleted", true))));
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "(NOT deleted = $1)");
    }

    #[test]
    fn where_nested() {
        let expr = WhereExpr::And(vec![
            WhereExpr::Atom(Condition::eq("status", "active")),
            WhereExpr::Or(vec![
                WhereExpr::Atom(Condition::eq("role", "admin")),
                WhereExpr::Atom(Condition::eq("role", "owner")),
            ]),
        ]);
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(
            sql.to_sql(),
            "(status = $1 AND (role = $2 OR role = $3))"
        );
    }

    #[test]
    fn where_empty_and_is_true() {
        let expr = WhereExpr::And(vec![]);
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "TRUE");
    }

    #[test]
    fn where_empty_or_is_false() {
        let expr = WhereExpr::Or(vec![]);
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "FALSE");
    }

    #[test]
    fn where_and_with_combines() {
        let a = WhereExpr::atom(Condition::eq("a", 1_i32));
        let b = WhereExpr::atom(Condition::eq("b", 2_i32));
        let expr = a.and_with(b);
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "(a = $1 AND b = $2)");
    }

    #[test]
    fn where_raw() {
        let expr = WhereExpr::raw("custom_func(x) > 0");
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "custom_func(x) > 0");
    }

    // ==================== OrderBy tests ====================

    #[test]
    fn order_by_single_asc() {
        let order = OrderBy::new().asc("created_at");
        assert_eq!(order.to_sql().unwrap(), "ORDER BY created_at ASC");
    }

    #[test]
    fn order_by_single_desc() {
        let order = OrderBy::new().desc("priority");
        assert_eq!(order.to_sql().unwrap(), "ORDER BY priority DESC");
    }

    #[test]
    fn order_by_multiple() {
        let order = OrderBy::new().asc("status").desc("created_at");
        assert_eq!(
            order.to_sql().unwrap(),
            "ORDER BY status ASC, created_at DESC"
        );
    }

    #[test]
    fn order_by_with_nulls() {
        let order = OrderBy::new().with_nulls("last_login", SortDir::Desc, NullsOrder::Last);
        assert_eq!(
            order.to_sql().unwrap(),
            "ORDER BY last_login DESC NULLS LAST"
        );
    }

    #[test]
    fn order_by_empty() {
        let order = OrderBy::new();
        assert!(order.is_empty());
        assert_eq!(order.to_sql().unwrap(), "");
    }

    #[test]
    fn order_by_append() {
        let order = OrderBy::new().asc("id");
        let mut sql = Sql::new("SELECT * FROM users");
        order.append_to_sql(&mut sql).unwrap();
        assert_eq!(sql.to_sql(), "SELECT * FROM users ORDER BY id ASC");
    }

    #[test]
    fn order_by_validates_column() {
        let order = OrderBy::new().asc("valid_column; DROP TABLE users;");
        assert!(order.to_sql().is_err());
    }

    // ==================== Pagination tests ====================

    #[test]
    fn pagination_limit_only() {
        let pag = Pagination::new().limit(10);
        let mut sql = Sql::new("SELECT * FROM users");
        pag.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "SELECT * FROM users LIMIT $1");
    }

    #[test]
    fn pagination_offset_only() {
        let pag = Pagination::new().offset(20);
        let mut sql = Sql::new("SELECT * FROM users");
        pag.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "SELECT * FROM users OFFSET $1");
    }

    #[test]
    fn pagination_limit_offset() {
        let pag = Pagination::new().limit(10).offset(20);
        let mut sql = Sql::new("SELECT * FROM users");
        pag.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "SELECT * FROM users LIMIT $1 OFFSET $2");
    }

    #[test]
    fn pagination_page() {
        let pag = Pagination::page(3, 25).unwrap();
        assert_eq!(pag.limit, Some(25));
        assert_eq!(pag.offset, Some(50)); // (3-1) * 25 = 50
    }

    #[test]
    fn pagination_page_one() {
        let pag = Pagination::page(1, 10).unwrap();
        assert_eq!(pag.limit, Some(10));
        assert_eq!(pag.offset, Some(0));
    }

    #[test]
    fn pagination_page_rejects_zero() {
        assert!(Pagination::page(0, 10).is_err());
    }

    #[test]
    fn pagination_page_rejects_negative() {
        assert!(Pagination::page(-1, 10).is_err());
    }

    #[test]
    fn pagination_empty() {
        let pag = Pagination::new();
        assert!(pag.is_empty());
        let mut sql = Sql::new("SELECT * FROM users");
        pag.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "SELECT * FROM users");
    }
}
