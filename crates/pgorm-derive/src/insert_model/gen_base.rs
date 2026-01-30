//! Base insert/upsert/returning code generation.
//!
//! This module contains code generation for:
//! - `insert` / `insert_many` methods
//! - `upsert` / `upsert_many` methods
//! - `insert_returning` / `insert_many_returning` methods
//! - `upsert_returning` / `upsert_many_returning` methods
//! - `__pgorm_diff_many_by_fk` helper for diff strategy

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::common::syn_types::{AutoTimestampKind, option_inner};

use super::attrs::StructAttrs;

/// Information about a field that needs to be bound in SQL.
pub(super) struct BindField {
    pub(super) ident: syn::Ident,
    pub(super) ty: syn::Type,
    pub(super) column: String,
    /// If this field has auto_now_add, store the timestamp kind.
    pub(super) auto_now_add: Option<AutoTimestampKind>,
}

impl BindField {
    /// Get the effective type for batch operations.
    /// For auto_now_add fields, this is the inner type (DateTime<Utc> instead of Option<DateTime<Utc>>).
    pub(super) fn batch_ty(&self) -> &syn::Type {
        if self.auto_now_add.is_some() {
            // For auto_now_add fields, extract the inner type from Option<T>
            option_inner(&self.ty).unwrap_or(&self.ty)
        } else {
            &self.ty
        }
    }
}

/// Generate the INSERT SQL string.
pub(super) fn generate_insert_sql(
    table_name: &str,
    columns: &[String],
    values: &[String],
) -> String {
    if columns.is_empty() {
        format!("INSERT INTO {table_name} DEFAULT VALUES")
    } else {
        format!(
            "INSERT INTO {} ({}) VALUES ({})",
            table_name,
            columns.join(", "),
            values.join(", ")
        )
    }
}

/// Generate the `insert` method.
pub(super) fn generate_insert_method(
    insert_sql: &str,
    batch_bind_fields: &[BindField],
) -> TokenStream {
    let bind_field_idents: Vec<_> = batch_bind_fields.iter().map(|f| f.ident.clone()).collect();

    let destructure = if bind_field_idents.is_empty() {
        quote! { let _ = self; }
    } else {
        quote! { let Self { #(#bind_field_idents),*, .. } = self; }
    };

    // Check if any field has auto_now_add
    let has_auto_now_add = batch_bind_fields.iter().any(|f| f.auto_now_add.is_some());

    let now_init = if has_auto_now_add {
        quote! { let __pgorm_now = ::chrono::Utc::now(); }
    } else {
        quote! {}
    };

    // Generate bind expressions, handling auto_now_add fields
    let bind_exprs: Vec<TokenStream> = batch_bind_fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            match f.auto_now_add {
                Some(AutoTimestampKind::DateTimeUtc) => {
                    quote! { #ident.unwrap_or(__pgorm_now) }
                }
                Some(AutoTimestampKind::NaiveDateTime) => {
                    quote! { #ident.unwrap_or_else(|| __pgorm_now.naive_utc()) }
                }
                None => {
                    quote! { #ident }
                }
            }
        })
        .collect();

    let insert_query_expr = bind_exprs.iter().fold(
        quote! { pgorm::query(#insert_sql) },
        |acc, expr| quote! { #acc.bind(#expr) },
    );

    quote! {
        /// Insert a new row into the target table.
        pub async fn insert(self, conn: &impl pgorm::GenericClient) -> pgorm::OrmResult<u64> {
            #destructure
            #now_init
            #insert_query_expr.execute(conn).await
        }
    }
}

/// Generate the `insert_many` method.
pub(super) fn generate_insert_many_method(
    table_name: &str,
    batch_bind_fields: &[BindField],
) -> TokenStream {
    if batch_bind_fields.is_empty() {
        quote! {
            /// Insert multiple rows.
            ///
            /// Falls back to per-row inserts when bulk insert isn't applicable.
            pub async fn insert_many(
                conn: &impl pgorm::GenericClient,
                rows: ::std::vec::Vec<Self>,
            ) -> pgorm::OrmResult<u64> {
                if rows.is_empty() {
                    return ::std::result::Result::Ok(0);
                }

                let mut affected = 0_u64;
                for row in rows {
                    affected += row.insert(conn).await?;
                }
                ::std::result::Result::Ok(affected)
            }
        }
    } else {
        let batch_columns: Vec<String> =
            batch_bind_fields.iter().map(|f| f.column.clone()).collect();
        let batch_columns_str = batch_columns.join(", ");
        let list_idents: Vec<syn::Ident> = batch_bind_fields
            .iter()
            .map(|f| format_ident!("__pgorm_insert_{}_list", f.ident))
            .collect();
        let field_idents: Vec<syn::Ident> =
            batch_bind_fields.iter().map(|f| f.ident.clone()).collect();

        // For batch inserts, use the batch_ty which extracts inner type for auto_now_add fields
        let batch_tys: Vec<&syn::Type> = batch_bind_fields.iter().map(|f| f.batch_ty()).collect();

        // Check if any field has auto_now_add
        let has_auto_now_add = batch_bind_fields.iter().any(|f| f.auto_now_add.is_some());

        let now_init = if has_auto_now_add {
            quote! { let __pgorm_now = ::chrono::Utc::now(); }
        } else {
            quote! {}
        };

        let init_lists: Vec<TokenStream> = list_idents
            .iter()
            .zip(batch_tys.iter())
            .map(|(list_ident, ty)| {
                quote! { let mut #list_ident: ::std::vec::Vec<#ty> = ::std::vec::Vec::with_capacity(rows.len()); }
            })
            .collect();

        // Generate push expressions, handling auto_now_add fields
        let push_lists: Vec<TokenStream> = list_idents
            .iter()
            .zip(batch_bind_fields.iter())
            .map(|(list_ident, f)| {
                let field_ident = &f.ident;
                let value_expr = match f.auto_now_add {
                    Some(AutoTimestampKind::DateTimeUtc) => {
                        quote! { #field_ident.unwrap_or(__pgorm_now) }
                    }
                    Some(AutoTimestampKind::NaiveDateTime) => {
                        quote! { #field_ident.unwrap_or_else(|| __pgorm_now.naive_utc()) }
                    }
                    None => {
                        quote! { #field_ident }
                    }
                };
                quote! { #list_ident.push(#value_expr); }
            })
            .collect();

        // Generate type casts using PgType trait at runtime (use batch_ty for correct array type)
        let type_cast_exprs: Vec<TokenStream> = batch_tys
            .iter()
            .enumerate()
            .map(|(i, ty)| {
                let idx = i + 1;
                quote! { ::std::format!("${}::{}", #idx, <#ty as pgorm::PgType>::pg_array_type()) }
            })
            .collect();

        let bind_lists_expr = list_idents.iter().fold(
            quote! { pgorm::query(sql) },
            |acc, list_ident| quote! { #acc.bind(#list_ident) },
        );

        quote! {
            /// Insert multiple rows using a single statement (UNNEST bulk insert).
            pub async fn insert_many(
                conn: &impl pgorm::GenericClient,
                rows: ::std::vec::Vec<Self>,
            ) -> pgorm::OrmResult<u64> {
                if rows.is_empty() {
                    return ::std::result::Result::Ok(0);
                }

                #now_init
                #(#init_lists)*

                for row in rows {
                    let Self { #(#field_idents),*, .. } = row;
                    #(#push_lists)*
                }

                static __PGORM_INSERT_MANY_SQL: ::std::sync::OnceLock<::std::string::String> =
                    ::std::sync::OnceLock::new();
                let sql = __PGORM_INSERT_MANY_SQL.get_or_init(|| {
                    let type_casts: ::std::vec::Vec<::std::string::String> =
                        ::std::vec![#(#type_cast_exprs),*];
                    ::std::format!(
                        "INSERT INTO {} ({}) SELECT * FROM UNNEST({})",
                        #table_name,
                        #batch_columns_str,
                        type_casts.join(", ")
                    )
                });

                #bind_lists_expr.execute(conn).await
            }
        }
    }
}

/// Conflict specification for UPSERT operations.
pub(super) enum ConflictSpec {
    Constraint(String),
    Columns(Vec<String>),
}

/// Determine conflict specification from struct attributes.
pub(super) fn determine_conflict_spec(
    struct_attrs: &StructAttrs,
    id_field: Option<&BindField>,
) -> Option<ConflictSpec> {
    if let Some(constraint) = struct_attrs.conflict_constraint.clone() {
        Some(ConflictSpec::Constraint(constraint))
    } else if let Some(cols) = struct_attrs.conflict_target.clone() {
        Some(ConflictSpec::Columns(cols))
    } else {
        id_field.map(|f| ConflictSpec::Columns(vec![f.column.clone()]))
    }
}

/// Generate upsert methods.
pub(super) fn generate_upsert_methods(
    table_name: &str,
    struct_attrs: &StructAttrs,
    conflict_spec: &ConflictSpec,
    batch_bind_fields: &[BindField],
    id_field: Option<&BindField>,
) -> TokenStream {
    // Build the ON CONFLICT clause
    let (on_conflict_clause, conflict_cols_for_exclusion): (String, Vec<String>) =
        match conflict_spec {
            ConflictSpec::Constraint(name) => (format!("ON CONFLICT ON CONSTRAINT {name}"), vec![]),
            ConflictSpec::Columns(cols) => {
                (format!("ON CONFLICT ({})", cols.join(", ")), cols.clone())
            }
        };

    // For upsert, we need all fields that should be in the INSERT (including conflict columns)
    let (upsert_columns, upsert_bind_idents, upsert_bind_field_tys): (
        Vec<String>,
        Vec<syn::Ident>,
        Vec<syn::Type>,
    ) = match conflict_spec {
        ConflictSpec::Constraint(_) => {
            // With constraint-based conflict, include all bind fields
            (
                batch_bind_fields.iter().map(|f| f.column.clone()).collect(),
                batch_bind_fields.iter().map(|f| f.ident.clone()).collect(),
                batch_bind_fields.iter().map(|f| f.ty.clone()).collect(),
            )
        }
        ConflictSpec::Columns(conflict_cols) => {
            if struct_attrs.conflict_target.is_some() {
                // With explicit conflict_target, include all insert columns (bind fields)
                let mut columns: Vec<String> =
                    batch_bind_fields.iter().map(|f| f.column.clone()).collect();
                let mut idents: Vec<syn::Ident> =
                    batch_bind_fields.iter().map(|f| f.ident.clone()).collect();
                let mut tys: Vec<syn::Type> =
                    batch_bind_fields.iter().map(|f| f.ty.clone()).collect();

                // If we have an id field and it's in the conflict columns, add it
                if let Some(id_f) = id_field {
                    if conflict_cols.contains(&id_f.column) && !columns.contains(&id_f.column) {
                        columns.insert(0, id_f.column.clone());
                        idents.insert(0, id_f.ident.clone());
                        tys.insert(0, id_f.ty.clone());
                    }
                }

                (columns, idents, tys)
            } else if let Some(id_f) = id_field {
                // Using id field as conflict target (original behavior)
                let columns: Vec<String> = std::iter::once(id_f.column.clone())
                    .chain(batch_bind_fields.iter().map(|f| f.column.clone()))
                    .collect();
                let idents: Vec<syn::Ident> = std::iter::once(id_f.ident.clone())
                    .chain(batch_bind_fields.iter().map(|f| f.ident.clone()))
                    .collect();
                let tys: Vec<syn::Type> = std::iter::once(id_f.ty.clone())
                    .chain(batch_bind_fields.iter().map(|f| f.ty.clone()))
                    .collect();
                (columns, idents, tys)
            } else {
                // Should not happen since we have conflict_cols
                (vec![], vec![], vec![])
            }
        }
    };

    let placeholders: Vec<String> = (1..=upsert_bind_idents.len())
        .map(|i| format!("${i}"))
        .collect();

    // Update assignments
    let mut update_assignments: Vec<String> =
        if let Some(update_cols) = &struct_attrs.conflict_update {
            update_cols
                .iter()
                .map(|col| format!("{col} = EXCLUDED.{col}"))
                .collect()
        } else {
            upsert_columns
                .iter()
                .filter(|col| !conflict_cols_for_exclusion.contains(col))
                .map(|col| format!("{col} = EXCLUDED.{col}"))
                .collect()
        };
    if update_assignments.is_empty() {
        if let Some(first_col) = upsert_columns.first() {
            update_assignments.push(format!("{first_col} = EXCLUDED.{first_col}"));
        }
    }

    let upsert_sql = format!(
        "INSERT INTO {} ({}) VALUES ({}) {} DO UPDATE SET {}",
        table_name,
        upsert_columns.join(", "),
        placeholders.join(", "),
        on_conflict_clause,
        update_assignments.join(", ")
    );

    let upsert_destructure = quote! { let Self { #(#upsert_bind_idents),*, .. } = self; };
    let upsert_query_expr = upsert_bind_idents.iter().fold(
        quote! { ::pgorm::query(#upsert_sql) },
        |acc, ident| quote! { #acc.bind(#ident) },
    );

    let upsert_method = quote! {
        /// Insert a row, or update it if a conflict occurs on the conflict target (Postgres UPSERT).
        pub async fn upsert(self, conn: &impl ::pgorm::GenericClient) -> ::pgorm::OrmResult<u64> {
            #upsert_destructure
            #upsert_query_expr.execute(conn).await
        }
    };

    let upsert_batch_list_idents: Vec<syn::Ident> = upsert_bind_idents
        .iter()
        .map(|ident| format_ident!("__pgorm_upsert_{}_list", ident))
        .collect();

    let upsert_batch_init_lists: Vec<TokenStream> = upsert_batch_list_idents
        .iter()
        .zip(upsert_bind_field_tys.iter())
        .map(|(list_ident, ty)| {
            quote! { let mut #list_ident: ::std::vec::Vec<#ty> = ::std::vec::Vec::with_capacity(rows.len()); }
        })
        .collect();

    let upsert_batch_push_lists: Vec<TokenStream> = upsert_batch_list_idents
        .iter()
        .zip(upsert_bind_idents.iter())
        .map(|(list_ident, field_ident)| quote! { #list_ident.push(#field_ident); })
        .collect();

    // Generate type casts for upsert batch using PgType trait
    let upsert_type_cast_exprs: Vec<TokenStream> = upsert_bind_field_tys
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let idx = i + 1;
            quote! { ::std::format!("${}::{}", #idx, <#ty as ::pgorm::PgType>::pg_array_type()) }
        })
        .collect();

    let upsert_many_query_expr = upsert_batch_list_idents.iter().fold(
        quote! { ::pgorm::query(&upsert_batch_sql) },
        |acc, list_ident| quote! { #acc.bind(#list_ident) },
    );

    let upsert_columns_str = upsert_columns.join(", ");
    let update_assignments_str = update_assignments.join(", ");

    let upsert_many_method = quote! {
        /// Insert or update multiple rows using a single statement (UNNEST + ON CONFLICT).
        pub async fn upsert_many(
            conn: &impl ::pgorm::GenericClient,
            rows: ::std::vec::Vec<Self>,
        ) -> ::pgorm::OrmResult<u64> {
            if rows.is_empty() {
                return ::std::result::Result::Ok(0);
            }

            #(#upsert_batch_init_lists)*

            for row in rows {
                let Self { #(#upsert_bind_idents),*, .. } = row;
                #(#upsert_batch_push_lists)*
            }

            let type_casts: ::std::vec::Vec<::std::string::String> = ::std::vec![#(#upsert_type_cast_exprs),*];
            let upsert_batch_sql = ::std::format!(
                "INSERT INTO {} ({}) SELECT * FROM UNNEST({}) {} DO UPDATE SET {}",
                #table_name,
                #upsert_columns_str,
                type_casts.join(", "),
                #on_conflict_clause,
                #update_assignments_str
            );

            #upsert_many_query_expr.execute(conn).await
        }
    };

    let upsert_returning_methods = if let Some(returning_ty) = struct_attrs.returning.as_ref() {
        let upsert_returning_query_expr = upsert_bind_idents.iter().fold(
            quote! { ::pgorm::query(sql) },
            |acc, ident| quote! { #acc.bind(#ident) },
        );
        let upsert_many_returning_query_expr = upsert_batch_list_idents.iter().fold(
            quote! { ::pgorm::query(sql) },
            |acc, list_ident| quote! { #acc.bind(#list_ident) },
        );

        quote! {
            /// UPSERT and return the resulting row mapped as the configured returning type.
            pub async fn upsert_returning(
                self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<#returning_ty>
            where
                #returning_ty: ::pgorm::FromRow,
            {
                #upsert_destructure
                let sql = ::std::format!(
                    "WITH {table} AS ({upsert} RETURNING *) SELECT {} FROM {table} {}",
                    #returning_ty::SELECT_LIST,
                    #returning_ty::JOIN_CLAUSE,
                    table = #table_name,
                    upsert = #upsert_sql,
                );
                #upsert_returning_query_expr.fetch_one_as::<#returning_ty>(conn).await
            }

            /// UPSERT multiple rows and return resulting rows mapped as the configured returning type.
            pub async fn upsert_many_returning(
                conn: &impl ::pgorm::GenericClient,
                rows: ::std::vec::Vec<Self>,
            ) -> ::pgorm::OrmResult<::std::vec::Vec<#returning_ty>>
            where
                #returning_ty: ::pgorm::FromRow,
            {
                if rows.is_empty() {
                    return ::std::result::Result::Ok(::std::vec::Vec::new());
                }

                #(#upsert_batch_init_lists)*

                for row in rows {
                    let Self { #(#upsert_bind_idents),*, .. } = row;
                    #(#upsert_batch_push_lists)*
                }

                let type_casts: ::std::vec::Vec<::std::string::String> = ::std::vec![#(#upsert_type_cast_exprs),*];
                let upsert_batch_sql = ::std::format!(
                    "INSERT INTO {} ({}) SELECT * FROM UNNEST({}) {} DO UPDATE SET {}",
                    #table_name,
                    #upsert_columns_str,
                    type_casts.join(", "),
                    #on_conflict_clause,
                    #update_assignments_str
                );

                let sql = ::std::format!(
                    "WITH {table} AS ({upsert} RETURNING *) SELECT {} FROM {table} {}",
                    #returning_ty::SELECT_LIST,
                    #returning_ty::JOIN_CLAUSE,
                    table = #table_name,
                    upsert = upsert_batch_sql,
                );

                #upsert_many_returning_query_expr.fetch_all_as::<#returning_ty>(conn).await
            }
        }
    } else {
        quote! {}
    };

    // Generate __pgorm_diff_many_by_fk helper for diff strategy
    let diff_helper = generate_diff_helper(
        table_name,
        &upsert_columns_str,
        &on_conflict_clause,
        &update_assignments_str,
        &upsert_bind_idents,
        &upsert_bind_field_tys,
    );

    quote! {
        #upsert_method
        #upsert_many_method
        #upsert_returning_methods
        #diff_helper
    }
}

/// Generate the `__pgorm_diff_many_by_fk` helper for diff strategy.
fn generate_diff_helper(
    table_name: &str,
    upsert_columns_str: &str,
    on_conflict_clause: &str,
    update_assignments_str: &str,
    upsert_bind_idents: &[syn::Ident],
    upsert_bind_field_tys: &[syn::Type],
) -> TokenStream {
    let field_idents: Vec<syn::Ident> = upsert_bind_idents.to_vec();
    let field_tys: Vec<syn::Type> = upsert_bind_field_tys.to_vec();
    let upsert_list_idents: Vec<syn::Ident> = field_idents
        .iter()
        .map(|ident| format_ident!("__pgorm_diff_{}_list", ident))
        .collect();

    let init_lists: Vec<TokenStream> = upsert_list_idents
        .iter()
        .zip(field_tys.iter())
        .map(|(list_ident, ty)| {
            quote! { let mut #list_ident: ::std::vec::Vec<#ty> = ::std::vec::Vec::with_capacity(rows.len()); }
        })
        .collect();

    let push_lists: Vec<TokenStream> = upsert_list_idents
        .iter()
        .zip(field_idents.iter())
        .map(|(list_ident, field_ident)| quote! { #list_ident.push(#field_ident); })
        .collect();

    // Generate type casts using PgType trait
    let type_cast_exprs: Vec<TokenStream> = field_tys
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            // fk_value is $1, so columns start at $2
            let idx = i + 2;
            quote! { ::std::format!("${}::{}", #idx, <#ty as ::pgorm::PgType>::pg_array_type()) }
        })
        .collect();

    let bind_lists_expr = upsert_list_idents.iter().fold(
        quote! { ::pgorm::query(&sql).bind(fk_value) },
        |acc, list_ident| quote! { #acc.bind(#list_ident) },
    );

    quote! {
        /// Internal helper for diff strategy. Upserts rows and deletes rows not in the new list.
        ///
        /// This method is used by UpdateModel's has_many_update with strategy = "diff".
        #[doc(hidden)]
        pub async fn __pgorm_diff_many_by_fk<I>(
            conn: &impl ::pgorm::GenericClient,
            fk_column: &'static str,
            fk_value: I,
            key_columns: &'static [&'static str],
            rows: ::std::vec::Vec<Self>,
        ) -> ::pgorm::OrmResult<u64>
        where
            I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
        {
            let rows_count = rows.len() as u64;

            if rows.is_empty() {
                // Empty list means delete all children with this fk
                let delete_sql = ::std::format!(
                    "DELETE FROM {} WHERE {} = $1",
                    #table_name,
                    fk_column
                );
                return ::pgorm::query(delete_sql).bind(fk_value).execute(conn).await;
            }

            // Collect columns into arrays
            #(#init_lists)*

            for row in rows {
                let Self { #(#field_idents),*, .. } = row;
                #(#push_lists)*
            }

            // Build the CTE query
            let type_casts: ::std::vec::Vec<::std::string::String> = ::std::vec![#(#type_cast_exprs),*];

            // Build key column equality conditions for NOT EXISTS
            let key_conditions: ::std::string::String = key_columns
                .iter()
                .map(|col| ::std::format!("u.{} = c.{}", col, col))
                .collect::<::std::vec::Vec<_>>()
                .join(" AND ");

            // Build RETURNING clause for key columns
            let returning_keys = key_columns.join(", ");

            let sql = ::std::format!(
                "WITH upserted AS (\
                    INSERT INTO {} ({}) \
                    SELECT * FROM UNNEST({}) AS t({}) \
                    {} DO UPDATE SET {} \
                    RETURNING {}\
                ), \
                deleted AS (\
                    DELETE FROM {} c \
                    WHERE c.{} = $1 \
                    AND NOT EXISTS (\
                        SELECT 1 FROM upserted u WHERE {}\
                    ) \
                    RETURNING 1\
                ) \
                SELECT (SELECT COUNT(*) FROM deleted) AS deleted_count",
                #table_name,
                #upsert_columns_str,
                type_casts.join(", "),
                #upsert_columns_str,
                #on_conflict_clause,
                #update_assignments_str,
                returning_keys,
                #table_name,
                fk_column,
                key_conditions
            );

            let row = #bind_lists_expr.fetch_one(conn).await?;
            let deleted_count: i64 = row.get(0);

            ::std::result::Result::Ok(rows_count + deleted_count as u64)
        }
    }
}

/// Generate the `insert_returning` and `insert_many_returning` methods.
pub(super) fn generate_returning_methods(
    table_name: &str,
    struct_attrs: &StructAttrs,
    insert_sql: &str,
    batch_bind_fields: &[BindField],
) -> TokenStream {
    let returning_ty = match struct_attrs.returning.as_ref() {
        Some(ty) => ty,
        None => return quote! {},
    };

    let bind_field_idents: Vec<_> = batch_bind_fields.iter().map(|f| f.ident.clone()).collect();

    let destructure = if bind_field_idents.is_empty() {
        quote! { let _ = self; }
    } else {
        quote! { let Self { #(#bind_field_idents),*, .. } = self; }
    };

    // Check if any field has auto_now_add
    let has_auto_now_add = batch_bind_fields.iter().any(|f| f.auto_now_add.is_some());

    let now_init = if has_auto_now_add {
        quote! { let __pgorm_now = ::chrono::Utc::now(); }
    } else {
        quote! {}
    };

    // Generate bind expressions, handling auto_now_add fields
    let bind_exprs: Vec<TokenStream> = batch_bind_fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            match f.auto_now_add {
                Some(AutoTimestampKind::DateTimeUtc) => {
                    quote! { #ident.unwrap_or(__pgorm_now) }
                }
                Some(AutoTimestampKind::NaiveDateTime) => {
                    quote! { #ident.unwrap_or_else(|| __pgorm_now.naive_utc()) }
                }
                None => {
                    quote! { #ident }
                }
            }
        })
        .collect();

    let returning_query_expr = bind_exprs
        .iter()
        .fold(quote! { pgorm::query(sql) }, |acc, expr| {
            quote! { #acc.bind(#expr) }
        });

    let insert_many_returning_method = if batch_bind_fields.is_empty() {
        quote! {
            /// Insert multiple rows and return created rows mapped as the configured returning type.
            ///
            /// Falls back to per-row inserts when bulk insert isn't applicable.
            pub async fn insert_many_returning(
                conn: &impl pgorm::GenericClient,
                rows: ::std::vec::Vec<Self>,
            ) -> pgorm::OrmResult<::std::vec::Vec<#returning_ty>>
            where
                #returning_ty: pgorm::FromRow,
            {
                if rows.is_empty() {
                    return ::std::result::Result::Ok(::std::vec::Vec::new());
                }

                let mut out = ::std::vec::Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(row.insert_returning(conn).await?);
                }
                ::std::result::Result::Ok(out)
            }
        }
    } else {
        let batch_columns: Vec<String> =
            batch_bind_fields.iter().map(|f| f.column.clone()).collect();

        let list_idents: Vec<syn::Ident> = batch_bind_fields
            .iter()
            .map(|f| format_ident!("__pgorm_insert_{}_list", f.ident))
            .collect();
        let field_idents: Vec<syn::Ident> =
            batch_bind_fields.iter().map(|f| f.ident.clone()).collect();

        // For batch inserts, use the batch_ty which extracts inner type for auto_now_add fields
        let batch_tys: Vec<&syn::Type> = batch_bind_fields.iter().map(|f| f.batch_ty()).collect();

        let batch_now_init = if has_auto_now_add {
            quote! { let __pgorm_now = ::chrono::Utc::now(); }
        } else {
            quote! {}
        };

        let init_lists: Vec<TokenStream> = list_idents
            .iter()
            .zip(batch_tys.iter())
            .map(|(list_ident, ty)| {
                quote! { let mut #list_ident: ::std::vec::Vec<#ty> = ::std::vec::Vec::with_capacity(rows.len()); }
            })
            .collect();

        // Generate push expressions, handling auto_now_add fields
        let push_lists: Vec<TokenStream> = list_idents
            .iter()
            .zip(batch_bind_fields.iter())
            .map(|(list_ident, f)| {
                let field_ident = &f.ident;
                let value_expr = match f.auto_now_add {
                    Some(AutoTimestampKind::DateTimeUtc) => {
                        quote! { #field_ident.unwrap_or(__pgorm_now) }
                    }
                    Some(AutoTimestampKind::NaiveDateTime) => {
                        quote! { #field_ident.unwrap_or_else(|| __pgorm_now.naive_utc()) }
                    }
                    None => {
                        quote! { #field_ident }
                    }
                };
                quote! { #list_ident.push(#value_expr); }
            })
            .collect();

        let batch_returning_query_expr = list_idents.iter().fold(
            quote! { pgorm::query(sql) },
            |acc, list_ident| quote! { #acc.bind(#list_ident) },
        );

        // Generate type casts for batch insert returning using PgType trait (use batch_ty)
        let batch_type_cast_exprs: Vec<TokenStream> = batch_tys
            .iter()
            .enumerate()
            .map(|(i, ty)| {
                let idx = i + 1;
                quote! { ::std::format!("${}::{}", #idx, <#ty as pgorm::PgType>::pg_array_type()) }
            })
            .collect();

        let batch_columns_str = batch_columns.join(", ");

        quote! {
            /// Insert multiple rows and return created rows mapped as the configured returning type.
            pub async fn insert_many_returning(
                conn: &impl pgorm::GenericClient,
                rows: ::std::vec::Vec<Self>,
            ) -> pgorm::OrmResult<::std::vec::Vec<#returning_ty>>
            where
                #returning_ty: pgorm::FromRow,
            {
                if rows.is_empty() {
                    return ::std::result::Result::Ok(::std::vec::Vec::new());
                }

                #batch_now_init
                #(#init_lists)*

                for row in rows {
                    let Self { #(#field_idents),*, .. } = row;
                    #(#push_lists)*
                }

                static __PGORM_INSERT_MANY_RETURNING_SQL: ::std::sync::OnceLock<::std::string::String> =
                    ::std::sync::OnceLock::new();
                let sql = __PGORM_INSERT_MANY_RETURNING_SQL.get_or_init(|| {
                    let type_casts: ::std::vec::Vec<::std::string::String> =
                        ::std::vec![#(#batch_type_cast_exprs),*];
                    let batch_insert_sql = ::std::format!(
                        "INSERT INTO {} ({}) SELECT * FROM UNNEST({})",
                        #table_name,
                        #batch_columns_str,
                        type_casts.join(", ")
                    );
                    ::std::format!(
                        "WITH {table} AS ({insert} RETURNING *) SELECT {} FROM {table} {}",
                        #returning_ty::SELECT_LIST,
                        #returning_ty::JOIN_CLAUSE,
                        table = #table_name,
                        insert = batch_insert_sql,
                    )
                });

                #batch_returning_query_expr.fetch_all_as::<#returning_ty>(conn).await
            }
        }
    };

    quote! {
        /// Insert and return the created row mapped as the configured returning type.
        pub async fn insert_returning(
            self,
            conn: &impl pgorm::GenericClient,
        ) -> pgorm::OrmResult<#returning_ty>
        where
            #returning_ty: pgorm::FromRow,
        {
            #destructure
            #now_init
            let sql = ::std::format!(
                "WITH {table} AS ({insert} RETURNING *) SELECT {} FROM {table} {}",
                #returning_ty::SELECT_LIST,
                #returning_ty::JOIN_CLAUSE,
                table = #table_name,
                insert = #insert_sql,
            );
            #returning_query_expr.fetch_one_as::<#returning_ty>(conn).await
        }

        #insert_many_returning_method
    }
}
