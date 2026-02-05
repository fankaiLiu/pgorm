//! CTE (WITH clause) query support.
//!
//! This module provides [`WithBuilder`] for constructing Common Table Expressions
//! (CTEs) with type-safe name validation and automatic parameter handling.
//!
//! # Example
//! ```ignore
//! use pgorm::prelude::*;
//!
//! // Simple CTE
//! let results = pgorm::sql("")
//!     .with(
//!         "active_users",
//!         pgorm::sql("SELECT id, name FROM users WHERE status = ").push_bind_owned("active"),
//!     )?
//!     .select(pgorm::sql("SELECT * FROM active_users"))
//!     .fetch_all_as::<User>(&client)
//!     .await?;
//!
//! // Recursive CTE
//! let tree = pgorm::sql("")
//!     .with_recursive(
//!         "org_tree",
//!         pgorm::sql("SELECT id, name, parent_id, 0 as level FROM employees WHERE parent_id IS NULL"),
//!         pgorm::sql("SELECT e.id, e.name, e.parent_id, t.level + 1 FROM employees e JOIN org_tree t ON e.parent_id = t.id"),
//!     )?
//!     .select(pgorm::sql("SELECT * FROM org_tree ORDER BY level"))
//!     .fetch_all_as::<OrgNode>(&client)
//!     .await?;
//! ```

use crate::error::OrmResult;
use crate::ident::{Ident, IntoIdent};
use crate::sql::Sql;

/// Internal representation of a single CTE definition.
struct CteDefinition {
    name: Ident,
    columns: Option<Vec<Ident>>,
    query: Sql,
    /// For recursive CTEs: the recursive part of the query.
    recursive_query: Option<Sql>,
    /// Whether to use UNION ALL (true) or UNION (false) for recursive CTEs.
    union_all: bool,
}

/// Builder for CTE (WITH clause) queries.
///
/// Created via [`Sql::with`] or [`Sql::with_recursive`].
///
/// # Example
/// ```ignore
/// pgorm::sql("")
///     .with("cte1", query1)?
///     .with("cte2", query2)?
///     .select(main_query)
///     .fetch_all_as::<T>(&client)
///     .await?;
/// ```
#[must_use]
pub struct WithBuilder {
    ctes: Vec<CteDefinition>,
    is_recursive: bool,
}

impl WithBuilder {
    /// Create a new WithBuilder (non-recursive).
    pub(crate) fn new(name: Ident, query: Sql) -> Self {
        Self {
            ctes: vec![CteDefinition {
                name,
                columns: None,
                query,
                recursive_query: None,
                union_all: false,
            }],
            is_recursive: false,
        }
    }

    /// Create a new WithBuilder with explicit column names.
    pub(crate) fn new_with_columns(name: Ident, columns: Vec<Ident>, query: Sql) -> Self {
        Self {
            ctes: vec![CteDefinition {
                name,
                columns: Some(columns),
                query,
                recursive_query: None,
                union_all: false,
            }],
            is_recursive: false,
        }
    }

    /// Create a new WithBuilder for recursive CTE.
    pub(crate) fn new_recursive(
        name: Ident,
        base: Sql,
        recursive: Sql,
        union_all: bool,
    ) -> Self {
        Self {
            ctes: vec![CteDefinition {
                name,
                columns: None,
                query: base,
                recursive_query: Some(recursive),
                union_all,
            }],
            is_recursive: true,
        }
    }

    /// Add another non-recursive CTE.
    pub fn with(mut self, name: impl IntoIdent, query: Sql) -> OrmResult<Self> {
        self.ctes.push(CteDefinition {
            name: name.into_ident()?,
            columns: None,
            query,
            recursive_query: None,
            union_all: false,
        });
        Ok(self)
    }

    /// Add a CTE with explicit column names.
    ///
    /// # Example
    /// ```ignore
    /// pgorm::sql("")
    ///     .with_columns(
    ///         "monthly_sales",
    ///         ["month", "total"],
    ///         pgorm::sql("SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY 1"),
    ///     )?
    ///     .select(pgorm::sql("SELECT * FROM monthly_sales WHERE total > 10000"))
    /// ```
    pub fn with_columns(
        mut self,
        name: impl IntoIdent,
        columns: impl IntoIterator<Item = impl IntoIdent>,
        query: Sql,
    ) -> OrmResult<Self> {
        let cols: Vec<Ident> = columns
            .into_iter()
            .map(|c| c.into_ident())
            .collect::<OrmResult<Vec<_>>>()?;
        self.ctes.push(CteDefinition {
            name: name.into_ident()?,
            columns: Some(cols),
            query,
            recursive_query: None,
            union_all: false,
        });
        Ok(self)
    }

    /// Add a recursive CTE (UNION ALL).
    pub fn with_recursive(
        mut self,
        name: impl IntoIdent,
        base: Sql,
        recursive: Sql,
    ) -> OrmResult<Self> {
        self.is_recursive = true;
        self.ctes.push(CteDefinition {
            name: name.into_ident()?,
            columns: None,
            query: base,
            recursive_query: Some(recursive),
            union_all: true,
        });
        Ok(self)
    }

    /// Add a recursive CTE using UNION (with deduplication).
    pub fn with_recursive_union(
        mut self,
        name: impl IntoIdent,
        base: Sql,
        recursive: Sql,
    ) -> OrmResult<Self> {
        self.is_recursive = true;
        self.ctes.push(CteDefinition {
            name: name.into_ident()?,
            columns: None,
            query: base,
            recursive_query: Some(recursive),
            union_all: false,
        });
        Ok(self)
    }

    /// Set the main query and produce the final [`Sql`].
    ///
    /// The returned `Sql` can be executed with `.fetch_all_as()`, `.execute()`, etc.
    pub fn select(self, main_query: Sql) -> Sql {
        self.build_with_query(main_query)
    }

    /// Shorthand: `SELECT * FROM <cte_name>` as the main query.
    pub fn select_from(self, cte_name: impl IntoIdent) -> OrmResult<Sql> {
        let ident = cte_name.into_ident()?;
        let mut main = Sql::new("SELECT * FROM ");
        main.push_ident_ref(&ident);
        Ok(self.build_with_query(main))
    }

    fn build_with_query(self, main_query: Sql) -> Sql {
        let mut result = Sql::empty();

        if self.is_recursive {
            result.push("WITH RECURSIVE ");
        } else {
            result.push("WITH ");
        }

        for (i, cte) in self.ctes.into_iter().enumerate() {
            if i > 0 {
                result.push(", ");
            }

            result.push_ident_ref(&cte.name);

            // Optional column list
            if let Some(cols) = &cte.columns {
                result.push("(");
                for (j, col) in cols.iter().enumerate() {
                    if j > 0 {
                        result.push(", ");
                    }
                    result.push_ident_ref(col);
                }
                result.push(")");
            }

            result.push(" AS (");
            result.push_sql(cte.query);

            // Recursive part
            if let Some(recursive) = cte.recursive_query {
                if cte.union_all {
                    result.push(" UNION ALL ");
                } else {
                    result.push(" UNION ");
                }
                result.push_sql(recursive);
            }

            result.push(")");
        }

        result.push(" ");
        result.push_sql(main_query);

        result
    }
}

#[cfg(test)]
mod tests {
    use super::WithBuilder;

    #[test]
    fn simple_cte() {
        let sql = crate::sql("")
            .with(
                "active_users",
                crate::sql("SELECT id FROM users WHERE status = ")
                    .bind("active"),
            )
            .unwrap()
            .select(crate::sql("SELECT * FROM active_users"));

        assert_eq!(
            sql.to_sql(),
            "WITH active_users AS (SELECT id FROM users WHERE status = $1) SELECT * FROM active_users"
        );
        assert_eq!(sql.params_ref().len(), 1);
    }

    #[test]
    fn multiple_ctes() {
        let sql = crate::sql("")
            .with(
                "cte1",
                crate::sql("SELECT id FROM users WHERE status = ")
                    .bind("active"),
            )
            .unwrap()
            .with(
                "cte2",
                crate::sql("SELECT * FROM orders WHERE amount > ")
                    .bind(100_i64),
            )
            .unwrap()
            .select(crate::sql(
                "SELECT u.id FROM cte1 u JOIN cte2 o ON o.user_id = u.id",
            ));

        assert_eq!(
            sql.to_sql(),
            "WITH cte1 AS (SELECT id FROM users WHERE status = $1), \
             cte2 AS (SELECT * FROM orders WHERE amount > $2) \
             SELECT u.id FROM cte1 u JOIN cte2 o ON o.user_id = u.id"
        );
        assert_eq!(sql.params_ref().len(), 2);
    }

    #[test]
    fn recursive_cte() {
        let sql = crate::sql("")
            .with_recursive(
                "org_tree",
                crate::sql("SELECT id, name, parent_id, 0 as level FROM employees WHERE parent_id IS NULL"),
                crate::sql("SELECT e.id, e.name, e.parent_id, t.level + 1 FROM employees e JOIN org_tree t ON e.parent_id = t.id"),
            )
            .unwrap()
            .select(crate::sql("SELECT * FROM org_tree ORDER BY level"));

        assert_eq!(
            sql.to_sql(),
            "WITH RECURSIVE org_tree AS (\
             SELECT id, name, parent_id, 0 as level FROM employees WHERE parent_id IS NULL \
             UNION ALL \
             SELECT e.id, e.name, e.parent_id, t.level + 1 FROM employees e JOIN org_tree t ON e.parent_id = t.id\
             ) SELECT * FROM org_tree ORDER BY level"
        );
        assert_eq!(sql.params_ref().len(), 0);
    }

    #[test]
    fn recursive_cte_with_params() {
        let sql = crate::sql("")
            .with_recursive(
                "category_tree",
                crate::sql("SELECT id, name, parent_id, 0 as depth FROM categories WHERE id = ")
                    .bind(1_i64),
                crate::sql("SELECT c.id, c.name, c.parent_id, ct.depth + 1 FROM categories c JOIN category_tree ct ON c.parent_id = ct.id WHERE ct.depth < ")
                    .bind(5_i32),
            )
            .unwrap()
            .select_from("category_tree")
            .unwrap();

        assert_eq!(
            sql.to_sql(),
            "WITH RECURSIVE category_tree AS (\
             SELECT id, name, parent_id, 0 as depth FROM categories WHERE id = $1 \
             UNION ALL \
             SELECT c.id, c.name, c.parent_id, ct.depth + 1 FROM categories c JOIN category_tree ct ON c.parent_id = ct.id WHERE ct.depth < $2\
             ) SELECT * FROM category_tree"
        );
        assert_eq!(sql.params_ref().len(), 2);
    }

    #[test]
    fn cte_with_columns() {
        let sql = crate::sql("")
            .with_columns(
                "monthly_sales",
                ["month", "total"],
                crate::sql("SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY 1"),
            )
            .unwrap()
            .select(crate::sql("SELECT * FROM monthly_sales WHERE total > ")
                .bind(10000_i64));

        assert_eq!(
            sql.to_sql(),
            "WITH monthly_sales(month, total) AS (\
             SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY 1\
             ) SELECT * FROM monthly_sales WHERE total > $1"
        );
        assert_eq!(sql.params_ref().len(), 1);
    }

    #[test]
    fn select_from_shorthand() {
        let sql = crate::sql("")
            .with(
                "stats",
                crate::sql("SELECT COUNT(*) as cnt FROM users"),
            )
            .unwrap()
            .select_from("stats")
            .unwrap();

        assert_eq!(
            sql.to_sql(),
            "WITH stats AS (SELECT COUNT(*) as cnt FROM users) SELECT * FROM stats"
        );
    }

    #[test]
    fn cte_validates_name() {
        let result = crate::sql("").with("invalid name!", crate::sql("SELECT 1"));
        assert!(result.is_err());
    }

    #[test]
    fn cte_validates_column_names() {
        let result = crate::sql("").with_columns(
            "valid_name",
            ["bad column!"],
            crate::sql("SELECT 1"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn mixed_recursive_and_non_recursive() {
        let sql = crate::sql("")
            .with(
                "active_users",
                crate::sql("SELECT id FROM users WHERE status = ").bind("active"),
            )
            .unwrap()
            .with_recursive(
                "org_tree",
                crate::sql("SELECT id, parent_id FROM employees WHERE parent_id IS NULL"),
                crate::sql("SELECT e.id, e.parent_id FROM employees e JOIN org_tree t ON e.parent_id = t.id"),
            )
            .unwrap()
            .select(crate::sql("SELECT * FROM org_tree WHERE id IN (SELECT id FROM active_users)"));

        // When any CTE is recursive, the WITH RECURSIVE keyword is used
        assert!(sql.to_sql().starts_with("WITH RECURSIVE "));
        assert_eq!(sql.params_ref().len(), 1);
    }

    #[test]
    fn recursive_cte_union_dedup() {
        let sql = crate::sql("")
            .with_recursive_union(
                "paths",
                crate::sql("SELECT start_node, end_node FROM edges WHERE start_node = ").bind(1_i64),
                crate::sql("SELECT e.start_node, e.end_node FROM edges e JOIN paths p ON e.start_node = p.end_node"),
            )
            .unwrap()
            .select_from("paths")
            .unwrap();

        // Should use UNION (not UNION ALL)
        assert!(sql.to_sql().contains(" UNION SELECT"));
        assert!(!sql.to_sql().contains("UNION ALL"));
    }
}
