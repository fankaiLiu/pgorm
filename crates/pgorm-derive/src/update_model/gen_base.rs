//! Base update methods code generation.
//!
//! This module contains code generation for:
//! - `update_by_id` / `update_by_ids` methods
//! - `update_by_id_returning` / `update_by_ids_returning` methods

use proc_macro2::TokenStream;
use quote::quote;

use super::attrs::StructAttrs;

/// Generate update_by_id and update_by_ids methods.
pub(super) fn generate_update_by_id_methods(
    table_name: &str,
    id_col_expr: &TokenStream,
    destructure: &TokenStream,
    set_stmts: &[TokenStream],
) -> TokenStream {
    quote! {
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
    }
}

/// Generate update_by_id_returning and update_by_ids_returning methods.
pub(super) fn generate_update_returning_methods(
    attrs: &StructAttrs,
    table_name: &str,
    id_col_expr: &TokenStream,
    destructure: &TokenStream,
    set_stmts: &[TokenStream],
) -> TokenStream {
    let returning_ty = match attrs.returning.as_ref() {
        Some(ty) => ty,
        None => return quote! {},
    };

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
}
