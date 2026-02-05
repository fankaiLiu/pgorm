//! Base update methods code generation.
//!
//! This module contains code generation for:
//! - `update_by_id` / `update_by_ids` methods
//! - `update_by_id_returning` / `update_by_ids_returning` methods
//! - `update_by_id_force` / `update_by_id_force_returning` methods (when version field exists)

use proc_macro2::TokenStream;
use quote::quote;

use super::attrs::StructAttrs;

/// Generate update_by_id and update_by_ids methods.
pub(super) fn generate_update_by_id_methods(
    table_name: &str,
    id_col_expr: &TokenStream,
    destructure: &TokenStream,
    set_stmts: &[TokenStream],
    has_auto_now: bool,
    version_field: Option<&(syn::Ident, String)>,
) -> TokenStream {
    let now_init = if has_auto_now {
        quote! { let __pgorm_now = ::chrono::Utc::now(); }
    } else {
        quote! {}
    };

    // Generate version SET clause: version = version + 1
    let version_set = if let Some((_, version_col)) = version_field {
        quote! {
            if !first {
                q.push(", ");
            } else {
                first = false;
            }
            q.push(#version_col);
            q.push(" = ");
            q.push(#version_col);
            q.push(" + 1");
        }
    } else {
        quote! {}
    };

    // Generate version WHERE clause: AND version = $N
    let version_where = if let Some((version_ident, version_col)) = version_field {
        quote! {
            q.push(" AND ");
            q.push(#version_col);
            q.push(" = ");
            q.push_bind(#version_ident as i64);
        }
    } else {
        quote! {}
    };

    // Suppress unused variable warning for version ident in update_by_ids
    // (bulk updates don't use version checking)
    let version_suppress = if let Some((version_ident, _)) = version_field {
        quote! { let _ = #version_ident; }
    } else {
        quote! {}
    };

    // For version checking, we need to capture id string before push_bind moves it
    let (id_capture, execute_with_check) = if let Some((version_ident, _)) = version_field {
        let capture = quote! {
            let __id_str = format!("{:?}", &id);
            let __version_val = #version_ident as i64;
        };
        let check = quote! {
            let __affected = q.execute(conn).await?;
            if __affected == 0 {
                return Err(pgorm::OrmError::stale_record(
                    #table_name,
                    __id_str,
                    __version_val,
                ));
            }
            Ok(__affected)
        };
        (capture, check)
    } else {
        (quote! {}, quote! { q.execute(conn).await })
    };

    quote! {
        /// Update columns by primary key (patch-style).
        ///
        /// If the struct has a `#[orm(version)]` field, this method performs optimistic locking:
        /// it checks that the version matches and returns `OrmError::StaleRecord` if not.
        pub async fn update_by_id<I>(
            self,
            conn: &impl pgorm::GenericClient,
            id: I,
        ) -> pgorm::OrmResult<u64>
        where
            I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
        {
            #destructure
            #now_init
            #id_capture

            let mut q = pgorm::sql("UPDATE ");
            q.push(#table_name);
            q.push(" SET ");

            let mut first = true;
            #(#set_stmts)*
            #version_set

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
            #version_where

            #execute_with_check
        }

        /// Update columns by primary key for multiple rows (patch-style).
        ///
        /// The same patch is applied to every matched row.
        ///
        /// Note: Optimistic locking (version check) is NOT supported for bulk updates.
        /// If you need version checking, use a loop with `update_by_id` instead.
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
            #now_init
            #version_suppress

            let mut q = pgorm::sql("UPDATE ");
            q.push(#table_name);
            q.push(" SET ");

            let mut first = true;
            #(#set_stmts)*
            #version_set

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

            // Note: No version check for bulk updates
            q.execute(conn).await
        }
    }
}

/// Generate update_by_id_force methods (skip version check).
/// Only generated when version field exists.
pub(super) fn generate_update_force_methods(
    table_name: &str,
    id_col_expr: &TokenStream,
    destructure: &TokenStream,
    set_stmts: &[TokenStream],
    has_auto_now: bool,
    version_col: &str,
    version_ident: &syn::Ident,
) -> TokenStream {
    let now_init = if has_auto_now {
        quote! { let __pgorm_now = ::chrono::Utc::now(); }
    } else {
        quote! {}
    };

    let version_suppress = quote! { let _ = #version_ident; };

    // Version SET clause (still increment version, just don't check it)
    let version_set = quote! {
        if !first {
            q.push(", ");
        } else {
            first = false;
        }
        q.push(#version_col);
        q.push(" = ");
        q.push(#version_col);
        q.push(" + 1");
    };

    quote! {
        /// Update columns by primary key, skipping optimistic locking check.
        ///
        /// This method still increments the version field but does NOT check
        /// the current version. Use this for admin overrides or when you
        /// explicitly want to bypass version checking.
        pub async fn update_by_id_force<I>(
            self,
            conn: &impl pgorm::GenericClient,
            id: I,
        ) -> pgorm::OrmResult<u64>
        where
            I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
        {
            #destructure
            #now_init
            #version_suppress

            let mut q = pgorm::sql("UPDATE ");
            q.push(#table_name);
            q.push(" SET ");

            let mut first = true;
            #(#set_stmts)*
            #version_set

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
    }
}

/// Generate update_by_id_returning and update_by_ids_returning methods.
pub(super) fn generate_update_returning_methods(
    attrs: &StructAttrs,
    table_name: &str,
    id_col_expr: &TokenStream,
    destructure: &TokenStream,
    set_stmts: &[TokenStream],
    has_auto_now: bool,
    version_field: Option<&(syn::Ident, String)>,
) -> TokenStream {
    let returning_ty = match attrs.returning.as_ref() {
        Some(ty) => ty,
        None => return quote! {},
    };

    let now_init = if has_auto_now {
        quote! { let __pgorm_now = ::chrono::Utc::now(); }
    } else {
        quote! {}
    };

    // Generate version SET clause: version = version + 1
    let version_set = if let Some((_, version_col)) = version_field {
        quote! {
            if !first {
                q.push(", ");
            } else {
                first = false;
            }
            q.push(#version_col);
            q.push(" = ");
            q.push(#version_col);
            q.push(" + 1");
        }
    } else {
        quote! {}
    };

    // Generate version WHERE clause: AND version = $N
    let version_where = if let Some((version_ident, version_col)) = version_field {
        quote! {
            q.push(" AND ");
            q.push(#version_col);
            q.push(" = ");
            q.push_bind(#version_ident as i64);
        }
    } else {
        quote! {}
    };

    // Suppress unused variable warning for version ident in bulk/force methods
    let version_suppress = if let Some((version_ident, _)) = version_field {
        quote! { let _ = #version_ident; }
    } else {
        quote! {}
    };

    // For returning methods, we need to capture id string before push_bind moves it
    let (id_capture, fetch_with_check) = if let Some((version_ident, _)) = version_field {
        let capture = quote! {
            let __id_str = format!("{:?}", &id);
            let __version_val = #version_ident as i64;
        };
        let check = quote! {
            match q.fetch_one_as::<#returning_ty>(conn).await {
                Ok(row) => Ok(row),
                Err(pgorm::OrmError::NotFound(_)) => {
                    Err(pgorm::OrmError::stale_record(
                        #table_name,
                        __id_str,
                        __version_val,
                    ))
                }
                Err(e) => Err(e),
            }
        };
        (capture, check)
    } else {
        (
            quote! {},
            quote! { q.fetch_one_as::<#returning_ty>(conn).await },
        )
    };

    // Generate force returning method if version field exists
    let force_returning = if let Some((_, version_col)) = version_field {
        let version_set_force = quote! {
            if !first {
                q.push(", ");
            } else {
                first = false;
            }
            q.push(#version_col);
            q.push(" = ");
            q.push(#version_col);
            q.push(" + 1");
        };

        quote! {
            /// Update columns by primary key and return the updated row, skipping optimistic locking check.
            ///
            /// This method still increments the version field but does NOT check
            /// the current version. Use this for admin overrides or when you
            /// explicitly want to bypass version checking.
            pub async fn update_by_id_force_returning<I>(
                self,
                conn: &impl pgorm::GenericClient,
                id: I,
            ) -> pgorm::OrmResult<#returning_ty>
            where
                I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
                #returning_ty: pgorm::FromRow,
            {
                #destructure
                #now_init
                #version_suppress

                let mut q = pgorm::Sql::empty();
                q.push("WITH ");
                q.push(#table_name);
                q.push(" AS (UPDATE ");
                q.push(#table_name);
                q.push(" SET ");

                let mut first = true;
                #(#set_stmts)*
                #version_set_force

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
        }
    } else {
        quote! {}
    };

    quote! {
        /// Update columns by primary key and return the updated row mapped as the configured returning type.
        ///
        /// If the struct has a `#[orm(version)]` field, this method performs optimistic locking:
        /// it checks that the version matches and returns `OrmError::StaleRecord` if not.
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
            #now_init
            #id_capture

            let mut q = pgorm::Sql::empty();
            q.push("WITH ");
            q.push(#table_name);
            q.push(" AS (UPDATE ");
            q.push(#table_name);
            q.push(" SET ");

            let mut first = true;
            #(#set_stmts)*
            #version_set

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
            #version_where

            q.push(" RETURNING *) SELECT ");
            q.push(#returning_ty::SELECT_LIST);
            q.push(" FROM ");
            q.push(#table_name);
            q.push(" ");
            q.push(#returning_ty::JOIN_CLAUSE);

            #fetch_with_check
        }

        #force_returning

        /// Update columns by primary key for multiple rows and return updated rows
        /// mapped as the configured returning type.
        ///
        /// The same patch is applied to every matched row.
        ///
        /// Note: Optimistic locking (version check) is NOT supported for bulk updates.
        /// If you need version checking, use a loop with `update_by_id_returning` instead.
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
            #now_init
            #version_suppress

            let mut q = pgorm::Sql::empty();
            q.push("WITH ");
            q.push(#table_name);
            q.push(" AS (UPDATE ");
            q.push(#table_name);
            q.push(" SET ");

            let mut first = true;
            #(#set_stmts)*
            #version_set

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

            // Note: No version check for bulk updates
            q.fetch_all_as::<#returning_ty>(conn).await
        }
    }
}
