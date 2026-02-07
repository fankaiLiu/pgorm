//! Graph update methods code generation.
//!
//! This module contains code generation for:
//! - `update_by_id_graph` method
//! - `update_by_id_graph_returning` method

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Result;

use super::attrs::StructAttrs;
use super::gen_children::{generate_has_many_update_code, generate_has_one_update_code};

/// Generate update_by_id_graph and update_by_id_graph_returning methods.
pub(super) fn generate_update_graph_methods(
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
            I: ::pgorm::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + ::core::clone::Clone + 'static,
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
                I: ::pgorm::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + ::core::clone::Clone + 'static,
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
