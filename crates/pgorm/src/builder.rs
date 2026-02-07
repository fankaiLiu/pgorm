//! Query builder types for dynamic WHERE, ORDER BY, and pagination.
//!
//! This module provides structured builders for constructing SQL clauses safely:
//! - [`WhereExpr`]: Boolean expression tree for WHERE clauses (AND/OR/NOT/grouping)
//! - [`OrderBy`]: Structured ORDER BY builder with nulls handling
//! - [`Pagination`]: LIMIT/OFFSET builder
//! - [`Keyset1`] / [`Keyset2`]: Keyset/cursor pagination builders
//! - [`Ident`]: Safe SQL identifier handling (see [`crate::Ident`])

use crate::Ident;
use crate::condition::Condition;
use crate::error::{OrmError, OrmResult};
use crate::ident::IntoIdent;
use crate::sql::Sql;
use std::sync::Arc;
use tokio_postgres::types::ToSql;

// ==================== WhereExpr: Boolean expression tree ====================

/// A WHERE clause expression tree supporting AND/OR/NOT/grouping.
///
/// # Example
/// ```ignore
/// use pgorm::builder::WhereExpr;
/// use pgorm::Condition;
///
/// let expr = WhereExpr::And(vec![
///     WhereExpr::Atom(Condition::eq("status", "active")?),
///     WhereExpr::Or(vec![
///         WhereExpr::Atom(Condition::eq("role", "admin")?),
///         WhereExpr::Atom(Condition::eq("role", "owner")?),
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
    /// Raw SQL template with parameter bindings.
    ///
    /// Each `?` in the template is replaced with `$N` and bound to the
    /// corresponding parameter. This is safer than `Raw` because values
    /// are properly parameterized.
    RawBind {
        template: String,
        params: Vec<Arc<dyn ToSql + Send + Sync>>,
    },
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
    #[allow(clippy::should_implement_trait)]
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

    /// Create a raw SQL expression with parameter bindings.
    ///
    /// Each `?` in the template is replaced with `$N` and bound to the
    /// corresponding parameter. This is safer than [`WhereExpr::raw`] because
    /// values are properly parameterized.
    ///
    /// # Example
    /// ```ignore
    /// let expr = WhereExpr::raw_bind(
    ///     "(user_id = ? OR ? = ANY(collaborators))",
    ///     vec![user_id, user_id],
    /// );
    /// ```
    pub fn raw_bind<T>(template: impl Into<String>, params: Vec<T>) -> Self
    where
        T: ToSql + Send + Sync + 'static,
    {
        WhereExpr::RawBind {
            template: template.into(),
            params: params.into_iter().map(|p| Arc::new(p) as _).collect(),
        }
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

    /// Returns `true` if this expression is the identity `TRUE` (i.e. `AND([])`).
    pub fn is_trivially_true(&self) -> bool {
        matches!(self, WhereExpr::And(exprs) if exprs.is_empty())
    }

    /// Returns `true` if this expression is the identity `FALSE` (i.e. `OR([])`).
    pub fn is_trivially_false(&self) -> bool {
        matches!(self, WhereExpr::Or(exprs) if exprs.is_empty())
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
            WhereExpr::RawBind { template, params } => {
                let mut param_iter = params.iter();
                for part in template.split('?') {
                    sql.push(part);
                    if let Some(param) = param_iter.next() {
                        sql.push_bind_value(param.clone());
                    }
                }
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
    fn to_sql(self) -> &'static str {
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
    fn to_sql(self) -> &'static str {
        match self {
            NullsOrder::First => "NULLS FIRST",
            NullsOrder::Last => "NULLS LAST",
        }
    }
}

/// A single ORDER BY item.
#[derive(Debug, Clone)]
pub enum OrderItem {
    Column {
        column: Ident,
        dir: SortDir,
        nulls: Option<NullsOrder>,
    },
    /// Raw SQL (escape hatch - use with extreme caution).
    Raw(String),
}

impl OrderItem {
    /// Create a new order item (validated identifier).
    pub fn new(column: Ident, dir: SortDir) -> Self {
        Self::Column {
            column,
            dir,
            nulls: None,
        }
    }

    /// Create a raw SQL order item.
    pub fn raw(sql: impl Into<String>) -> Self {
        Self::Raw(sql.into())
    }

    /// Set NULLS ordering (no-op for raw items).
    pub fn nulls(mut self, order: NullsOrder) -> Self {
        if let OrderItem::Column { nulls, .. } = &mut self {
            *nulls = Some(order);
        }
        self
    }

    fn append_to_sql(&self, sql: &mut Sql) {
        match self {
            OrderItem::Column { column, dir, nulls } => {
                sql.push_ident_ref(column);
                sql.push(" ");
                sql.push(dir.to_sql());
                if let Some(nulls) = nulls {
                    sql.push(" ");
                    sql.push(nulls.to_sql());
                }
            }
            OrderItem::Raw(s) => {
                sql.push(s);
            }
        }
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

    /// Add an ascending sort (validated identifier).
    pub fn asc(mut self, column: impl IntoIdent) -> OrmResult<Self> {
        self.items
            .push(OrderItem::new(column.into_ident()?, SortDir::Asc));
        Ok(self)
    }

    /// Add a descending sort (validated identifier).
    pub fn desc(mut self, column: impl IntoIdent) -> OrmResult<Self> {
        self.items
            .push(OrderItem::new(column.into_ident()?, SortDir::Desc));
        Ok(self)
    }

    /// Add a sort with custom direction and nulls ordering.
    pub fn with_nulls(
        mut self,
        column: impl IntoIdent,
        dir: SortDir,
        nulls: NullsOrder,
    ) -> OrmResult<Self> {
        self.items
            .push(OrderItem::new(column.into_ident()?, dir).nulls(nulls));
        Ok(self)
    }

    /// Add a custom order item.
    #[allow(clippy::should_implement_trait)]
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
    pub fn append_to_sql(&self, sql: &mut Sql) {
        if self.items.is_empty() {
            return;
        }
        sql.push(" ORDER BY ");
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                sql.push(", ");
            }
            item.append_to_sql(sql);
        }
    }

    /// Build the ORDER BY clause as a string.
    pub fn to_sql(&self) -> String {
        if self.items.is_empty() {
            return String::new();
        }
        let mut sql = Sql::empty();
        sql.push("ORDER BY ");
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                sql.push(", ");
            }
            item.append_to_sql(&mut sql);
        }
        sql.to_sql()
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
                "page must be >= 1, got {page}"
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

// ==================== Keyset pagination (cursor/seek method) ====================

type DynValue = Arc<dyn ToSql + Send + Sync>;

const DEFAULT_KEYSET_LIMIT: i64 = 50;

/// Cursor position for keyset pagination.
#[derive(Debug, Clone)]
pub enum Cursor<T> {
    After(T),
    Before(T),
}

fn seek_cmp_op<T>(dir: SortDir, cursor: &Cursor<T>) -> &'static str {
    match (dir, cursor) {
        (SortDir::Asc, Cursor::After(_)) => ">",
        (SortDir::Asc, Cursor::Before(_)) => "<",
        (SortDir::Desc, Cursor::After(_)) => "<",
        (SortDir::Desc, Cursor::Before(_)) => ">",
    }
}

/// Single-column keyset pagination builder.
///
/// Semantics:
/// - `after(x)`: "next page" in the current sort direction.
/// - `before(x)`: "previous page" in the current sort direction.
#[derive(Debug, Clone)]
pub struct Keyset1 {
    column: Ident,
    dir: SortDir,
    cursor: Option<Cursor<DynValue>>,
    limit: i64,
}

impl Keyset1 {
    pub fn asc(column: impl IntoIdent) -> OrmResult<Self> {
        Ok(Self {
            column: column.into_ident()?,
            dir: SortDir::Asc,
            cursor: None,
            limit: DEFAULT_KEYSET_LIMIT,
        })
    }

    pub fn desc(column: impl IntoIdent) -> OrmResult<Self> {
        Ok(Self {
            column: column.into_ident()?,
            dir: SortDir::Desc,
            cursor: None,
            limit: DEFAULT_KEYSET_LIMIT,
        })
    }

    pub fn after<T>(mut self, v: T) -> Self
    where
        T: ToSql + Send + Sync + 'static,
    {
        self.cursor = Some(Cursor::After(Arc::new(v)));
        self
    }

    pub fn before<T>(mut self, v: T) -> Self
    where
        T: ToSql + Send + Sync + 'static,
    {
        self.cursor = Some(Cursor::Before(Arc::new(v)));
        self
    }

    pub fn limit(mut self, n: i64) -> Self {
        self.limit = n;
        self
    }

    pub fn order_by(&self) -> OrderBy {
        OrderBy::new().add(OrderItem::new(self.column.clone(), self.dir))
    }

    pub fn into_where_expr(&self) -> OrmResult<WhereExpr> {
        let Some(cursor) = &self.cursor else {
            return Ok(WhereExpr::and(Vec::new()));
        };

        let op = seek_cmp_op(self.dir, cursor);
        let v = match cursor {
            Cursor::After(v) | Cursor::Before(v) => v.clone(),
        };
        Ok(WhereExpr::atom(Condition::cmp_dyn(
            self.column.clone(),
            op,
            v,
        )))
    }

    pub fn append_order_by_limit_to_sql(&self, sql: &mut Sql) -> OrmResult<()> {
        if self.limit < 1 {
            return Err(OrmError::validation(format!(
                "keyset limit must be >= 1, got {}",
                self.limit
            )));
        }
        self.order_by().append_to_sql(sql);
        sql.limit(self.limit);
        Ok(())
    }

    pub fn append_to_sql(&self, sql: &mut Sql) -> OrmResult<()> {
        let seek = self.into_where_expr()?;
        if !seek.is_trivially_true() {
            sql.push(" WHERE ");
            seek.append_to_sql(sql);
        }
        self.append_order_by_limit_to_sql(sql)
    }
}

/// Two-column keyset pagination builder (primary key + tie-breaker).
///
/// This uses Postgres tuple comparison:
/// `WHERE (a, b) < ($1, $2)` / `WHERE (a, b) > ($1, $2)`.
#[derive(Debug, Clone)]
pub struct Keyset2 {
    a: Ident,
    b: Ident,
    dir: SortDir,
    cursor: Option<Cursor<(DynValue, DynValue)>>,
    limit: i64,
}

impl Keyset2 {
    pub fn asc(a: impl IntoIdent, b: impl IntoIdent) -> OrmResult<Self> {
        Ok(Self {
            a: a.into_ident()?,
            b: b.into_ident()?,
            dir: SortDir::Asc,
            cursor: None,
            limit: DEFAULT_KEYSET_LIMIT,
        })
    }

    pub fn desc(a: impl IntoIdent, b: impl IntoIdent) -> OrmResult<Self> {
        Ok(Self {
            a: a.into_ident()?,
            b: b.into_ident()?,
            dir: SortDir::Desc,
            cursor: None,
            limit: DEFAULT_KEYSET_LIMIT,
        })
    }

    pub fn after<A, B>(mut self, a: A, b: B) -> Self
    where
        A: ToSql + Send + Sync + 'static,
        B: ToSql + Send + Sync + 'static,
    {
        self.cursor = Some(Cursor::After((Arc::new(a), Arc::new(b))));
        self
    }

    pub fn before<A, B>(mut self, a: A, b: B) -> Self
    where
        A: ToSql + Send + Sync + 'static,
        B: ToSql + Send + Sync + 'static,
    {
        self.cursor = Some(Cursor::Before((Arc::new(a), Arc::new(b))));
        self
    }

    pub fn limit(mut self, n: i64) -> Self {
        self.limit = n;
        self
    }

    pub fn order_by(&self) -> OrderBy {
        OrderBy::new()
            .add(OrderItem::new(self.a.clone(), self.dir))
            .add(OrderItem::new(self.b.clone(), self.dir))
    }

    pub fn into_where_expr(&self) -> OrmResult<WhereExpr> {
        let Some(cursor) = &self.cursor else {
            return Ok(WhereExpr::and(Vec::new()));
        };

        let op = seek_cmp_op(self.dir, cursor);
        let (va, vb) = match cursor {
            Cursor::After((va, vb)) | Cursor::Before((va, vb)) => (va.clone(), vb.clone()),
        };

        Ok(WhereExpr::atom(Condition::tuple2_cmp_dyn(
            self.a.clone(),
            self.b.clone(),
            op,
            va,
            vb,
        )))
    }

    pub fn append_order_by_limit_to_sql(&self, sql: &mut Sql) -> OrmResult<()> {
        if self.limit < 1 {
            return Err(OrmError::validation(format!(
                "keyset limit must be >= 1, got {}",
                self.limit
            )));
        }
        self.order_by().append_to_sql(sql);
        sql.limit(self.limit);
        Ok(())
    }

    pub fn append_to_sql(&self, sql: &mut Sql) -> OrmResult<()> {
        let seek = self.into_where_expr()?;
        if !seek.is_trivially_true() {
            sql.push(" WHERE ");
            seek.append_to_sql(sql);
        }
        self.append_order_by_limit_to_sql(sql)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== WhereExpr tests ====================

    #[test]
    fn where_atom() {
        let expr = WhereExpr::atom(Condition::eq("status", "active").unwrap());
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "status = $1");
    }

    #[test]
    fn where_and() {
        let expr = WhereExpr::And(vec![
            WhereExpr::Atom(Condition::eq("a", 1_i32).unwrap()),
            WhereExpr::Atom(Condition::eq("b", 2_i32).unwrap()),
        ]);
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "(a = $1 AND b = $2)");
    }

    #[test]
    fn where_or() {
        let expr = WhereExpr::Or(vec![
            WhereExpr::Atom(Condition::eq("role", "admin").unwrap()),
            WhereExpr::Atom(Condition::eq("role", "owner").unwrap()),
        ]);
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "(role = $1 OR role = $2)");
    }

    #[test]
    fn where_not() {
        let expr = WhereExpr::Not(Box::new(WhereExpr::Atom(
            Condition::eq("deleted", true).unwrap(),
        )));
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "(NOT deleted = $1)");
    }

    #[test]
    fn where_nested() {
        let expr = WhereExpr::And(vec![
            WhereExpr::Atom(Condition::eq("status", "active").unwrap()),
            WhereExpr::Or(vec![
                WhereExpr::Atom(Condition::eq("role", "admin").unwrap()),
                WhereExpr::Atom(Condition::eq("role", "owner").unwrap()),
            ]),
        ]);
        let mut sql = Sql::empty();
        expr.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "(status = $1 AND (role = $2 OR role = $3))");
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
        let a = WhereExpr::atom(Condition::eq("a", 1_i32).unwrap());
        let b = WhereExpr::atom(Condition::eq("b", 2_i32).unwrap());
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
        let order = OrderBy::new().asc("created_at").unwrap();
        assert_eq!(order.to_sql(), "ORDER BY created_at ASC");
    }

    #[test]
    fn order_by_single_desc() {
        let order = OrderBy::new().desc("priority").unwrap();
        assert_eq!(order.to_sql(), "ORDER BY priority DESC");
    }

    #[test]
    fn order_by_multiple() {
        let order = OrderBy::new()
            .asc("status")
            .unwrap()
            .desc("created_at")
            .unwrap();
        assert_eq!(order.to_sql(), "ORDER BY status ASC, created_at DESC");
    }

    #[test]
    fn order_by_with_nulls() {
        let order = OrderBy::new()
            .with_nulls("last_login", SortDir::Desc, NullsOrder::Last)
            .unwrap();
        assert_eq!(order.to_sql(), "ORDER BY last_login DESC NULLS LAST");
    }

    #[test]
    fn order_by_empty() {
        let order = OrderBy::new();
        assert!(order.is_empty());
        assert_eq!(order.to_sql(), "");
    }

    #[test]
    fn order_by_append() {
        let order = OrderBy::new().asc("id").unwrap();
        let mut sql = Sql::new("SELECT * FROM users");
        order.append_to_sql(&mut sql);
        assert_eq!(sql.to_sql(), "SELECT * FROM users ORDER BY id ASC");
    }

    #[test]
    fn order_by_validates_column() {
        let res = OrderBy::new().asc("valid_column; DROP TABLE users;");
        assert!(res.is_err());
    }

    // ==================== Keyset tests ====================

    #[test]
    fn keyset1_asc_after_generates_sql() {
        let keyset = Keyset1::asc("id").unwrap().after(100_i64).limit(10);
        let mut sql = Sql::new("SELECT * FROM users");
        keyset.append_to_sql(&mut sql).unwrap();
        assert_eq!(
            sql.to_sql(),
            "SELECT * FROM users WHERE id > $1 ORDER BY id ASC LIMIT $2"
        );
    }

    #[test]
    fn keyset1_desc_after_flips_comparator() {
        let keyset = Keyset1::desc("created_at").unwrap().after(123_i64).limit(5);
        let mut sql = Sql::new("SELECT * FROM users");
        keyset.append_to_sql(&mut sql).unwrap();
        assert_eq!(
            sql.to_sql(),
            "SELECT * FROM users WHERE created_at < $1 ORDER BY created_at DESC LIMIT $2"
        );
    }

    #[test]
    fn keyset1_composes_with_other_where_expr() {
        let keyset = Keyset1::asc("id").unwrap().after(10_i64).limit(3);
        let where_expr = WhereExpr::atom(Condition::eq("status", "active").unwrap())
            .and_with(keyset.into_where_expr().unwrap());

        let mut sql = Sql::new("SELECT * FROM users WHERE ");
        where_expr.append_to_sql(&mut sql);
        keyset.append_order_by_limit_to_sql(&mut sql).unwrap();

        assert_eq!(
            sql.to_sql(),
            "SELECT * FROM users WHERE (status = $1 AND id > $2) ORDER BY id ASC LIMIT $3"
        );
    }

    #[test]
    fn keyset2_desc_after_generates_tuple_cmp() {
        let keyset = Keyset2::desc("created_at", "id")
            .unwrap()
            .after(100_i64, 42_i64)
            .limit(20);
        let mut sql = Sql::new("SELECT * FROM users");
        keyset.append_to_sql(&mut sql).unwrap();
        assert_eq!(
            sql.to_sql(),
            "SELECT * FROM users WHERE (created_at, id) < ($1, $2) ORDER BY created_at DESC, id DESC LIMIT $3"
        );
    }

    #[test]
    fn keyset2_composes_with_other_where_expr() {
        let keyset = Keyset2::asc("created_at", "id")
            .unwrap()
            .after(123_i64, 456_i64)
            .limit(2);
        let where_expr = WhereExpr::atom(Condition::eq("status", "active").unwrap())
            .and_with(keyset.into_where_expr().unwrap());

        let mut sql = Sql::new("SELECT * FROM users WHERE ");
        where_expr.append_to_sql(&mut sql);
        keyset.append_order_by_limit_to_sql(&mut sql).unwrap();

        assert_eq!(
            sql.to_sql(),
            "SELECT * FROM users WHERE (status = $1 AND (created_at, id) > ($2, $3)) ORDER BY created_at ASC, id ASC LIMIT $4"
        );
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
