//! Model derive macro implementation
//!
//! ## Module Structure
//!
//! - `attrs`: Struct and field attribute parsing (stub)
//! - `join`: JOIN clause parsing (stub)
//! - `query`: Query struct generation
//! - `relations`: has_many/belongs_to parsing (stub)

mod attrs;
mod join;
mod query;
mod relations;

use query::{generate_query_struct, QueryFieldInfo};

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use std::collections::HashMap;
use syn::ext::IdentExt;
use syn::{Data, DeriveInput, Fields, Result};

/// Represents a has_many relationship: Parent has many Children
struct HasManyRelation {
    /// The related model type (e.g., Review)
    model: syn::Path,
    /// The foreign key column in the child table (e.g., "product_id")
    foreign_key: String,
    /// The method name to generate (e.g., "reviews" -> select_reviews)
    method_name: String,
}

/// Represents a belongs_to relationship: Child belongs to Parent
struct BelongsToRelation {
    /// The related model type (e.g., Category)
    model: syn::Path,
    /// The foreign key column in this table (e.g., "category_id")
    foreign_key: String,
    /// The method name to generate (e.g., "category" -> select_category)
    method_name: String,
}

/// Represents a JOIN clause
struct JoinClause {
    /// The table to join
    table: String,
    /// The ON condition
    on: String,
    /// Join type: inner, left, right, full
    join_type: String,
}

/// Field info with table source
struct FieldInfo {
    /// The table this field comes from (None means main table)
    table: Option<String>,
    /// The column name in the database
    column: String,
}

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;

    let table_name = get_table_name(&input)?;
    let has_many_relations = get_has_many_relations(&input)?;
    let belongs_to_relations = get_belongs_to_relations(&input)?;
    let join_clauses = get_join_clauses(&input)?;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "Model can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "Model can only be derived for structs",
            ));
        }
    };

    let mut column_names = Vec::with_capacity(fields.len());
    let mut columns_for_alias = Vec::with_capacity(fields.len());
    let mut qualified_columns = Vec::with_capacity(fields.len()); // table.column AS field_name format
    let mut id_column: Option<String> = None;
    let mut id_field_type: Option<&syn::Type> = None;
    let mut id_field_ident: Option<syn::Ident> = None;
    let mut fk_field_types: HashMap<String, &syn::Type> = HashMap::with_capacity(fields.len() * 2);
    let mut fk_field_idents: HashMap<String, syn::Ident> = HashMap::with_capacity(fields.len() * 2);
    let mut query_fields: Vec<QueryFieldInfo> = Vec::with_capacity(fields.len());

    for field in fields.iter() {
        let field_ident = field.ident.clone().unwrap();
        let field_name = field_ident.to_string();
        let field_info = get_field_info(field, &table_name);
        let column_name = field_info.column.clone();
        let is_id = is_id_field(field);
        let is_main_table = match &field_info.table {
            Some(tbl) => tbl == &table_name,
            None => true,
        };

        column_names.push(column_name.clone());
        if is_main_table {
            columns_for_alias.push(column_name.clone());
        }

        // Build qualified column name with alias (table.column AS field_name)
        let qualified = if let Some(ref tbl) = field_info.table {
            // If field_name differs from column, use AS alias
            if field_name != column_name {
                format!("{}.{} AS {}", tbl, column_name, field_name)
            } else {
                format!("{}.{}", tbl, column_name)
            }
        } else {
            // Main table
            if field_name != column_name {
                format!("{}.{} AS {}", table_name, column_name, field_name)
            } else {
                format!("{}.{}", table_name, column_name)
            }
        };
        qualified_columns.push(qualified);

        if is_id {
            id_column = Some(column_name.clone());
            id_field_type = Some(&field.ty);
            id_field_ident = Some(field_ident.clone());
        }

        // Track fields for belongs_to foreign key lookups.
        //
        // Only include columns that originate from the model's main table; joined-table fields
        // are not valid foreign keys for this model.
        if is_main_table {
            fk_field_types.insert(field_name.clone(), &field.ty);
            fk_field_types.insert(column_name.clone(), &field.ty);
            fk_field_idents.insert(field_name.clone(), field_ident.clone());
            fk_field_idents.insert(column_name.clone(), field_ident.clone());
        }

        // Collect query field info
        // Skip fields that come from a different table (joined tables)
        let is_joined = match &field_info.table {
            Some(tbl) => tbl != &table_name, // Different table = joined
            None => false,                   // No table specified = main table
        };

        query_fields.push(QueryFieldInfo {
            name: field_ident,
            column: column_name,
            is_joined,
        });
    }

    // Build SELECT list based on whether we have JOINs
    let has_joins = !join_clauses.is_empty();
    let select_list = if has_joins {
        qualified_columns.join(", ")
    } else {
        column_names.join(", ")
    };

    // Build JOIN clause string
    let join_sql = join_clauses
        .iter()
        .map(|j| {
            let jt = match j.join_type.to_lowercase().as_str() {
                "left" => "LEFT JOIN",
                "right" => "RIGHT JOIN",
                "full" => "FULL OUTER JOIN",
                "cross" => "CROSS JOIN",
                _ => "INNER JOIN",
            };
            format!("{} {} ON {}", jt, j.table, j.on)
        })
        .collect::<Vec<_>>()
        .join(" ");

    let id_const = if let Some(id) = &id_column {
        quote! { pub const ID: &'static str = #id; }
    } else {
        quote! {}
    };

    // Generate primary key option for TableMeta
    let pk_option = if let Some(id) = &id_column {
        quote! { Some(#id) }
    } else {
        quote! { None }
    };

    // Generate select_one method only if there's an ID field
    let select_one_method = if let (Some(id_col), Some(id_ty)) = (&id_column, id_field_type) {
        let id_col_qualified = format!("{}.{}", table_name, id_col);
        if has_joins {
            quote! {
                /// Fetch a single record by its primary key.
                ///
                /// Returns `OrmError::NotFound` if no record is found.
                pub async fn select_one(
                    conn: &impl pgorm::GenericClient,
                    id: #id_ty,
                ) -> pgorm::OrmResult<Self>
                where
                    Self: pgorm::FromRow,
                {
                    let sql = ::std::format!(
                        "SELECT {} FROM {} {} WHERE {} = $1",
                        Self::SELECT_LIST,
                        Self::TABLE,
                        Self::JOIN_CLAUSE,
                        #id_col_qualified
                    );
                    let row = conn.query_one(&sql, &[&id]).await?;
                    pgorm::FromRow::from_row(&row)
                }
            }
        } else {
            quote! {
                /// Fetch a single record by its primary key.
                ///
                /// Returns `OrmError::NotFound` if no record is found.
                pub async fn select_one(
                    conn: &impl pgorm::GenericClient,
                    id: #id_ty,
                ) -> pgorm::OrmResult<Self>
                where
                    Self: pgorm::FromRow,
                {
                    let sql = ::std::format!(
                        "SELECT {} FROM {} WHERE {} = $1",
                        Self::SELECT_LIST,
                        Self::TABLE,
                        #id_col
                    );
                    let row = conn.query_one(&sql, &[&id]).await?;
                    pgorm::FromRow::from_row(&row)
                }
            }
        }
    } else {
        quote! {}
    };

    // Generate delete_by_id methods only if there's an ID field.
    let delete_by_id_methods = if let (Some(id_col), Some(id_ty)) = (&id_column, id_field_type) {
        let id_col_qualified = format!("{}.{}", table_name, id_col);

        quote! {
            /// Delete a single record by its primary key.
            ///
            /// Returns the number of affected rows (0 or 1).
            pub async fn delete_by_id(
                conn: &impl pgorm::GenericClient,
                id: #id_ty,
            ) -> pgorm::OrmResult<u64> {
                let sql = ::std::format!("DELETE FROM {} WHERE {} = $1", Self::TABLE, #id_col_qualified);
                conn.execute(&sql, &[&id]).await
            }

            /// Delete multiple records by their primary keys.
            ///
            /// Returns the number of affected rows.
            pub async fn delete_by_ids(
                conn: &impl pgorm::GenericClient,
                ids: ::std::vec::Vec<#id_ty>,
            ) -> pgorm::OrmResult<u64> {
                if ids.is_empty() {
                    return ::std::result::Result::Ok(0);
                }

                let sql = ::std::format!(
                    "DELETE FROM {} WHERE {} = ANY($1)",
                    Self::TABLE,
                    #id_col_qualified
                );
                conn.execute(&sql, &[&ids]).await
            }

            /// Delete a single record by its primary key and return the deleted row.
            ///
            /// Returns `OrmError::NotFound` if no record is found.
            pub async fn delete_by_id_returning(
                conn: &impl pgorm::GenericClient,
                id: #id_ty,
            ) -> pgorm::OrmResult<Self>
            where
                Self: pgorm::FromRow,
            {
                let sql = ::std::format!(
                    "WITH {table} AS (DELETE FROM {table} WHERE {id_qualified} = $1 RETURNING *) \
        SELECT {} FROM {table} {}",
                    Self::SELECT_LIST,
                    Self::JOIN_CLAUSE,
                    table = Self::TABLE,
                    id_qualified = #id_col_qualified,
                );
                let row = conn.query_one(&sql, &[&id]).await?;
                pgorm::FromRow::from_row(&row)
            }

            /// Delete multiple records by their primary keys and return deleted rows.
            pub async fn delete_by_ids_returning(
                conn: &impl pgorm::GenericClient,
                ids: ::std::vec::Vec<#id_ty>,
            ) -> pgorm::OrmResult<::std::vec::Vec<Self>>
            where
                Self: pgorm::FromRow,
            {
                if ids.is_empty() {
                    return ::std::result::Result::Ok(::std::vec::Vec::new());
                }

                let sql = ::std::format!(
                    "WITH {table} AS (DELETE FROM {table} WHERE {id_qualified} = ANY($1) RETURNING *) \
        SELECT {} FROM {table} {}",
                    Self::SELECT_LIST,
                    Self::JOIN_CLAUSE,
                    table = Self::TABLE,
                    id_qualified = #id_col_qualified,
                );
                let rows = conn.query(&sql, &[&ids]).await?;
                rows.iter().map(pgorm::FromRow::from_row).collect()
            }
        }
    } else {
        quote! {}
    };

    // Generate has_many methods (requires ID field)
    let has_many_methods: Vec<TokenStream> =
        if let (Some(id_ty), Some(id_field)) = (id_field_type, id_field_ident.as_ref()) {
            has_many_relations
                .iter()
                .map(|rel| {
                    let method_name = format_ident!("select_{}", rel.method_name);
                    let related_model = &rel.model;
                    let fk = &rel.foreign_key;

                    quote! {
                        /// Fetch all related records (has_many relationship).
                        pub async fn #method_name(
                            &self,
                            conn: &impl pgorm::GenericClient,
                        ) -> pgorm::OrmResult<::std::vec::Vec<#related_model>>
                        where
                            #related_model: pgorm::FromRow,
                            #id_ty: ::tokio_postgres::types::ToSql + ::core::marker::Sync,
                        {
                            let sql = ::std::format!(
                                "SELECT {} FROM {} WHERE {} = $1",
                                #related_model::SELECT_LIST,
                                #related_model::TABLE,
                                #fk
                            );
                            let rows = conn.query(&sql, &[&self.#id_field]).await?;
                            rows.iter().map(pgorm::FromRow::from_row).collect()
                        }
                    }
                })
                .collect()
        } else {
            vec![]
        };

    // Generate belongs_to methods
    let belongs_to_methods: Vec<TokenStream> = belongs_to_relations
        .iter()
        .filter_map(|rel| {
            // Find the field type for the foreign key
            let fk_type = fk_field_types.get(&rel.foreign_key)?;
            let fk_field = fk_field_idents.get(&rel.foreign_key)?;
            let method_name = format_ident!("select_{}", rel.method_name);
            let related_model = &rel.model;

            Some(quote! {
                /// Fetch the related parent record (belongs_to relationship).
                pub async fn #method_name(
                    &self,
                    conn: &impl pgorm::GenericClient,
                ) -> pgorm::OrmResult<#related_model>
                where
                    #related_model: pgorm::FromRow,
                    #fk_type: ::tokio_postgres::types::ToSql + ::core::marker::Sync,
                {
                    let sql = ::std::format!(
                        "SELECT {} FROM {} WHERE {} = $1",
                        #related_model::SELECT_LIST,
                        #related_model::TABLE,
                        #related_model::ID
                    );
                    let row = conn.query_one(&sql, &[&self.#fk_field]).await?;
                    pgorm::FromRow::from_row(&row)
                }
            })
        })
        .collect();

    // Generate JOIN_CLAUSE constant and modified select_all if joins exist
    let join_const = if has_joins {
        quote! { pub const JOIN_CLAUSE: &'static str = #join_sql; }
    } else {
        quote! { pub const JOIN_CLAUSE: &'static str = ""; }
    };

    let select_all_method = if has_joins {
        quote! {
            /// Fetch all records from the table (with JOINs if defined).
            pub async fn select_all(conn: &impl pgorm::GenericClient) -> pgorm::OrmResult<::std::vec::Vec<Self>>
            where
                Self: pgorm::FromRow,
            {
                let sql = ::std::format!("SELECT {} FROM {} {}", Self::SELECT_LIST, Self::TABLE, Self::JOIN_CLAUSE);
                let rows = conn.query(&sql, &[]).await?;
                rows.iter().map(pgorm::FromRow::from_row).collect()
            }
        }
    } else {
        quote! {
            /// Fetch all records from the table.
            pub async fn select_all(conn: &impl pgorm::GenericClient) -> pgorm::OrmResult<::std::vec::Vec<Self>>
            where
                Self: pgorm::FromRow,
            {
                let sql = ::std::format!("SELECT {} FROM {}", Self::SELECT_LIST, Self::TABLE);
                let rows = conn.query(&sql, &[]).await?;
                rows.iter().map(pgorm::FromRow::from_row).collect()
            }
        }
    };

    // Generate generated_sql method
    let generated_sql_method = if let Some(id_col) = &id_column {
        quote! {
            /// Returns all SQL statements this model generates.
            ///
            /// This includes SELECT, DELETE, and relationship queries.
            /// Useful for schema validation and SQL auditing.
            pub fn generated_sql() -> ::std::vec::Vec<(&'static str, ::std::string::String)> {
                let mut sqls = ::std::vec::Vec::new();

                // select_all
                let select_all = ::std::format!(
                    "SELECT {} FROM {} {}",
                    Self::SELECT_LIST,
                    Self::TABLE,
                    Self::JOIN_CLAUSE
                ).trim().to_string();
                sqls.push(("select_all", select_all));

                // select_one (by id)
                let select_one = ::std::format!(
                    "SELECT {} FROM {} {} WHERE {}.{} = $1",
                    Self::SELECT_LIST,
                    Self::TABLE,
                    Self::JOIN_CLAUSE,
                    Self::TABLE,
                    #id_col
                ).trim().to_string();
                sqls.push(("select_one", select_one));

                // delete_by_id
                let delete_by_id = ::std::format!(
                    "DELETE FROM {} WHERE {} = $1",
                    Self::TABLE,
                    #id_col
                );
                sqls.push(("delete_by_id", delete_by_id));

                // delete_by_id_returning
                let delete_returning = ::std::format!(
                    "WITH {} AS (DELETE FROM {} WHERE {}.{} = $1 RETURNING *) SELECT {} FROM {} {}",
                    Self::TABLE,
                    Self::TABLE,
                    Self::TABLE,
                    #id_col,
                    Self::SELECT_LIST,
                    Self::TABLE,
                    Self::JOIN_CLAUSE
                ).trim().to_string();
                sqls.push(("delete_by_id_returning", delete_returning));

                sqls
            }

            /// Check all generated SQL against the provided registry.
            ///
            /// Returns a map of SQL name to issues found.
            pub fn check_schema(registry: &pgorm::SchemaRegistry) -> ::std::collections::HashMap<&'static str, ::std::vec::Vec<pgorm::SchemaIssue>> {
                let mut results = ::std::collections::HashMap::new();
                for (name, sql) in Self::generated_sql() {
                    let issues = registry.check_sql(&sql);
                    if !issues.is_empty() {
                        results.insert(name, issues);
                    }
                }
                results
            }
        }
    } else {
        quote! {
            /// Returns all SQL statements this model generates.
            pub fn generated_sql() -> ::std::vec::Vec<(&'static str, ::std::string::String)> {
                let mut sqls = ::std::vec::Vec::new();

                // select_all (no id, so only this method)
                let select_all = ::std::format!(
                    "SELECT {} FROM {} {}",
                    Self::SELECT_LIST,
                    Self::TABLE,
                    Self::JOIN_CLAUSE
                ).trim().to_string();
                sqls.push(("select_all", select_all));

                sqls
            }

            /// Check all generated SQL against the provided registry.
            pub fn check_schema(registry: &pgorm::SchemaRegistry) -> ::std::collections::HashMap<&'static str, ::std::vec::Vec<pgorm::SchemaIssue>> {
                let mut results = ::std::collections::HashMap::new();
                for (name, sql) in Self::generated_sql() {
                    let issues = registry.check_sql(&sql);
                    if !issues.is_empty() {
                        results.insert(name, issues);
                    }
                }
                results
            }
        }
    };

    // Generate Query struct for dynamic queries
    let query_struct = generate_query_struct(name, &table_name, &query_fields, has_joins);

    // Generate ModelPk implementation only if there's an ID field
    let model_pk_impl =
        if let (Some(id_ty), Some(id_ident)) = (id_field_type, id_field_ident.as_ref()) {
            quote! {
                impl ::pgorm::ModelPk for #name {
                    type Id = #id_ty;

                    fn pk(&self) -> &Self::Id {
                        &self.#id_ident
                    }
                }
            }
        } else {
            quote! {}
        };

    Ok(quote! {
        impl #name {
            pub const TABLE: &'static str = #table_name;
            #id_const
            pub const SELECT_LIST: &'static str = #select_list;
            #join_const

            pub fn select_list_as(alias: &str) -> String {
                [#(#columns_for_alias),*]
                    .iter()
                    .map(|col| format!("{}.{}", alias, col))
                    .collect::<Vec<_>>()
                    .join(", ")
            }

            #select_all_method

            #select_one_method

            #delete_by_id_methods

            #(#has_many_methods)*

            #(#belongs_to_methods)*

            #generated_sql_method
        }

        impl pgorm::TableMeta for #name {
            fn table_name() -> &'static str {
                #table_name
            }

            fn columns() -> &'static [&'static str] {
                &[#(#column_names),*]
            }

            fn primary_key() -> Option<&'static str> {
                #pk_option
            }
        }

        #query_struct

        #model_pk_impl

        // Auto-register this model with CheckedClient via inventory
        pgorm::inventory::submit! {
            pgorm::ModelRegistration {
                register_fn: |registry: &mut pgorm::SchemaRegistry| {
                    registry.register::<#name>();
                }
            }
        }
    })
}

fn get_table_name(input: &DeriveInput) -> Result<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("orm") {
            if let Ok(nested) = attr.parse_args::<syn::MetaNameValue>() {
                if nested.path.is_ident("table") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(lit),
                        ..
                    }) = &nested.value
                    {
                        return Ok(lit.value());
                    }
                }
            }
        }
    }
    Err(syn::Error::new_spanned(
        input,
        "Model requires #[orm(table = \"table_name\")] attribute",
    ))
}

/// Get field info including table source and column name
/// Supports: #[orm(table = "categories", column = "name")]
fn get_field_info(field: &syn::Field, _default_table: &str) -> FieldInfo {
    let mut table: Option<String> = None;
    let mut column: Option<String> = None;

    for attr in &field.attrs {
        if attr.path().is_ident("orm") {
            // Try to parse as a list of key=value pairs
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<FieldAttr>(tokens) {
                    if parsed.table.is_some() {
                        table = parsed.table;
                    }
                    if parsed.column.is_some() {
                        column = parsed.column;
                    }
                }
            }
            // Also try single key=value for backward compatibility
            if let Ok(nested) = attr.parse_args::<syn::MetaNameValue>() {
                if nested.path.is_ident("column") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(lit),
                        ..
                    }) = &nested.value
                    {
                        column = Some(lit.value());
                    }
                } else if nested.path.is_ident("table") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(lit),
                        ..
                    }) = &nested.value
                    {
                        table = Some(lit.value());
                    }
                }
            }
        }
    }

    FieldInfo {
        table,
        column: column.unwrap_or_else(|| field.ident.as_ref().unwrap().to_string()),
    }
}

/// Helper struct for parsing field attributes
struct FieldAttr {
    is_id: bool,
    table: Option<String>,
    column: Option<String>,
}

impl syn::parse::Parse for FieldAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let mut is_id = false;
        let mut table = None;
        let mut column = None;

        // Parse comma-separated key=value pairs or single identifiers
        loop {
            if input.is_empty() {
                break;
            }

            // Check for 'id' keyword first
            if input.peek(syn::Ident) {
                let ident: syn::Ident = input.parse()?;
                if ident == "id" {
                    // Skip 'id' marker
                    is_id = true;
                    if input.peek(syn::Token![,]) {
                        let _: syn::Token![,] = input.parse()?;
                    }
                    continue;
                }
                // Otherwise it's a key = value
                let _: syn::Token![=] = input.parse()?;
                let value: syn::LitStr = input.parse()?;

                if ident == "table" {
                    table = Some(value.value());
                } else if ident == "column" {
                    column = Some(value.value());
                }
            }

            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            } else {
                break;
            }
        }

        Ok(FieldAttr {
            is_id,
            table,
            column,
        })
    }
}

fn is_id_field(field: &syn::Field) -> bool {
    for attr in &field.attrs {
        if attr.path().is_ident("orm") {
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<FieldAttr>(tokens) {
                    if parsed.is_id {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Parse has_many relations from struct attributes.
/// Example: #[orm(has_many(Review, foreign_key = "product_id", as = "reviews"))]
fn get_has_many_relations(input: &DeriveInput) -> Result<Vec<HasManyRelation>> {
    let mut relations = Vec::new();

    for attr in &input.attrs {
        if attr.path().is_ident("orm") {
            // Try to parse as a function-style attribute: orm(has_many(...))
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<HasManyAttr>(tokens) {
                    relations.push(HasManyRelation {
                        model: parsed.model,
                        foreign_key: parsed.foreign_key,
                        method_name: parsed.method_name,
                    });
                }
            }
        }
    }

    Ok(relations)
}

/// Parse belongs_to relations from struct attributes.
/// Example: #[orm(belongs_to(Category, foreign_key = "category_id", as = "category"))]
fn get_belongs_to_relations(input: &DeriveInput) -> Result<Vec<BelongsToRelation>> {
    let mut relations = Vec::new();

    for attr in &input.attrs {
        if attr.path().is_ident("orm") {
            // Try to parse as a function-style attribute: orm(belongs_to(...))
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<BelongsToAttr>(tokens) {
                    relations.push(BelongsToRelation {
                        model: parsed.model,
                        foreign_key: parsed.foreign_key,
                        method_name: parsed.method_name,
                    });
                }
            }
        }
    }

    Ok(relations)
}

/// Parse join clauses from struct attributes.
/// Example: #[orm(join(table = "categories", on = "products.category_id = categories.id", type = "inner"))]
fn get_join_clauses(input: &DeriveInput) -> Result<Vec<JoinClause>> {
    let mut joins = Vec::new();

    for attr in &input.attrs {
        if attr.path().is_ident("orm") {
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<JoinAttr>(tokens) {
                    joins.push(JoinClause {
                        table: parsed.table,
                        on: parsed.on,
                        join_type: parsed.join_type,
                    });
                }
            }
        }
    }

    Ok(joins)
}

/// Helper struct for parsing join attribute
struct JoinAttr {
    table: String,
    on: String,
    join_type: String,
}

impl syn::parse::Parse for JoinAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident != "join" {
            return Err(syn::Error::new(ident.span(), "expected join"));
        }

        let content;
        syn::parenthesized!(content in input);

        let mut table: Option<String> = None;
        let mut on: Option<String> = None;
        let mut join_type = "inner".to_string();

        // Parse key = value pairs
        loop {
            if content.is_empty() {
                break;
            }

            let key = syn::Ident::parse_any(&content)?;
            let _: syn::Token![=] = content.parse()?;
            let value: syn::LitStr = content.parse()?;

            if key == "table" {
                table = Some(value.value());
            } else if key == "on" {
                on = Some(value.value());
            } else if key == "type" {
                join_type = value.value();
            }

            if content.peek(syn::Token![,]) {
                let _: syn::Token![,] = content.parse()?;
            } else {
                break;
            }
        }

        let table = table
            .ok_or_else(|| syn::Error::new(Span::call_site(), "join requires table = \"...\""))?;

        let on =
            on.ok_or_else(|| syn::Error::new(Span::call_site(), "join requires on = \"...\""))?;

        Ok(JoinAttr {
            table,
            on,
            join_type,
        })
    }
}

/// Helper struct for parsing has_many attribute
struct HasManyAttr {
    model: syn::Path,
    foreign_key: String,
    method_name: String,
}

impl syn::parse::Parse for HasManyAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident != "has_many" {
            return Err(syn::Error::new(ident.span(), "expected has_many"));
        }

        let content;
        syn::parenthesized!(content in input);

        let model: syn::Path = content.parse()?;

        let mut foreign_key: Option<String> = None;
        let mut method_name: Option<String> = None;

        while content.peek(syn::Token![,]) {
            let _: syn::Token![,] = content.parse()?;
            if content.is_empty() {
                break;
            }

            // Use parse_any to handle keywords like 'as'
            let key = syn::Ident::parse_any(&content)?;
            let _: syn::Token![=] = content.parse()?;
            let value: syn::LitStr = content.parse()?;

            if key == "foreign_key" {
                foreign_key = Some(value.value());
            } else if key == "as" || key == "name" {
                method_name = Some(value.value());
            }
        }

        let fk = foreign_key.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "has_many requires foreign_key = \"...\"")
        })?;

        // Default method name: lowercase model name + 's'
        let name = method_name.unwrap_or_else(|| {
            let model_name = model.segments.last().unwrap().ident.to_string();
            format!("{}s", model_name.to_lowercase())
        });

        Ok(HasManyAttr {
            model,
            foreign_key: fk,
            method_name: name,
        })
    }
}

/// Helper struct for parsing belongs_to attribute
struct BelongsToAttr {
    model: syn::Path,
    foreign_key: String,
    method_name: String,
}

impl syn::parse::Parse for BelongsToAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident != "belongs_to" {
            return Err(syn::Error::new(ident.span(), "expected belongs_to"));
        }

        let content;
        syn::parenthesized!(content in input);

        let model: syn::Path = content.parse()?;

        let mut foreign_key: Option<String> = None;
        let mut method_name: Option<String> = None;

        while content.peek(syn::Token![,]) {
            let _: syn::Token![,] = content.parse()?;
            if content.is_empty() {
                break;
            }

            // Use parse_any to handle keywords like 'as'
            let key = syn::Ident::parse_any(&content)?;
            let _: syn::Token![=] = content.parse()?;
            let value: syn::LitStr = content.parse()?;

            if key == "foreign_key" {
                foreign_key = Some(value.value());
            } else if key == "as" || key == "name" {
                method_name = Some(value.value());
            }
        }

        let fk = foreign_key.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "belongs_to requires foreign_key = \"...\"",
            )
        })?;

        // Default method name: lowercase model name
        let name = method_name.unwrap_or_else(|| {
            let model_name = model.segments.last().unwrap().ident.to_string();
            model_name.to_lowercase()
        });

        Ok(BelongsToAttr {
            model,
            foreign_key: fk,
            method_name: name,
        })
    }
}
