//! UpdateModel derive macro implementation

mod attrs;
mod gen_base;
mod gen_children;
mod gen_graph;
mod graph_decl;
mod graph_parse;
mod types;

use attrs::{StructAttrs, get_field_attrs, get_struct_attrs};
use graph_decl::{UpdateGraphDeclarations, UpdateStrategy};
use types::option_inner;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Result};

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let attrs = get_struct_attrs(&input)?;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "UpdateModel can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "UpdateModel can only be derived for structs",
            ));
        }
    };

    let id_col_expr = if let Some(model_ty) = attrs.model.as_ref() {
        quote! { #model_ty::ID }
    } else if let Some(id_col) = attrs.id_column.as_ref() {
        quote! { #id_col }
    } else if let Some(returning_ty) = attrs.returning.as_ref() {
        quote! { #returning_ty::ID }
    } else {
        return Err(syn::Error::new_spanned(
            &input,
            "UpdateModel requires #[orm(id_column = \"...\")] or #[orm(model = \"...\")] (or a returning type with ID)",
        ));
    };

    let mut destructure_idents: Vec<syn::Ident> = Vec::new();
    let mut set_stmts: Vec<TokenStream> = Vec::new();

    // Get field names used by graph declarations
    let graph_field_names = attrs.graph.graph_field_names();

    for field in fields.iter() {
        let field_ident = field.ident.clone().unwrap();
        let field_name = field_ident.to_string();
        let field_ty = &field.ty;

        // Skip fields used by graph declarations
        if graph_field_names.contains(&field_name) {
            continue;
        }

        let field_attrs = get_field_attrs(field)?;

        if let Some(field_table) = &field_attrs.table {
            if field_table != &attrs.table {
                return Err(syn::Error::new_spanned(
                    field,
                    "UpdateModel does not support fields from joined/other tables",
                ));
            }
        }

        if field_attrs.skip_update {
            continue;
        }

        let column_name = field_attrs.column.unwrap_or(field_name);

        if field_attrs.default {
            set_stmts.push(quote! {
                if !first {
                    q.push(", ");
                } else {
                    first = false;
                }
                q.push(#column_name);
                q.push(" = DEFAULT");
            });
            continue;
        }

        // Non-default fields need the value.
        destructure_idents.push(field_ident.clone());

        if let Some(inner) = option_inner(field_ty) {
            if option_inner(inner).is_some() {
                // Option<Option<T>>: Some(Some(v)) => bind; Some(None) => NULL; None => skip.
                set_stmts.push(quote! {
                    if let Some(v) = #field_ident {
                        if !first {
                            q.push(", ");
                        } else {
                            first = false;
                        }
                        q.push(#column_name);
                        q.push(" = ");
                        match v {
                            Some(vv) => {
                                q.push_bind(vv);
                            }
                            None => {
                                q.push("NULL");
                            }
                        }
                    }
                });
            } else {
                // Option<T>: Some(v) => bind; None => skip.
                set_stmts.push(quote! {
                    if let Some(v) = #field_ident {
                        if !first {
                            q.push(", ");
                        } else {
                            first = false;
                        }
                        q.push(#column_name);
                        q.push(" = ");
                        q.push_bind(v);
                    }
                });
            }
        } else {
            // T: always bind.
            set_stmts.push(quote! {
                if !first {
                    q.push(", ");
                } else {
                    first = false;
                }
                q.push(#column_name);
                q.push(" = ");
                q.push_bind(#field_ident);
            });
        }
    }

    let table_name = &attrs.table;

    let destructure = if destructure_idents.is_empty() {
        quote! { let _ = self; }
    } else {
        quote! { let Self { #(#destructure_idents),*, .. } = self; }
    };

    let update_by_id_method = quote! {
        /// Update columns by primary key (patch-style).
        pub async fn update_by_id<I>(
            self,
            conn: &impl pgorm::GenericClient,
            id: I,
        ) -> pgorm::OrmResult<u64>
        where
            I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
        {
            #destructure

            let mut q = pgorm::sql("UPDATE ");
            q.push(#table_name);
            q.push(" SET ");

            let mut first = true;
            #(#set_stmts)*

            if first {
                return Err(pgorm::OrmError::Validation(
                    "UpdateModel: no fields to update".to_string(),
                ));
            }

            q.push(" WHERE ");
            q.push(#table_name);
            q.push(".");
            q.push(#id_col_expr);
            q.push(" = ");
            q.push_bind(id);

            q.execute(conn).await
        }
    };

    let update_by_ids_method = quote! {
        /// Update columns by primary key for multiple rows (patch-style).
        ///
        /// The same patch is applied to every matched row.
        pub async fn update_by_ids<I>(
            self,
            conn: &impl pgorm::GenericClient,
            ids: ::std::vec::Vec<I>,
        ) -> pgorm::OrmResult<u64>
        where
            I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
        {
            if ids.is_empty() {
                return ::std::result::Result::Ok(0);
            }

            #destructure

            let mut q = pgorm::sql("UPDATE ");
            q.push(#table_name);
            q.push(" SET ");

            let mut first = true;
            #(#set_stmts)*

            if first {
                return Err(pgorm::OrmError::Validation(
                    "UpdateModel: no fields to update".to_string(),
                ));
            }

            q.push(" WHERE ");
            q.push(#table_name);
            q.push(".");
            q.push(#id_col_expr);
            q.push(" = ANY(");
            q.push_bind(ids);
            q.push(")");

            q.execute(conn).await
        }
    };

    let update_by_id_returning_method = if let Some(returning_ty) = attrs.returning.as_ref() {
        quote! {
            /// Update columns by primary key and return the updated row mapped as the configured returning type.
            pub async fn update_by_id_returning<I>(
                self,
                conn: &impl pgorm::GenericClient,
                id: I,
            ) -> pgorm::OrmResult<#returning_ty>
            where
                I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
                #returning_ty: pgorm::FromRow,
            {
                #destructure

                let mut q = pgorm::Sql::empty();
                q.push("WITH ");
                q.push(#table_name);
                q.push(" AS (UPDATE ");
                q.push(#table_name);
                q.push(" SET ");

                let mut first = true;
                #(#set_stmts)*

                if first {
                    return Err(pgorm::OrmError::Validation(
                        "UpdateModel: no fields to update".to_string(),
                    ));
                }

                q.push(" WHERE ");
                q.push(#table_name);
                q.push(".");
                q.push(#id_col_expr);
                q.push(" = ");
                q.push_bind(id);

                q.push(" RETURNING *) SELECT ");
                q.push(#returning_ty::SELECT_LIST);
                q.push(" FROM ");
                q.push(#table_name);
                q.push(" ");
                q.push(#returning_ty::JOIN_CLAUSE);

                q.fetch_one_as::<#returning_ty>(conn).await
            }

            /// Update columns by primary key for multiple rows and return updated rows
            /// mapped as the configured returning type.
            ///
            /// The same patch is applied to every matched row.
            pub async fn update_by_ids_returning<I>(
                self,
                conn: &impl pgorm::GenericClient,
                ids: ::std::vec::Vec<I>,
            ) -> pgorm::OrmResult<::std::vec::Vec<#returning_ty>>
            where
                I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
                #returning_ty: pgorm::FromRow,
            {
                if ids.is_empty() {
                    return ::std::result::Result::Ok(::std::vec::Vec::new());
                }

                #destructure

                let mut q = pgorm::Sql::empty();
                q.push("WITH ");
                q.push(#table_name);
                q.push(" AS (UPDATE ");
                q.push(#table_name);
                q.push(" SET ");

                let mut first = true;
                #(#set_stmts)*

                if first {
                    return Err(pgorm::OrmError::Validation(
                        "UpdateModel: no fields to update".to_string(),
                    ));
                }

                q.push(" WHERE ");
                q.push(#table_name);
                q.push(".");
                q.push(#id_col_expr);
                q.push(" = ANY(");
                q.push_bind(ids);
                q.push(")");

                q.push(" RETURNING *) SELECT ");
                q.push(#returning_ty::SELECT_LIST);
                q.push(" FROM ");
                q.push(#table_name);
                q.push(" ");
                q.push(#returning_ty::JOIN_CLAUSE);

                q.fetch_all_as::<#returning_ty>(conn).await
            }
        }
    } else {
        quote! {}
    };

    // Generate update_by_id_graph methods
    let update_graph_methods = generate_update_graph_methods(&attrs, &id_col_expr)?;

    Ok(quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            pub const TABLE: &'static str = #table_name;

            #update_by_id_method

            #update_by_ids_method

            #update_by_id_returning_method

            #update_graph_methods
        }
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// update_by_id_graph methods generation
// ─────────────────────────────────────────────────────────────────────────────

/// Generate update_by_id_graph and update_by_id_graph_returning methods.
fn generate_update_graph_methods(
    attrs: &StructAttrs,
    id_col_expr: &TokenStream,
) -> Result<TokenStream> {
    let graph = &attrs.graph;

    // If no graph declarations, don't generate graph methods
    if !graph.has_any() {
        return Ok(quote! {});
    }

    let table_name = &attrs.table;

    // Generate child table handling code
    let has_many_code = generate_has_many_update_code(graph, table_name)?;
    let has_one_code = generate_has_one_update_code(graph, table_name)?;

    // Generate code to check if any child fields have values (Some(...))
    let mut check_children_stmts = Vec::new();
    for rel in &graph.has_many {
        let field_ident = format_ident!("{}", rel.field);
        check_children_stmts.push(quote! {
            if self.#field_ident.is_some() { __pgorm_has_child_ops = true; }
        });
    }
    for rel in &graph.has_one {
        let field_ident = format_ident!("{}", rel.field);
        check_children_stmts.push(quote! {
            if self.#field_ident.is_some() { __pgorm_has_child_ops = true; }
        });
    }
    let check_children_code = quote! { #(#check_children_stmts)* };

    // The graph methods need to handle (per doc §6.3):
    // 1. If root patch has main table fields and affected == 0: NotFound
    // 2. If root patch has no main table fields but children have changes: verify root exists first
    // 3. If nothing to do (no main fields, all children None): Validation error

    let update_by_id_graph_method = quote! {
        /// Update this struct and all related child tables by primary key.
        ///
        /// Child fields with `None` are not touched. `Some(vec)` triggers the configured strategy.
        ///
        /// Per doc §6.3:
        /// - If root has fields to update but affected == 0: returns NotFound
        /// - If root has no fields but children have changes: verifies root exists first
        /// - If nothing to do at all: returns Validation error
        pub async fn update_by_id_graph<I>(
            mut self,
            conn: &impl ::pgorm::GenericClient,
            id: I,
        ) -> ::pgorm::OrmResult<u64>
        where
            I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + ::core::clone::Clone + 'static,
        {
            let mut __pgorm_total_affected: u64 = 0;
            let __pgorm_id = id.clone();

            // Check if any child fields have operations
            let mut __pgorm_has_child_ops = false;
            #check_children_code

            // Try to update main table
            let __pgorm_main_update_result = self.update_by_id(conn, id).await;

            match __pgorm_main_update_result {
                ::std::result::Result::Ok(affected) => {
                    if affected == 0 {
                        // Main table had fields to update but no rows matched - NotFound
                        return ::std::result::Result::Err(::pgorm::OrmError::NotFound(
                            "update_by_id_graph: root row not found".to_string()
                        ));
                    }
                    __pgorm_total_affected += affected;
                }
                ::std::result::Result::Err(::pgorm::OrmError::Validation(msg)) if msg.contains("no fields to update") => {
                    // No main table fields to update
                    if !__pgorm_has_child_ops {
                        // Nothing to do at all - validation error
                        return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                            "WriteGraph: no operations to perform".to_string()
                        ));
                    }
                    // Verify root exists before touching children
                    let exists_sql = ::std::format!(
                        "SELECT 1 FROM {} WHERE {} = $1",
                        #table_name,
                        #id_col_expr
                    );
                    let exists_result = ::pgorm::query(exists_sql)
                        .bind(__pgorm_id.clone())
                        .fetch_opt(conn)
                        .await?;
                    if exists_result.is_none() {
                        return ::std::result::Result::Err(::pgorm::OrmError::NotFound(
                            "update_by_id_graph: root row not found".to_string()
                        ));
                    }
                }
                ::std::result::Result::Err(e) => return ::std::result::Result::Err(e),
            }

            // Process has_many child tables
            #has_many_code

            // Process has_one child tables
            #has_one_code

            ::std::result::Result::Ok(__pgorm_total_affected)
        }
    };

    let update_by_id_graph_returning_method = if let Some(returning_ty) = attrs.returning.as_ref() {
        quote! {
            /// Update this struct and all related child tables, returning the updated root row.
            ///
            /// Child fields with `None` are not touched. `Some(vec)` triggers the configured strategy.
            ///
            /// Per doc §6.3:
            /// - If root has fields to update but affected == 0: returns NotFound
            /// - If root has no fields but children have changes: verifies root exists first
            /// - If nothing to do at all: returns Validation error
            pub async fn update_by_id_graph_returning<I>(
                mut self,
                conn: &impl ::pgorm::GenericClient,
                id: I,
            ) -> ::pgorm::OrmResult<#returning_ty>
            where
                I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + ::core::clone::Clone + 'static,
                #returning_ty: ::pgorm::FromRow,
            {
                let mut __pgorm_total_affected: u64 = 0;
                let __pgorm_id = id.clone();
                let __pgorm_root_result: #returning_ty;

                // Check if any child fields have operations
                let mut __pgorm_has_child_ops = false;
                #check_children_code

                // Try to update main table
                let __pgorm_main_update_result = self.update_by_id_returning(conn, id).await;

                match __pgorm_main_update_result {
                    ::std::result::Result::Ok(result) => {
                        __pgorm_total_affected += 1;
                        __pgorm_root_result = result;
                    }
                    ::std::result::Result::Err(::pgorm::OrmError::Validation(msg)) if msg.contains("no fields to update") => {
                        // No main table fields to update
                        if !__pgorm_has_child_ops {
                            // Nothing to do at all - validation error
                            return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                                "WriteGraph: no operations to perform".to_string()
                            ));
                        }
                        // Fetch current row (also verifies it exists)
                        let sql = ::std::format!(
                            "SELECT {} FROM {} {} WHERE {}.{} = $1",
                            #returning_ty::SELECT_LIST,
                            #table_name,
                            #returning_ty::JOIN_CLAUSE,
                            #table_name,
                            #id_col_expr
                        );
                        __pgorm_root_result = ::pgorm::query(sql)
                            .bind(__pgorm_id.clone())
                            .fetch_one_as::<#returning_ty>(conn)
                            .await
                            .map_err(|e| match e {
                                ::pgorm::OrmError::NotFound(_) => ::pgorm::OrmError::NotFound(
                                    "update_by_id_graph: root row not found".to_string()
                                ),
                                other => other,
                            })?;
                    }
                    ::std::result::Result::Err(::pgorm::OrmError::NotFound(_)) => {
                        return ::std::result::Result::Err(::pgorm::OrmError::NotFound(
                            "update_by_id_graph: root row not found".to_string()
                        ));
                    }
                    ::std::result::Result::Err(e) => return ::std::result::Result::Err(e),
                }

                // Process has_many child tables
                #has_many_code

                // Process has_one child tables
                #has_one_code

                ::std::result::Result::Ok(__pgorm_root_result)
            }
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        #update_by_id_graph_method
        #update_by_id_graph_returning_method
    })
}

/// Generate code for has_many_update child tables.
/// Uses with_* setters to inject FK values (avoiding direct field access for cross-module compatibility).
fn generate_has_many_update_code(
    graph: &UpdateGraphDeclarations,
    _table_name: &str,
) -> Result<TokenStream> {
    if graph.has_many.is_empty() {
        return Ok(quote! {});
    }

    let mut code_blocks = Vec::new();

    for rel in &graph.has_many {
        let field_ident = format_ident!("{}", rel.field);
        let child_type = &rel.child_type;
        let fk_column = &rel.fk_column;

        // Use with_* setter to inject FK
        let setter_name = format_ident!("with_{}", rel.fk_field);

        let strategy_code = match rel.strategy {
            UpdateStrategy::Replace => {
                // Delete all old children, insert new ones
                quote! {
                    // Delete old children
                    let delete_sql = ::std::format!(
                        "DELETE FROM {} WHERE {} = $1",
                        #child_type::TABLE,
                        #fk_column
                    );
                    let deleted = ::pgorm::query(delete_sql).bind(__pgorm_id.clone()).execute(conn).await?;
                    __pgorm_total_affected += deleted;

                    // Insert new children
                    if !children.is_empty() {
                        let children_with_fk: ::std::vec::Vec<_> = children
                            .into_iter()
                            .map(|child| child.#setter_name(__pgorm_id.clone()))
                            .collect();
                        let inserted = #child_type::insert_many(conn, children_with_fk).await?;
                        __pgorm_total_affected += inserted;
                    }
                }
            }
            UpdateStrategy::Append => {
                // Only insert new children
                quote! {
                    if !children.is_empty() {
                        let children_with_fk: ::std::vec::Vec<_> = children
                            .into_iter()
                            .map(|child| child.#setter_name(__pgorm_id.clone()))
                            .collect();
                        let inserted = #child_type::insert_many(conn, children_with_fk).await?;
                        __pgorm_total_affected += inserted;
                    }
                }
            }
            UpdateStrategy::Upsert => {
                // Upsert children
                quote! {
                    if !children.is_empty() {
                        let children_with_fk: ::std::vec::Vec<_> = children
                            .into_iter()
                            .map(|child| child.#setter_name(__pgorm_id.clone()))
                            .collect();
                        let upserted = #child_type::upsert_many(conn, children_with_fk).await?;
                        __pgorm_total_affected += upserted;
                    }
                }
            }
            UpdateStrategy::Diff => {
                // Upsert + delete missing (uses __pgorm_diff_many_by_fk helper)
                let key_columns_vec: Vec<String> = rel.key_columns.as_ref().unwrap().clone();

                quote! {
                    // Use diff helper to upsert and delete missing children
                    let diff_affected = #child_type::__pgorm_diff_many_by_fk(
                        conn,
                        #fk_column,
                        __pgorm_id.clone(),
                        &[#(#key_columns_vec),*],
                        children
                            .into_iter()
                            .map(|child| child.#setter_name(__pgorm_id.clone()))
                            .collect(),
                    ).await?;
                    __pgorm_total_affected += diff_affected;
                }
            }
        };

        let code = quote! {
            // has_many_update: #field_ident
            if let ::std::option::Option::Some(children) = self.#field_ident {
                #strategy_code
            }
        };

        code_blocks.push(code);
    }

    Ok(quote! {
        #(#code_blocks)*
    })
}

/// Generate code for has_one_update child tables.
/// Uses with_* setters to inject FK values (avoiding direct field access for cross-module compatibility).
fn generate_has_one_update_code(
    graph: &UpdateGraphDeclarations,
    _table_name: &str,
) -> Result<TokenStream> {
    if graph.has_one.is_empty() {
        return Ok(quote! {});
    }

    let mut code_blocks = Vec::new();

    for rel in &graph.has_one {
        let field_ident = format_ident!("{}", rel.field);
        let child_type = &rel.child_type;
        let fk_column = &rel.fk_column;

        // Use with_* setter to inject FK
        let setter_name = format_ident!("with_{}", rel.fk_field);

        let strategy_code = match rel.strategy {
            UpdateStrategy::Replace => {
                quote! {
                    // Delete old child
                    let delete_sql = ::std::format!(
                        "DELETE FROM {} WHERE {} = $1",
                        #child_type::TABLE,
                        #fk_column
                    );
                    let deleted = ::pgorm::query(delete_sql).bind(__pgorm_id.clone()).execute(conn).await?;
                    __pgorm_total_affected += deleted;

                    // Insert new child if present
                    if let ::std::option::Option::Some(child) = inner_value {
                        let child_with_fk = child.#setter_name(__pgorm_id.clone());
                        let inserted = child_with_fk.insert(conn).await?;
                        __pgorm_total_affected += inserted;
                    }
                }
            }
            UpdateStrategy::Upsert => {
                // Per doc §6.2.2: Some(None) => DELETE, Some(Some(child)) => upsert
                quote! {
                    match inner_value {
                        ::std::option::Option::None => {
                            // Delete the child
                            let delete_sql = ::std::format!(
                                "DELETE FROM {} WHERE {} = $1",
                                #child_type::TABLE,
                                #fk_column
                            );
                            let deleted = ::pgorm::query(delete_sql).bind(__pgorm_id.clone()).execute(conn).await?;
                            __pgorm_total_affected += deleted;
                        }
                        ::std::option::Option::Some(child) => {
                            let child_with_fk = child.#setter_name(__pgorm_id.clone());
                            let upserted = child_with_fk.upsert(conn).await?;
                            __pgorm_total_affected += upserted;
                        }
                    }
                }
            }
            _ => {
                // Other strategies not applicable for has_one
                quote! {}
            }
        };

        // has_one_update uses Option<Option<Child>>:
        // - None: don't touch
        // - Some(None): delete child
        // - Some(Some(child)): replace/upsert
        let code = quote! {
            // has_one_update: #field_ident
            if let ::std::option::Option::Some(inner_value) = self.#field_ident {
                #strategy_code
            }
        };

        code_blocks.push(code);
    }

    Ok(quote! {
        #(#code_blocks)*
    })
}
