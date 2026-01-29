//! Graph insert methods code generation.
//!
//! This module contains code generation for:
//! - `insert_graph` method
//! - `insert_graph_returning` method
//! - `insert_graph_report` method
//! - Helper functions for belongs_to, has_one/has_many, and insert_step operations

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{DeriveInput, Result};

use super::attrs::StructAttrs;
use super::graph_decl::{BelongsToMode, GraphDeclarations, HasRelationMode, StepMode};
use crate::common::syn_types::{option_inner, vec_inner};

/// Info needed for generating inline INSERT SQL in graph methods.
pub(super) struct InsertSqlInfo {
    /// The INSERT SQL string (without RETURNING).
    pub(super) sql: String,
    /// Field identifiers that need to be bound to the SQL.
    pub(super) bind_idents: Vec<syn::Ident>,
}

/// Generate insert_graph, insert_graph_returning, and insert_graph_report methods.
pub(super) fn generate_insert_graph_methods(
    struct_attrs: &StructAttrs,
    input: &DeriveInput,
    fields: &syn::punctuated::Punctuated<syn::Field, syn::Token![,]>,
    insert_sql_info: &InsertSqlInfo,
) -> Result<TokenStream> {
    let graph = &struct_attrs.graph;

    // If no graph declarations, don't generate graph methods
    if !graph.has_any() {
        return Ok(quote! {});
    }

    // Compile-time check: has_one/has_many without graph_root_id_field requires returning type
    // because we need the root ID (via ModelPk::pk()) to inject into children.
    if !graph.has_relations.is_empty() && graph.graph_root_id_field.is_none() {
        if struct_attrs.returning.is_none() {
            let relation_names: Vec<_> = graph.has_relations.iter().map(|r| &r.field).collect();
            return Err(syn::Error::new_spanned(
                input,
                format!(
                    "InsertModel with has_one/has_many relations ({:?}) requires either: \
                    \n  1. #[orm(returning = \"YourModel\")] where YourModel implements ModelPk, or \
                    \n  2. #[orm(graph_root_id_field = \"id\")] to get root_id from input field. \
                    \nThe root ID is needed to set foreign keys on child records.",
                    relation_names,
                ),
            ));
        }
    }

    let table_name = &struct_attrs.table;

    // Collect all field idents that need to be extracted from self
    let mut all_field_idents: Vec<syn::Ident> = Vec::new();
    for field in fields.iter() {
        if let Some(ident) = &field.ident {
            all_field_idents.push(ident.clone());
        }
    }

    // Placeholder ident for root_key (not actually used now)
    let root_key_ident = format_ident!("id");

    // Generate code for belongs_to (pre-insert) steps
    let belongs_to_code = generate_belongs_to_code(graph, &root_key_ident)?;

    // Generate code for before_insert steps
    let before_insert_code = generate_insert_step_code(graph, true)?;

    // Generate code to extract graph fields before consuming self
    let (extract_graph_fields_code, direct_t_fields) =
        generate_extract_graph_fields_code(graph, fields)?;

    // Check if we have direct T fields (non-Option, non-Vec)
    // If so, we need to use inline SQL instead of self.insert() because we can't call .take()
    let has_direct_t_fields = !direct_t_fields.is_empty();

    // Generate code for direct T field extraction (if any)
    // For direct T fields, we need to destructure self and extract them before the SQL insert
    let direct_t_extract_code = if has_direct_t_fields {
        let mut stmts = Vec::new();
        for field_name in &direct_t_fields {
            let field_ident = format_ident!("{}", field_name);
            let extracted_ident = format_ident!("__pgorm_graph_{}", field_name);
            // Find if it's a has_relation or insert_step
            let is_has_relation = graph.has_relations.iter().any(|r| r.field == *field_name);
            if is_has_relation {
                stmts.push(quote! {
                    let #extracted_ident = ::std::option::Option::Some(#field_ident);
                });
            } else {
                let extracted_step_ident = format_ident!("__pgorm_step_{}", field_name);
                stmts.push(quote! {
                    let #extracted_step_ident = ::std::option::Option::Some(#field_ident);
                });
            }
        }
        quote! { #(#stmts)* }
    } else {
        quote! {}
    };

    // Generate code for has_one/has_many (post-insert) steps
    let has_relation_code = generate_has_relation_code(graph, &root_key_ident)?;

    // Generate code for after_insert steps
    let after_insert_code = generate_insert_step_code(graph, false)?;

    // Build tag for root insert
    let root_tag = format!("graph:root:{}", table_name);

    // Check if we need returning type
    let returning_ty = struct_attrs.returning.as_ref();

    // Determine root ID source:
    // 1. If graph_root_id_field is set, use Input mode (get from self.<field>)
    // 2. Otherwise use Returning mode (get from ModelPk::pk() on returning type)
    let has_graph_root_id_field = graph.graph_root_id_field.is_some();
    let needs_root_id = !graph.has_relations.is_empty();

    // Generate inline SQL execution code for when we have direct T fields
    // This replaces the call to self.insert() / self.insert_returning()
    let insert_sql = &insert_sql_info.sql;
    let bind_idents = &insert_sql_info.bind_idents;

    let _inline_insert_query = bind_idents.iter().fold(
        quote! { ::pgorm::query(#insert_sql) },
        |acc, ident| quote! { #acc.bind(&#ident) },
    );

    // Generate destructure pattern for direct T fields mode
    let full_destructure = quote! {
        let Self { #(#all_field_idents),* } = self;
    };

    // The pre-insert execution logic depends on whether we have direct T fields
    let pre_insert_code = if has_direct_t_fields {
        // Direct T mode: destructure self first, then extract graph fields into Option wrappers
        quote! {
            let mut __pgorm_steps: ::std::vec::Vec<::pgorm::WriteStepReport> = ::std::vec::Vec::new();
            let mut __pgorm_total_affected: u64 = 0;

            // Destructure self into field variables (allows moving direct T fields)
            #full_destructure

            // Extract graph fields for direct T types (wrap in Some)
            #direct_t_extract_code

            // Extract graph fields for Option/Vec types - these are now local variables
            // (The extract_graph_fields_code uses self.field, but we've destructured into local vars)
            // We need to generate different code for direct T mode
        }
    } else {
        // Normal mode: use &mut self and .take() methods
        quote! {
            let mut __pgorm_steps: ::std::vec::Vec<::pgorm::WriteStepReport> = ::std::vec::Vec::new();
            let mut __pgorm_total_affected: u64 = 0;

            // 0. Extract graph fields before consuming self
            #extract_graph_fields_code

            // 1. belongs_to (pre-insert): write parent tables and get their IDs
            #belongs_to_code

            // 2. before_insert steps
            #before_insert_code
        }
    };

    // For direct T mode, we need to regenerate graph field extraction to use local variables
    // Note: These are currently unused but kept for future full direct T support (option B)
    let (_direct_t_mode_extract_code, _direct_t_mode_belongs_to, _direct_t_mode_before_insert) =
        if has_direct_t_fields {
            // Generate extraction code that uses local variables (not self.field)
            let mut extract_stmts = Vec::new();
            for rel in &graph.has_relations {
                let field_ident = format_ident!("{}", rel.field);
                let extracted_field_ident = format_ident!("__pgorm_graph_{}", rel.field);

                // Skip direct T fields - they're already extracted above
                if direct_t_fields.contains(&rel.field) {
                    continue;
                }

                // Find field type
                let field_ty = fields
                    .iter()
                    .find(|f| f.ident.as_ref().map(|i| i.to_string()) == Some(rel.field.clone()))
                    .map(|f| &f.ty);

                if let Some(ty) = field_ty {
                    if option_inner(ty).is_some() {
                        extract_stmts.push(quote! {
                            let #extracted_field_ident = #field_ident; // Already Option<T>
                        });
                    } else if vec_inner(ty).is_some() {
                        extract_stmts.push(quote! {
                            let #extracted_field_ident = ::std::option::Option::Some(#field_ident);
                        });
                    }
                }
            }

            // For after_insert steps
            for step in &graph.insert_steps {
                if !step.is_before {
                    let field_ident = format_ident!("{}", step.field);
                    let extracted_field_ident = format_ident!("__pgorm_step_{}", step.field);

                    if direct_t_fields.contains(&step.field) {
                        continue;
                    }

                    let field_ty = fields
                        .iter()
                        .find(|f| {
                            f.ident.as_ref().map(|i| i.to_string()) == Some(step.field.clone())
                        })
                        .map(|f| &f.ty);

                    if let Some(ty) = field_ty {
                        if option_inner(ty).is_some() {
                            extract_stmts.push(quote! {
                                let #extracted_field_ident = #field_ident;
                            });
                        } else if vec_inner(ty).is_some() {
                            extract_stmts.push(quote! {
                            let #extracted_field_ident = ::std::option::Option::Some(#field_ident);
                        });
                        }
                    }
                }
            }

            let extract_code = quote! { #(#extract_stmts)* };

            // Generate belongs_to code that uses local variables
            let mut bt_blocks = Vec::new();
            for bt in &graph.belongs_to {
                let field_ident = format_ident!("{}", bt.field);
                let set_fk_field_ident = format_ident!("{}", bt.set_fk_field);
                let tag = format!("graph:belongs_to:{}", bt.field);
                let field_name = &bt.field;
                let set_fk_field_name = &bt.set_fk_field;

                let insert_call = match bt.mode {
                    BelongsToMode::InsertReturning => quote! { insert_returning(conn).await? },
                    BelongsToMode::UpsertReturning => quote! { upsert_returning(conn).await? },
                };

                // For direct T mode, we need to use mutable local variables
                // Since belongs_to modifies set_fk_field, we need it mutable
                let code = if bt.required {
                    quote! {
                        let __fk_has_value = #set_fk_field_ident.is_some();
                        let __field_has_value = #field_ident.is_some();

                        if __fk_has_value && __field_has_value {
                            return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                                ::std::format!(
                                    "belongs_to '{}': '{}' and '{}' are mutually exclusive but both are set",
                                    #field_name, #set_fk_field_name, #field_name
                                )
                            ));
                        } else if __fk_has_value {
                            // FK already set, skip
                        } else if let ::std::option::Option::Some(parent_data) = #field_ident {
                            let parent_result = parent_data.#insert_call;
                            let parent_id = ::pgorm::ModelPk::pk(&parent_result).clone();
                            #set_fk_field_ident = ::std::option::Option::Some(parent_id);
                            __pgorm_steps.push(::pgorm::WriteStepReport { tag: #tag, affected: 1 });
                            __pgorm_total_affected += 1;
                        } else {
                            return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                                ::std::format!(
                                    "belongs_to '{}' is required but neither '{}' nor '{}' is set",
                                    #field_name, #set_fk_field_name, #field_name
                                )
                            ));
                        }
                    }
                } else {
                    quote! {
                        let __fk_has_value = #set_fk_field_ident.is_some();
                        let __field_has_value = #field_ident.is_some();

                        if __fk_has_value && __field_has_value {
                            return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                                ::std::format!(
                                    "belongs_to '{}': '{}' and '{}' are mutually exclusive but both are set",
                                    #field_name, #set_fk_field_name, #field_name
                                )
                            ));
                        } else if !__fk_has_value {
                            if let ::std::option::Option::Some(parent_data) = #field_ident {
                                let parent_result = parent_data.#insert_call;
                                let parent_id = ::pgorm::ModelPk::pk(&parent_result).clone();
                                #set_fk_field_ident = ::std::option::Option::Some(parent_id);
                                __pgorm_steps.push(::pgorm::WriteStepReport { tag: #tag, affected: 1 });
                                __pgorm_total_affected += 1;
                            }
                        }
                    }
                };
                bt_blocks.push(code);
            }
            let bt_code = quote! { #(#bt_blocks)* };

            // Generate before_insert code that uses local variables
            let mut bi_blocks = Vec::new();
            for step in graph.insert_steps.iter().filter(|s| s.is_before) {
                let field_ident = format_ident!("{}", step.field);
                let tag = format!("graph:before_insert:{}", step.field);

                let insert_call = match step.mode {
                    StepMode::Insert => quote! {
                        let affected = step_data.insert(conn).await?;
                        __pgorm_steps.push(::pgorm::WriteStepReport { tag: #tag, affected });
                        __pgorm_total_affected += affected;
                    },
                    StepMode::Upsert => quote! {
                        let affected = step_data.upsert(conn).await?;
                        __pgorm_steps.push(::pgorm::WriteStepReport { tag: #tag, affected });
                        __pgorm_total_affected += affected;
                    },
                };

                let field_ty = fields
                    .iter()
                    .find(|f| f.ident.as_ref().map(|i| i.to_string()) == Some(step.field.clone()))
                    .map(|f| &f.ty);

                let code = if let Some(ty) = field_ty {
                    if option_inner(ty).is_some() {
                        quote! {
                            if let ::std::option::Option::Some(step_data) = #field_ident {
                                #insert_call
                            }
                        }
                    } else if vec_inner(ty).is_some() {
                        quote! {
                            for step_data in #field_ident {
                                #insert_call
                            }
                        }
                    } else {
                        // Direct T - just use it
                        quote! {
                            let step_data = #field_ident;
                            #insert_call
                        }
                    }
                } else {
                    quote! {
                        if let ::std::option::Option::Some(step_data) = #field_ident {
                            #insert_call
                        }
                    }
                };
                bi_blocks.push(code);
            }
            let bi_code = quote! { #(#bi_blocks)* };

            (extract_code, bt_code, bi_code)
        } else {
            (quote! {}, quote! {}, quote! {})
        };

    // Generate code to get root_id based on the source mode
    // For graph_root_id_field, we handle both T and Option<T> field types at macro expansion time
    let (get_root_id_code, needs_model_pk_bound, _extract_helper) = if let Some(ref root_id_field) =
        graph.graph_root_id_field
    {
        // Input mode: get root_id from self.<field> before insert
        // Field can be T or Option<T>. Detect at macro time.
        let root_id_field_ident = format_ident!("{}", root_id_field);
        let root_id_field_name = root_id_field.clone();

        // Find the field type
        let field_ty = fields
            .iter()
            .find(|f| f.ident.as_ref().map(|i| i.to_string()) == Some(root_id_field.clone()))
            .map(|f| &f.ty);

        let (code, helper) = if let Some(ty) = field_ty {
            if option_inner(ty).is_some() {
                // Field is Option<T>: need to unwrap and return error if None
                let code = quote! {
                    // Get root_id from input field (graph_root_id_field mode, Option<T>)
                    let __pgorm_root_id = self.#root_id_field_ident.clone().ok_or_else(|| {
                        ::pgorm::OrmError::Validation(
                            ::std::format!("graph_root_id_field '{}' is None but required for has_* relations", #root_id_field_name)
                        )
                    })?;
                };
                (code, quote! {})
            } else {
                // Field is T: just clone it
                let code = quote! {
                    // Get root_id from input field (graph_root_id_field mode, T)
                    let __pgorm_root_id = self.#root_id_field_ident.clone();
                };
                (code, quote! {})
            }
        } else {
            // Field not found - this will cause a compile error anyway
            let code = quote! {
                let __pgorm_root_id = self.#root_id_field_ident.clone();
            };
            (code, quote! {})
        };
        (code, false, helper)
    } else if needs_root_id {
        // Returning mode: get root_id from ModelPk::pk() after insert_returning
        // This requires returning type to implement ModelPk
        (quote! {}, true, quote! {}) // root_id will be extracted after insert_returning
    } else {
        // No root_id needed (no has_relations)
        (quote! {}, false, quote! {})
    };

    // Generate the three methods with proper handling of root_id source
    let insert_graph_method = if has_graph_root_id_field {
        // Input mode: we can get root_id before insert, so we don't need returning for has_*
        let returning_handling = if let Some(ret_ty) = returning_ty {
            quote! {
                // Root insert with returning
                let __pgorm_root_result: #ret_ty = self.insert_returning(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: 1,
                });
                __pgorm_total_affected += 1;
            }
        } else {
            quote! {
                // Root insert (without returning, but we have root_id from input)
                let __pgorm_root_affected = self.insert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: __pgorm_root_affected,
                });
                __pgorm_total_affected += __pgorm_root_affected;
            }
        };

        quote! {
            /// Insert this struct and all related graph nodes.
            ///
            /// Execution order: belongs_to → before_insert → root → has_one/has_many → after_insert
            pub async fn insert_graph(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<u64> {
                #pre_insert_code

                // Get root_id from input field before insert
                #get_root_id_code

                #returning_handling

                // Execute post-insert steps with root ID
                #has_relation_code
                #after_insert_code

                ::std::result::Result::Ok(__pgorm_total_affected)
            }
        }
    } else if let Some(ret_ty) = returning_ty {
        // Returning mode with returning type configured
        let where_clause = if needs_model_pk_bound {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow + ::pgorm::ModelPk,
            }
        } else {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow,
            }
        };

        let root_id_extraction = if needs_root_id {
            quote! {
                let __pgorm_root_id = ::pgorm::ModelPk::pk(&__pgorm_root_result).clone();
            }
        } else {
            quote! {}
        };

        quote! {
            /// Insert this struct and all related graph nodes.
            ///
            /// Execution order: belongs_to → before_insert → root → has_one/has_many → after_insert
            pub async fn insert_graph(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<u64>
            #where_clause
            {
                #pre_insert_code

                // Root insert with returning to get the ID
                let __pgorm_root_result: #ret_ty = self.insert_returning(conn).await?;
                #root_id_extraction
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: 1,
                });
                __pgorm_total_affected += 1;

                // Execute post-insert steps with root ID
                #has_relation_code
                #after_insert_code

                ::std::result::Result::Ok(__pgorm_total_affected)
            }
        }
    } else {
        // No returning type - graph operations with has_one/has_many won't work
        // This case is handled by task #3 (compile error for has_* without returning)
        quote! {
            /// Insert this struct and all related graph nodes.
            ///
            /// Note: has_one/has_many relations require either `#[orm(returning = "...")]`
            /// or `#[orm(graph_root_id_field = "...")]` to be set.
            pub async fn insert_graph(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<u64> {
                #pre_insert_code

                // Root insert
                let __pgorm_root_affected = self.insert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: __pgorm_root_affected,
                });
                __pgorm_total_affected += __pgorm_root_affected;

                ::std::result::Result::Ok(__pgorm_total_affected)
            }
        }
    };

    let insert_graph_returning_method = if let Some(ret_ty) = returning_ty {
        let where_clause = if needs_model_pk_bound && !has_graph_root_id_field {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow + ::pgorm::ModelPk,
            }
        } else {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow,
            }
        };

        // Code to run before insert (for graph_root_id_field mode)
        let pre_insert_root_id = if has_graph_root_id_field {
            quote! {
                #get_root_id_code
            }
        } else {
            quote! {}
        };

        // Code to run after insert (for returning mode)
        let post_insert_root_id = if !has_graph_root_id_field && needs_root_id {
            quote! {
                let __pgorm_root_id = ::pgorm::ModelPk::pk(&__pgorm_root_result).clone();
            }
        } else {
            quote! {}
        };

        quote! {
            /// Insert this struct and all related graph nodes, returning the root row.
            ///
            /// Execution order: belongs_to → before_insert → root → has_one/has_many → after_insert
            pub async fn insert_graph_returning(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<#ret_ty>
            #where_clause
            {
                #pre_insert_code

                // Get root_id from input field before insert (if graph_root_id_field mode)
                #pre_insert_root_id

                // Root insert with returning
                let __pgorm_root_result: #ret_ty = self.insert_returning(conn).await?;

                // Get root_id from returning result (if not graph_root_id_field mode)
                #post_insert_root_id

                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: 1,
                });
                __pgorm_total_affected += 1;

                // Execute post-insert steps with root ID
                #has_relation_code
                #after_insert_code

                ::std::result::Result::Ok(__pgorm_root_result)
            }
        }
    } else {
        quote! {}
    };

    let insert_graph_report_method = if let Some(ret_ty) = returning_ty {
        let where_clause = if needs_model_pk_bound && !has_graph_root_id_field {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow + ::pgorm::ModelPk,
            }
        } else {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow,
            }
        };

        // Code to run before insert (for graph_root_id_field mode)
        let pre_insert_root_id = if has_graph_root_id_field {
            quote! {
                #get_root_id_code
            }
        } else {
            quote! {}
        };

        // Code to run after insert (for returning mode)
        let post_insert_root_id = if !has_graph_root_id_field && needs_root_id {
            quote! {
                let __pgorm_root_id = ::pgorm::ModelPk::pk(&__pgorm_root_result).clone();
            }
        } else {
            quote! {}
        };

        quote! {
            /// Insert this struct and all related graph nodes, returning a detailed report.
            ///
            /// Execution order: belongs_to → before_insert → root → has_one/has_many → after_insert
            pub async fn insert_graph_report(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<::pgorm::WriteReport<#ret_ty>>
            #where_clause
            {
                #pre_insert_code

                // Get root_id from input field before insert (if graph_root_id_field mode)
                #pre_insert_root_id

                // Root insert with returning
                let __pgorm_root_result: #ret_ty = self.insert_returning(conn).await?;

                // Get root_id from returning result (if not graph_root_id_field mode)
                #post_insert_root_id

                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: 1,
                });
                __pgorm_total_affected += 1;

                // Execute post-insert steps with root ID
                #has_relation_code
                #after_insert_code

                ::std::result::Result::Ok(::pgorm::WriteReport {
                    affected: __pgorm_total_affected,
                    steps: __pgorm_steps,
                    root: ::std::option::Option::Some(__pgorm_root_result),
                })
            }
        }
    } else if has_graph_root_id_field {
        // No returning type but we have graph_root_id_field, so we can still do has_* operations
        quote! {
            /// Insert this struct and all related graph nodes, returning a detailed report.
            ///
            /// Returns `WriteReport<()>` with `root: None` since no returning type is configured.
            pub async fn insert_graph_report(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<::pgorm::WriteReport<()>> {
                #pre_insert_code

                // Get root_id from input field before insert
                #get_root_id_code

                // Root insert (without returning)
                let __pgorm_root_affected = self.insert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: __pgorm_root_affected,
                });
                __pgorm_total_affected += __pgorm_root_affected;

                // Execute post-insert steps with root ID
                #has_relation_code
                #after_insert_code

                ::std::result::Result::Ok(::pgorm::WriteReport {
                    affected: __pgorm_total_affected,
                    steps: __pgorm_steps,
                    root: ::std::option::Option::None,
                })
            }
        }
    } else {
        // When no returning type is configured, return WriteReport<()>
        // Note: has_one/has_many won't work in this case
        quote! {
            /// Insert this struct and all related graph nodes, returning a detailed report.
            ///
            /// Note: has_one/has_many relations require `#[orm(returning = "...")]` or
            /// `#[orm(graph_root_id_field = "...")]` to be set.
            /// Without either, this method returns `WriteReport<()>` with `root: None`.
            pub async fn insert_graph_report(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<::pgorm::WriteReport<()>> {
                #pre_insert_code

                // Root insert (without returning)
                let __pgorm_root_affected = self.insert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: __pgorm_root_affected,
                });
                __pgorm_total_affected += __pgorm_root_affected;

                ::std::result::Result::Ok(::pgorm::WriteReport {
                    affected: __pgorm_total_affected,
                    steps: __pgorm_steps,
                    root: ::std::option::Option::None,
                })
            }
        }
    };

    Ok(quote! {
        #insert_graph_method
        #insert_graph_returning_method
        #insert_graph_report_method
    })
}

/// Generate code to extract graph fields (has_one/has_many and after_insert) before consuming self.
///
/// For has_one/has_many fields, we support `Option<T>`, `Vec<T>`, and direct `T` types:
/// - `Option<T>`: uses `.take()` to extract
/// - `Vec<T>`: uses `std::mem::take()` to extract, wrap in Some
/// - `T`: requires special handling - see below
///
/// For direct T fields (non-Option, non-Vec), we can't use .take() because there's no
/// "empty" state. According to option B, we restructure to destructure self into field
/// variables first, then execute SQL directly. This is handled by the calling function.
///
/// This function generates extraction code for Option<T> and Vec<T> fields only.
/// Direct T fields are signaled back to the caller via the returned list.
fn generate_extract_graph_fields_code(
    graph: &GraphDeclarations,
    fields: &syn::punctuated::Punctuated<syn::Field, syn::Token![,]>,
) -> Result<(TokenStream, Vec<String>)> {
    let mut extract_stmts = Vec::new();
    let mut direct_t_fields: Vec<String> = Vec::new();

    // Helper to find field type by name
    let find_field_type = |field_name: &str| -> Option<&syn::Type> {
        fields
            .iter()
            .find(|f| f.ident.as_ref().map(|i| i.to_string()) == Some(field_name.to_string()))
            .map(|f| &f.ty)
    };

    // Extract has_relations fields
    for rel in &graph.has_relations {
        let field_ident = format_ident!("{}", rel.field);
        let extracted_field_ident = format_ident!("__pgorm_graph_{}", rel.field);

        // Detect field type at macro time
        if let Some(ty) = find_field_type(&rel.field) {
            if option_inner(ty).is_some() {
                // Option<T>: use take()
                extract_stmts.push(quote! {
                    let #extracted_field_ident = self.#field_ident.take();
                });
            } else if vec_inner(ty).is_some() {
                // Vec<T>: use std::mem::take() and wrap in Some
                extract_stmts.push(quote! {
                    let #extracted_field_ident = ::std::option::Option::Some(::std::mem::take(&mut self.#field_ident));
                });
            } else {
                // Direct T: signal that this needs special handling
                direct_t_fields.push(rel.field.clone());
            }
        } else {
            // Field not found - will fail at compile time
            direct_t_fields.push(rel.field.clone());
        }
    }

    // Extract after_insert step fields
    for step in &graph.insert_steps {
        if !step.is_before {
            let field_ident = format_ident!("{}", step.field);
            let extracted_field_ident = format_ident!("__pgorm_step_{}", step.field);

            if let Some(ty) = find_field_type(&step.field) {
                if option_inner(ty).is_some() {
                    // Option<T>: use take()
                    extract_stmts.push(quote! {
                        let #extracted_field_ident = self.#field_ident.take();
                    });
                } else if vec_inner(ty).is_some() {
                    // Vec<T>: use std::mem::take() and wrap in Some
                    extract_stmts.push(quote! {
                        let #extracted_field_ident = ::std::option::Option::Some(::std::mem::take(&mut self.#field_ident));
                    });
                } else {
                    // Direct T: signal that this needs special handling
                    direct_t_fields.push(step.field.clone());
                }
            } else {
                direct_t_fields.push(step.field.clone());
            }
        }
    }

    // Check if we need the helper trait (for before_insert steps)
    let has_before_insert = graph.insert_steps.iter().any(|s| s.is_before);

    if extract_stmts.is_empty() && !has_before_insert {
        return Ok((quote! {}, direct_t_fields));
    }

    // Generate the helper trait only if needed for before_insert
    let helper_code = if has_before_insert {
        quote! {
            // Helper trait to extract graph fields uniformly for various types
            trait __PgormTakeGraphField<T> {
                fn __pgorm_take(&mut self) -> ::std::option::Option<T>;
            }

            // Implementation for Option<T>: use take()
            impl<T> __PgormTakeGraphField<T> for ::std::option::Option<T> {
                fn __pgorm_take(&mut self) -> ::std::option::Option<T> {
                    self.take()
                }
            }

            // Implementation for Vec<T>: take ownership by replacing with empty vec, wrap in Some
            impl<T> __PgormTakeGraphField<::std::vec::Vec<T>> for ::std::vec::Vec<T> {
                fn __pgorm_take(&mut self) -> ::std::option::Option<::std::vec::Vec<T>> {
                    ::std::option::Option::Some(::std::mem::take(self))
                }
            }

            fn __pgorm_take_graph_field<T, F: __PgormTakeGraphField<T>>(field: &mut F) -> ::std::option::Option<T> {
                field.__pgorm_take()
            }
        }
    } else {
        quote! {}
    };

    Ok((
        quote! {
            #helper_code
            #(#extract_stmts)*
        },
        direct_t_fields,
    ))
}

/// Generate code for belongs_to (pre-insert) steps.
/// Uses ModelPk::pk() to get parent ID (avoiding direct field access for cross-module compatibility).
///
/// Semantics (per doc §6.1.2):
/// - set_fk_field and field are mutually exclusive (both set => Validation error)
/// - If set_fk_field already has a value (Some), skip belongs_to write
/// - If required=true but both set_fk_field is None and field is None, return Validation error
fn generate_belongs_to_code(
    graph: &GraphDeclarations,
    _root_key_ident: &syn::Ident,
) -> Result<TokenStream> {
    if graph.belongs_to.is_empty() {
        return Ok(quote! {});
    }

    let mut code_blocks = Vec::new();

    for bt in &graph.belongs_to {
        let field_ident = format_ident!("{}", bt.field);
        let set_fk_field_ident = format_ident!("{}", bt.set_fk_field);
        let _parent_type = &bt.parent_type;
        let tag = format!("graph:belongs_to:{}", bt.field);
        let field_name = &bt.field;
        let set_fk_field_name = &bt.set_fk_field;

        let insert_call = match bt.mode {
            BelongsToMode::InsertReturning => quote! { insert_returning(conn).await? },
            BelongsToMode::UpsertReturning => quote! { upsert_returning(conn).await? },
        };

        let code = if bt.required {
            // Required: must have either fk field set or parent field set (but not both)
            quote! {
                // belongs_to: #field_ident (required)
                // Check mutual exclusion: set_fk_field and field cannot both have values
                let __fk_has_value = self.#set_fk_field_ident.is_some();
                let __field_has_value = self.#field_ident.is_some();

                if __fk_has_value && __field_has_value {
                    return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                        ::std::format!(
                            "belongs_to '{}': '{}' and '{}' are mutually exclusive but both are set",
                            #field_name,
                            #set_fk_field_name,
                            #field_name
                        )
                    ));
                } else if __fk_has_value {
                    // FK already set, skip belongs_to write
                } else if let ::std::option::Option::Some(parent_data) = self.#field_ident.take() {
                    let parent_result = parent_data.#insert_call;
                    let parent_id = ::pgorm::ModelPk::pk(&parent_result).clone();
                    self.#set_fk_field_ident = ::std::option::Option::Some(parent_id);
                    __pgorm_steps.push(::pgorm::WriteStepReport {
                        tag: #tag,
                        affected: 1,
                    });
                    __pgorm_total_affected += 1;
                } else {
                    return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                        ::std::format!(
                            "belongs_to '{}' is required but neither '{}' nor '{}' is set",
                            #field_name,
                            #set_fk_field_name,
                            #field_name
                        )
                    ));
                }
            }
        } else {
            // Optional: if set_fk_field already has value, skip. Otherwise process parent if present.
            // Still check mutual exclusion
            quote! {
                // belongs_to: #field_ident (optional)
                // Check mutual exclusion: set_fk_field and field cannot both have values
                let __fk_has_value = self.#set_fk_field_ident.is_some();
                let __field_has_value = self.#field_ident.is_some();

                if __fk_has_value && __field_has_value {
                    return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                        ::std::format!(
                            "belongs_to '{}': '{}' and '{}' are mutually exclusive but both are set",
                            #field_name,
                            #set_fk_field_name,
                            #field_name
                        )
                    ));
                } else if !__fk_has_value {
                    if let ::std::option::Option::Some(parent_data) = self.#field_ident.take() {
                        let parent_result = parent_data.#insert_call;
                        let parent_id = ::pgorm::ModelPk::pk(&parent_result).clone();
                        self.#set_fk_field_ident = ::std::option::Option::Some(parent_id);
                        __pgorm_steps.push(::pgorm::WriteStepReport {
                            tag: #tag,
                            affected: 1,
                        });
                        __pgorm_total_affected += 1;
                    }
                }
            }
        };

        code_blocks.push(code);
    }

    Ok(quote! {
        #(#code_blocks)*
    })
}

/// Generate code for before_insert/after_insert steps.
fn generate_insert_step_code(graph: &GraphDeclarations, is_before: bool) -> Result<TokenStream> {
    let steps: Vec<_> = graph
        .insert_steps
        .iter()
        .filter(|s| s.is_before == is_before)
        .collect();

    if steps.is_empty() {
        return Ok(quote! {});
    }

    let mut code_blocks = Vec::new();

    for step in steps {
        let field_ident = format_ident!("{}", step.field);
        let _step_type = &step.step_type;
        let tag = format!(
            "graph:{}:{}",
            if is_before {
                "before_insert"
            } else {
                "after_insert"
            },
            step.field
        );

        let insert_call = match step.mode {
            StepMode::Insert => quote! {
                let affected = step_data.insert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #tag,
                    affected,
                });
                __pgorm_total_affected += affected;
            },
            StepMode::Upsert => quote! {
                let affected = step_data.upsert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #tag,
                    affected,
                });
                __pgorm_total_affected += affected;
            },
        };

        // For before_insert, use helper to extract (supports Option<T>, Vec<T>, etc.)
        // For after_insert, use the pre-extracted variable
        let code = if is_before {
            quote! {
                // before_insert step: #field_ident
                if let ::std::option::Option::Some(step_data) = __pgorm_take_graph_field(&mut self.#field_ident) {
                    #insert_call
                }
            }
        } else {
            let extracted_field_ident = format_ident!("__pgorm_step_{}", step.field);
            quote! {
                // after_insert step: #field_ident
                if let ::std::option::Option::Some(step_data) = #extracted_field_ident {
                    #insert_call
                }
            }
        };

        code_blocks.push(code);
    }

    Ok(quote! {
        #(#code_blocks)*
    })
}

/// Generate code for has_one/has_many (post-insert) steps.
/// Uses with_* setters to inject FK values (avoiding direct field access for cross-module compatibility).
/// Supports mode = "insert" (default) or "upsert" per doc §6.1.1.
fn generate_has_relation_code(
    graph: &GraphDeclarations,
    _root_key_ident: &syn::Ident,
) -> Result<TokenStream> {
    if graph.has_relations.is_empty() {
        return Ok(quote! {});
    }

    let mut code_blocks = Vec::new();

    for rel in &graph.has_relations {
        let _field_ident = format_ident!("{}", rel.field);
        let extracted_field_ident = format_ident!("__pgorm_graph_{}", rel.field);
        let child_type = &rel.child_type;
        let tag = format!(
            "graph:{}:{}",
            if rel.is_many { "has_many" } else { "has_one" },
            rel.field
        );

        // Generate setter call using with_* method
        let setter_name = format_ident!("with_{}", rel.fk_field);

        let code = if rel.is_many {
            // has_many: Vec<Child> or Option<Vec<Child>>
            let insert_call = match rel.mode {
                HasRelationMode::Insert => {
                    quote! { #child_type::insert_many(conn, children_with_fk).await? }
                }
                HasRelationMode::Upsert => {
                    quote! { #child_type::upsert_many(conn, children_with_fk).await? }
                }
            };
            quote! {
                // has_many: #_field_ident
                if let ::std::option::Option::Some(children) = #extracted_field_ident {
                    if !children.is_empty() {
                        // Inject root ID into each child's fk field using with_* setter
                        let children_with_fk: ::std::vec::Vec<_> = children
                            .into_iter()
                            .map(|child| child.#setter_name(__pgorm_root_id.clone()))
                            .collect();
                        // Insert/upsert all children
                        let affected = #insert_call;
                        __pgorm_steps.push(::pgorm::WriteStepReport {
                            tag: #tag,
                            affected,
                        });
                        __pgorm_total_affected += affected;
                    }
                }
            }
        } else {
            // has_one: Child or Option<Child>
            let insert_call = match rel.mode {
                HasRelationMode::Insert => quote! { child_with_fk.insert(conn).await? },
                HasRelationMode::Upsert => quote! { child_with_fk.upsert(conn).await? },
            };
            quote! {
                // has_one: #_field_ident
                if let ::std::option::Option::Some(child) = #extracted_field_ident {
                    // Inject root ID into child's fk field using with_* setter
                    let child_with_fk = child.#setter_name(__pgorm_root_id.clone());
                    // Insert/upsert child
                    let affected = #insert_call;
                    __pgorm_steps.push(::pgorm::WriteStepReport {
                        tag: #tag,
                        affected,
                    });
                    __pgorm_total_affected += affected;
                }
            }
        };

        code_blocks.push(code);
    }

    Ok(quote! {
        #(#code_blocks)*
    })
}
