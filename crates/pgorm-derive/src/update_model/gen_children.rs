//! Children update code generation (has_many/has_one).
//!
//! This module contains code generation for has_many_update and has_one_update handlers.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Result;

use super::graph_decl::{UpdateGraphDeclarations, UpdateStrategy};

/// Generate code for has_many_update child tables.
/// Uses with_* setters to inject FK values (avoiding direct field access for cross-module compatibility).
pub(super) fn generate_has_many_update_code(
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
pub(super) fn generate_has_one_update_code(
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
