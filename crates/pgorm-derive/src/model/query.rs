//! Query struct generation for Model derive macro.
//!
//! This module generates the dynamic query builder (e.g., `ProductQuery`) with:
//! - Column name constants
//! - Filtering methods (eq, ne, gt, gte, lt, lte, like, ilike, etc.)
//! - Ordering methods (order_by_asc, order_by_desc)
//! - Pagination methods (limit, offset, page)
//! - Execution methods (find, find_one, find_one_opt, count)

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;

/// Query field info for generating the Query struct
pub(super) struct QueryFieldInfo {
    /// The field name in the struct
    pub(super) name: syn::Ident,
    /// The column name in the database
    pub(super) column: String,
    /// Whether this field comes from a joined table (skip in query struct)
    pub(super) is_joined: bool,
}

/// Generate the Query struct for dynamic queries.
pub(super) fn generate_query_struct(
    model_name: &syn::Ident,
    table_name: &str,
    fields: &[QueryFieldInfo],
    has_joins: bool,
) -> TokenStream {
    let query_name = format_ident!("{}Query", model_name);

    // Filter out joined table fields for the query struct
    let query_fields: Vec<_> = fields.iter().filter(|f| !f.is_joined).collect();

    // Generate column constants
    let column_consts = gen_column_consts(&query_fields, table_name, has_joins);

    // Generate the base SQL depending on whether we have JOINs
    let base_sql = if has_joins {
        quote! {
            ::std::format!(
                "SELECT {} FROM {} {}",
                #model_name::SELECT_LIST,
                #model_name::TABLE,
                #model_name::JOIN_CLAUSE
            )
        }
    } else {
        quote! {
            ::std::format!(
                "SELECT {} FROM {}",
                #model_name::SELECT_LIST,
                #model_name::TABLE
            )
        }
    };

    // Generate filtering methods
    let filtering_methods = gen_filtering_methods();

    // Generate ordering methods
    let ordering_methods = gen_ordering_methods();

    // Generate pagination methods
    let pagination_methods = gen_pagination_methods();

    // Generate execution methods
    let execution_methods = gen_execution_methods(model_name, has_joins);

    quote! {
        /// Dynamic query builder for #model_name.
        ///
        /// Supports flexible filtering with chainable methods and pagination.
        ///
        /// # Example
        /// ```ignore
        /// // Simple equality
        /// let products = Product::query()
        ///     .eq("category_id", 1_i64)?
        ///     .find(&client).await?;
        ///
        /// // ILIKE query (case-insensitive pattern match)
        /// let products = Product::query()
        ///     .ilike("name", "%phone%")?
        ///     .find(&client).await?;
        ///
        /// // Range query
        /// let products = Product::query()
        ///     .gte("price_cents", 1000_i64)?
        ///     .lt("price_cents", 5000_i64)?
        ///     .find(&client).await?;
        ///
        /// // IN query
        /// let products = Product::query()
        ///     .in_list("category_id", vec![1_i64, 2, 3])?
        ///     .find(&client).await?;
        ///
        /// // Pagination + ordering
        /// let products = Product::query()
        ///     .eq("in_stock", true)?
        ///     .page(1, 10)?
        ///     .order_by_desc("created_at")?
        ///     .find(&client).await?;
        /// ```
        #[derive(Debug, Clone)]
        pub struct #query_name {
            where_expr: pgorm::WhereExpr,
            order_by: pgorm::OrderBy,
            pagination: pgorm::Pagination,
        }

        impl #query_name {
            /// Column name constants for type-safe queries.
            /// Use these instead of string literals to avoid typos.
            #(#column_consts)*
        }

        impl Default for #query_name {
            fn default() -> Self {
                Self {
                    where_expr: pgorm::WhereExpr::And(::std::vec::Vec::new()),
                    order_by: pgorm::OrderBy::new(),
                    pagination: pgorm::Pagination::new(),
                }
            }
        }

        impl #query_name {
            /// Create a new empty query.
            pub fn new() -> Self {
                Self::default()
            }

            // ==================== Filtering ====================
            #filtering_methods

            // ==================== Ordering ====================
            #ordering_methods

            // ==================== Pagination ====================
            #pagination_methods

            // ==================== Execution ====================
            fn build_base_sql(&self) -> pgorm::Sql {
                let mut q = pgorm::sql(#base_sql);
                if !self.where_expr.is_trivially_true() {
                    q.push(" WHERE ");
                    self.where_expr.append_to_sql(&mut q);
                }
                q
            }

            fn build_find_sql(&self) -> pgorm::Sql {
                let mut q = self.build_base_sql();
                self.order_by.append_to_sql(&mut q);
                self.pagination.append_to_sql(&mut q);
                q
            }

            fn build_first_sql(&self) -> pgorm::Sql {
                let mut q = self.build_base_sql();
                self.order_by.append_to_sql(&mut q);
                q.limit(1);
                q
            }

            #execution_methods
        }

        impl #model_name {
            /// Create a new query builder for dynamic queries.
            pub fn query() -> #query_name {
                #query_name::new()
            }
        }
    }
}

/// Generate column constants for type-safe queries.
///
/// We generate two forms:
/// - `<field_name>` (lowercase) for ergonomics when it doesn't conflict with methods.
/// - `COL_<FIELD_NAME>` (uppercase) as a conflict-free fallback.
fn gen_column_consts(
    query_fields: &[&QueryFieldInfo],
    table_name: &str,
    has_joins: bool,
) -> Vec<TokenStream> {
    query_fields
        .iter()
        .map(|f| {
            let field_ident = &f.name;
            let field_name = f.name.unraw().to_string();
            let col = if has_joins && !f.column.contains('.') {
                format!("{}.{}", table_name, f.column)
            } else {
                f.column.clone()
            };

            let upper_const_name = format_ident!("COL_{}", field_name.to_uppercase());
            let is_reserved = matches!(
                field_name.as_str(),
                "new"
                    | "eq"
                    | "ne"
                    | "gt"
                    | "gte"
                    | "lt"
                    | "lte"
                    | "like"
                    | "ilike"
                    | "not_like"
                    | "not_ilike"
                    | "is_null"
                    | "is_not_null"
                    | "in_list"
                    | "not_in"
                    | "between"
                    | "not_between"
                    | "and"
                    | "or"
                    | "raw"
                    | "paginate"
                    | "limit"
                    | "offset"
                    | "page"
                    | "order_by"
                    | "order_by_asc"
                    | "order_by_desc"
                    | "order_by_raw"
                    | "find"
                    | "count"
                    | "find_one"
                    | "find_one_opt"
            );

            if is_reserved {
                quote! {
                    pub const #upper_const_name: &'static str = #col;
                }
            } else {
                quote! {
                    #[allow(non_upper_case_globals)]
                    pub const #field_ident: &'static str = #col;
                    pub const #upper_const_name: &'static str = #col;
                }
            }
        })
        .collect()
}

/// Generate filtering methods (eq, ne, gt, gte, lt, lte, like, ilike, etc.)
fn gen_filtering_methods() -> TokenStream {
    quote! {
        /// Combine the current WHERE expression with another using `AND`.
        pub fn and(mut self, expr: pgorm::WhereExpr) -> Self {
            let current = self.where_expr;
            self.where_expr = current.and_with(expr);
            self
        }

        /// Combine the current WHERE expression with another using `OR`.
        pub fn or(mut self, expr: pgorm::WhereExpr) -> Self {
            let current = self.where_expr;
            self.where_expr = current.or_with(expr);
            self
        }

        /// Filter by equality: column = value
        pub fn eq<T>(mut self, column: impl pgorm::IntoIdent, value: T) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::eq(column, value)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by inequality: column != value
        pub fn ne<T>(mut self, column: impl pgorm::IntoIdent, value: T) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::ne(column, value)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by greater than: column > value
        pub fn gt<T>(mut self, column: impl pgorm::IntoIdent, value: T) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::gt(column, value)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by greater than or equal: column >= value
        pub fn gte<T>(mut self, column: impl pgorm::IntoIdent, value: T) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::gte(column, value)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by less than: column < value
        pub fn lt<T>(mut self, column: impl pgorm::IntoIdent, value: T) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::lt(column, value)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by less than or equal: column <= value
        pub fn lte<T>(mut self, column: impl pgorm::IntoIdent, value: T) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::lte(column, value)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by LIKE pattern: column LIKE pattern
        pub fn like<T>(mut self, column: impl pgorm::IntoIdent, pattern: T) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::like(column, pattern)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by case-insensitive ILIKE pattern: column ILIKE pattern
        pub fn ilike<T>(mut self, column: impl pgorm::IntoIdent, pattern: T) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::ilike(column, pattern)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by NOT LIKE pattern: column NOT LIKE pattern
        pub fn not_like<T>(mut self, column: impl pgorm::IntoIdent, pattern: T) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::not_like(column, pattern)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by NOT ILIKE pattern: column NOT ILIKE pattern
        pub fn not_ilike<T>(mut self, column: impl pgorm::IntoIdent, pattern: T) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::not_ilike(column, pattern)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by IS NULL: column IS NULL
        pub fn is_null(mut self, column: impl pgorm::IntoIdent) -> pgorm::OrmResult<Self> {
            let cond = pgorm::Condition::is_null(column)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by IS NOT NULL: column IS NOT NULL
        pub fn is_not_null(mut self, column: impl pgorm::IntoIdent) -> pgorm::OrmResult<Self> {
            let cond = pgorm::Condition::is_not_null(column)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by IN list: column IN (values...)
        pub fn in_list<T>(mut self, column: impl pgorm::IntoIdent, values: ::std::vec::Vec<T>) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::in_list(column, values)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by NOT IN list: column NOT IN (values...)
        pub fn not_in<T>(mut self, column: impl pgorm::IntoIdent, values: ::std::vec::Vec<T>) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::not_in(column, values)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by BETWEEN: column BETWEEN from AND to
        pub fn between<T>(
            mut self,
            column: impl pgorm::IntoIdent,
            from: T,
            to: T,
        ) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::between(column, from, to)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Filter by NOT BETWEEN: column NOT BETWEEN from AND to
        pub fn not_between<T>(
            mut self,
            column: impl pgorm::IntoIdent,
            from: T,
            to: T,
        ) -> pgorm::OrmResult<Self>
        where
            T: ::tokio_postgres::types::ToSql + ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            let cond = pgorm::Condition::not_between(column, from, to)?;
            let current = self.where_expr;
            self.where_expr = current.and_with(cond.into());
            ::std::result::Result::Ok(self)
        }

        /// Add a raw WHERE expression (escape hatch).
        ///
        /// # Safety
        /// Be careful with SQL injection when using raw expressions.
        pub fn raw(mut self, sql: impl ::core::convert::Into<::std::string::String>) -> Self {
            let current = self.where_expr;
            self.where_expr = current.and_with(pgorm::WhereExpr::raw(sql));
            self
        }
    }
}

/// Generate ordering methods (order_by_asc, order_by_desc, order_by_raw)
fn gen_ordering_methods() -> TokenStream {
    quote! {
        /// Replace the ORDER BY builder.
        pub fn order_by(mut self, order_by: pgorm::OrderBy) -> Self {
            self.order_by = order_by;
            self
        }

        /// Add an ascending sort.
        pub fn order_by_asc(mut self, column: impl pgorm::IntoIdent) -> pgorm::OrmResult<Self> {
            let order = self.order_by;
            self.order_by = order.asc(column)?;
            ::std::result::Result::Ok(self)
        }

        /// Add a descending sort.
        pub fn order_by_desc(mut self, column: impl pgorm::IntoIdent) -> pgorm::OrmResult<Self> {
            let order = self.order_by;
            self.order_by = order.desc(column)?;
            ::std::result::Result::Ok(self)
        }

        /// Add a raw ORDER BY item (escape hatch).
        ///
        /// # Safety
        /// Be careful with SQL injection when using raw ORDER BY strings.
        pub fn order_by_raw(mut self, sql: impl ::core::convert::Into<::std::string::String>) -> Self {
            let order = self.order_by;
            self.order_by = order.add(pgorm::OrderItem::raw(sql));
            self
        }
    }
}

/// Generate pagination methods (limit, offset, page, paginate)
fn gen_pagination_methods() -> TokenStream {
    quote! {
        /// Replace the pagination builder.
        pub fn paginate(mut self, pagination: pgorm::Pagination) -> Self {
            self.pagination = pagination;
            self
        }

        /// Set LIMIT.
        pub fn limit(mut self, limit: i64) -> Self {
            self.pagination = self.pagination.limit(limit);
            self
        }

        /// Set OFFSET.
        pub fn offset(mut self, offset: i64) -> Self {
            self.pagination = self.pagination.offset(offset);
            self
        }

        /// Page-based pagination (page numbers start at 1).
        pub fn page(mut self, page: i64, per_page: i64) -> pgorm::OrmResult<Self> {
            self.pagination = pgorm::Pagination::page(page, per_page)?;
            ::std::result::Result::Ok(self)
        }
    }
}

/// Generate execution methods (find, find_one, find_one_opt, count)
fn gen_execution_methods(model_name: &syn::Ident, has_joins: bool) -> TokenStream {
    quote! {
        /// Execute the query and return matching records.
        pub async fn find(
            &self,
            conn: &impl pgorm::GenericClient,
        ) -> pgorm::OrmResult<::std::vec::Vec<#model_name>>
        where
            #model_name: pgorm::FromRow,
        {
            let q = self.build_find_sql();
            q.fetch_all_as(conn).await
        }

        /// Count the number of matching records.
        pub async fn count(&self, conn: &impl pgorm::GenericClient) -> pgorm::OrmResult<i64> {
            let mut q = pgorm::sql(if #has_joins {
                ::std::format!(
                    "SELECT COUNT(*) FROM {} {}",
                    #model_name::TABLE,
                    #model_name::JOIN_CLAUSE
                )
            } else {
                ::std::format!("SELECT COUNT(*) FROM {}", #model_name::TABLE)
            });

            if !self.where_expr.is_trivially_true() {
                q.push(" WHERE ");
                self.where_expr.append_to_sql(&mut q);
            }

            q.fetch_scalar_one(conn).await
        }

        /// Execute the query and return the first matching record.
        pub async fn find_one(
            &self,
            conn: &impl pgorm::GenericClient,
        ) -> pgorm::OrmResult<#model_name>
        where
            #model_name: pgorm::FromRow,
        {
            let q = self.build_first_sql();
            q.fetch_one_as(conn).await
        }

        /// Execute the query and return the first matching record, or None if not found.
        pub async fn find_one_opt(
            &self,
            conn: &impl pgorm::GenericClient,
        ) -> pgorm::OrmResult<::std::option::Option<#model_name>>
        where
            #model_name: pgorm::FromRow,
        {
            let q = self.build_first_sql();
            q.fetch_opt_as(conn).await
        }
    }
}
