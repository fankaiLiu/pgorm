//! InsertModel derive macro implementation

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Result};

struct BindField {
    ident: syn::Ident,
    ty: syn::Type,
    column: String,
}

struct StructAttrs {
    table: String,
    returning: Option<syn::Path>,
}

struct StructAttrList {
    table: Option<String>,
    returning: Option<syn::Path>,
}

impl syn::parse::Parse for StructAttrList {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let mut table: Option<String> = None;
        let mut returning: Option<syn::Path> = None;

        loop {
            if input.is_empty() {
                break;
            }

            let ident: syn::Ident = input.parse()?;
            let key = ident.to_string();

            let _: syn::Token![=] = input.parse()?;
            let value: syn::LitStr = input.parse()?;

            match key.as_str() {
                "table" => table = Some(value.value()),
                "returning" => {
                    let ty: syn::Path = syn::parse_str(&value.value()).map_err(|e| {
                        syn::Error::new(Span::call_site(), format!("invalid returning type: {e}"))
                    })?;
                    returning = Some(ty);
                }
                _ => {}
            }

            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            } else {
                break;
            }
        }

        Ok(Self { table, returning })
    }
}

struct FieldAttrs {
    is_id: bool,
    skip_insert: bool,
    default: bool,
    table: Option<String>,
    column: Option<String>,
}

impl syn::parse::Parse for FieldAttrs {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let mut attrs = FieldAttrs {
            is_id: false,
            skip_insert: false,
            default: false,
            table: None,
            column: None,
        };

        loop {
            if input.is_empty() {
                break;
            }

            let ident: syn::Ident = input.parse()?;
            let key = ident.to_string();

            match key.as_str() {
                "id" => attrs.is_id = true,
                "skip_insert" => attrs.skip_insert = true,
                "default" => attrs.default = true,
                _ => {
                    let _: syn::Token![=] = input.parse()?;
                    let value: syn::LitStr = input.parse()?;
                    match key.as_str() {
                        "table" => attrs.table = Some(value.value()),
                        "column" => attrs.column = Some(value.value()),
                        _ => {}
                    }
                }
            }

            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            } else {
                break;
            }
        }

        Ok(attrs)
    }
}

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let struct_attrs = get_struct_attrs(&input)?;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "InsertModel can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "InsertModel can only be derived for structs",
            ));
        }
    };

    let mut insert_columns: Vec<String> = Vec::new();
    let mut insert_value_exprs: Vec<String> = Vec::new();
    let mut bind_field_idents: Vec<syn::Ident> = Vec::new();
    let mut batch_bind_fields: Vec<BindField> = Vec::new();
    let mut id_field: Option<BindField> = None;

    let mut param_idx = 0_usize;

    for field in fields.iter() {
        let field_ident = field.ident.clone().unwrap();
        let field_name = field_ident.to_string();
        let field_ty = field.ty.clone();

        let field_attrs = get_field_attrs(field)?;

        if let Some(field_table) = &field_attrs.table {
            if field_table != &struct_attrs.table {
                return Err(syn::Error::new_spanned(
                    field,
                    "InsertModel does not support fields from joined/other tables",
                ));
            }
        }

        let column_name = field_attrs
            .column
            .clone()
            .unwrap_or_else(|| field_name.clone());

        if field_attrs.is_id {
            if id_field.is_some() {
                return Err(syn::Error::new_spanned(
                    field,
                    "InsertModel supports only one #[orm(id)] field",
                ));
            }
            id_field = Some(BindField {
                ident: field_ident.clone(),
                ty: field_ty.clone(),
                column: column_name.clone(),
            });
        }

        if field_attrs.skip_insert || field_attrs.is_id {
            continue;
        }

        insert_columns.push(column_name.clone());

        if field_attrs.default {
            insert_value_exprs.push("DEFAULT".to_string());
        } else {
            param_idx += 1;
            insert_value_exprs.push(format!("${}", param_idx));
            bind_field_idents.push(field_ident.clone());
            batch_bind_fields.push(BindField {
                ident: field_ident,
                ty: field_ty,
                column: column_name,
            });
        }
    }

    let table_name = &struct_attrs.table;
    let insert_sql = if insert_columns.is_empty() {
        format!("INSERT INTO {} DEFAULT VALUES", table_name)
    } else {
        format!(
            "INSERT INTO {} ({}) VALUES ({})",
            table_name,
            insert_columns.join(", "),
            insert_value_exprs.join(", ")
        )
    };

    let destructure = if bind_field_idents.is_empty() {
        quote! { let _ = self; }
    } else {
        quote! { let Self { #(#bind_field_idents),*, .. } = self; }
    };

    let insert_query_expr = bind_field_idents.iter().fold(
        quote! { pgorm::query(#insert_sql) },
        |acc, ident| quote! { #acc.bind(#ident) },
    );

    let insert_method = quote! {
        /// Insert a new row into the target table.
        pub async fn insert(self, conn: &impl pgorm::GenericClient) -> pgorm::OrmResult<u64> {
            #destructure
            #insert_query_expr.execute(conn).await
        }
    };

    let insert_many_method = if batch_bind_fields.is_empty() {
        quote! {
            /// Insert multiple rows.
            ///
            /// Falls back to per-row inserts when bulk insert isn't applicable.
            pub async fn insert_many(
                conn: &impl pgorm::GenericClient,
                rows: Vec<Self>,
            ) -> pgorm::OrmResult<u64> {
                if rows.is_empty() {
                    return Ok(0);
                }

                let mut affected = 0_u64;
                for row in rows {
                    affected += row.insert(conn).await?;
                }
                Ok(affected)
            }
        }
    } else {
        let batch_columns: Vec<String> = batch_bind_fields.iter().map(|f| f.column.clone()).collect();
        let placeholders: Vec<String> = (1..=batch_bind_fields.len()).map(|i| format!("${}", i)).collect();
        let batch_insert_sql = format!(
            "INSERT INTO {} ({}) SELECT * FROM UNNEST({})",
            table_name,
            batch_columns.join(", "),
            placeholders.join(", ")
        );

        let list_idents: Vec<syn::Ident> = batch_bind_fields
            .iter()
            .map(|f| format_ident!("__pgorm_insert_{}_list", f.ident))
            .collect();
        let field_idents: Vec<syn::Ident> = batch_bind_fields.iter().map(|f| f.ident.clone()).collect();
        let field_tys: Vec<syn::Type> = batch_bind_fields.iter().map(|f| f.ty.clone()).collect();

        let init_lists: Vec<TokenStream> = list_idents
            .iter()
            .zip(field_tys.iter())
            .map(|(list_ident, ty)| {
                quote! { let mut #list_ident: Vec<#ty> = Vec::with_capacity(rows.len()); }
            })
            .collect();

        let push_lists: Vec<TokenStream> = list_idents
            .iter()
            .zip(field_idents.iter())
            .map(|(list_ident, field_ident)| quote! { #list_ident.push(#field_ident); })
            .collect();

        let bind_lists_expr = list_idents.iter().fold(
            quote! { pgorm::query(#batch_insert_sql) },
            |acc, list_ident| quote! { #acc.bind(#list_ident) },
        );

        quote! {
            /// Insert multiple rows using a single statement (UNNEST bulk insert).
            pub async fn insert_many(
                conn: &impl pgorm::GenericClient,
                rows: Vec<Self>,
            ) -> pgorm::OrmResult<u64> {
                if rows.is_empty() {
                    return Ok(0);
                }

                #(#init_lists)*

                for row in rows {
                    let Self { #(#field_idents),*, .. } = row;
                    #(#push_lists)*
                }

                #bind_lists_expr.execute(conn).await
            }
        }
    };

    let upsert_methods = if let Some(id_field) = id_field.as_ref() {
        let id_col = id_field.column.clone();

        let upsert_columns: Vec<String> = std::iter::once(id_col.clone())
            .chain(batch_bind_fields.iter().map(|f| f.column.clone()))
            .collect();
        let upsert_bind_idents: Vec<syn::Ident> = std::iter::once(id_field.ident.clone())
            .chain(batch_bind_fields.iter().map(|f| f.ident.clone()))
            .collect();

        let placeholders: Vec<String> = (1..=upsert_bind_idents.len()).map(|i| format!("${}", i)).collect();
        let mut update_assignments: Vec<String> = batch_bind_fields
            .iter()
            .map(|f| format!("{} = EXCLUDED.{}", f.column, f.column))
            .collect();
        if update_assignments.is_empty() {
            update_assignments.push(format!("{} = EXCLUDED.{}", id_col, id_col));
        }

        let upsert_sql = format!(
            "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {}",
            table_name,
            upsert_columns.join(", "),
            placeholders.join(", "),
            id_col,
            update_assignments.join(", ")
        );

        let upsert_batch_sql = format!(
            "INSERT INTO {} ({}) SELECT * FROM UNNEST({}) ON CONFLICT ({}) DO UPDATE SET {}",
            table_name,
            upsert_columns.join(", "),
            placeholders.join(", "),
            id_col,
            update_assignments.join(", ")
        );

        let upsert_destructure = quote! { let Self { #(#upsert_bind_idents),*, .. } = self; };
        let upsert_query_expr = upsert_bind_idents.iter().fold(
            quote! { pgorm::query(#upsert_sql) },
            |acc, ident| quote! { #acc.bind(#ident) },
        );

        let upsert_method = quote! {
            /// Insert a row, or update it if the primary key already exists (Postgres UPSERT).
            pub async fn upsert(self, conn: &impl pgorm::GenericClient) -> pgorm::OrmResult<u64> {
                #upsert_destructure
                #upsert_query_expr.execute(conn).await
            }
        };

        let upsert_batch_list_idents: Vec<syn::Ident> = upsert_bind_idents
            .iter()
            .map(|ident| format_ident!("__pgorm_upsert_{}_list", ident))
            .collect();
        let upsert_batch_field_tys: Vec<syn::Type> = std::iter::once(id_field.ty.clone())
            .chain(batch_bind_fields.iter().map(|f| f.ty.clone()))
            .collect();

        let upsert_batch_init_lists: Vec<TokenStream> = upsert_batch_list_idents
            .iter()
            .zip(upsert_batch_field_tys.iter())
            .map(|(list_ident, ty)| {
                quote! { let mut #list_ident: Vec<#ty> = Vec::with_capacity(rows.len()); }
            })
            .collect();

        let upsert_batch_push_lists: Vec<TokenStream> = upsert_batch_list_idents
            .iter()
            .zip(upsert_bind_idents.iter())
            .map(|(list_ident, field_ident)| quote! { #list_ident.push(#field_ident); })
            .collect();

        let upsert_many_query_expr = upsert_batch_list_idents.iter().fold(
            quote! { pgorm::query(#upsert_batch_sql) },
            |acc, list_ident| quote! { #acc.bind(#list_ident) },
        );

        let upsert_many_method = quote! {
            /// Insert or update multiple rows using a single statement (UNNEST + ON CONFLICT).
            pub async fn upsert_many(
                conn: &impl pgorm::GenericClient,
                rows: Vec<Self>,
            ) -> pgorm::OrmResult<u64> {
                if rows.is_empty() {
                    return Ok(0);
                }

                #(#upsert_batch_init_lists)*

                for row in rows {
                    let Self { #(#upsert_bind_idents),*, .. } = row;
                    #(#upsert_batch_push_lists)*
                }

                #upsert_many_query_expr.execute(conn).await
            }
        };

        let upsert_returning_methods = if let Some(returning_ty) = struct_attrs.returning.as_ref() {
            let upsert_returning_query_expr = upsert_bind_idents.iter().fold(
                quote! { pgorm::query(sql) },
                |acc, ident| quote! { #acc.bind(#ident) },
            );
            let upsert_many_returning_query_expr = upsert_batch_list_idents.iter().fold(
                quote! { pgorm::query(sql) },
                |acc, list_ident| quote! { #acc.bind(#list_ident) },
            );

            quote! {
                /// UPSERT and return the resulting row mapped as the configured returning type.
                pub async fn upsert_returning(
                    self,
                    conn: &impl pgorm::GenericClient,
                ) -> pgorm::OrmResult<#returning_ty>
                where
                    #returning_ty: pgorm::FromRow,
                {
                    #upsert_destructure
                    let sql = format!(
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
                    conn: &impl pgorm::GenericClient,
                    rows: Vec<Self>,
                ) -> pgorm::OrmResult<Vec<#returning_ty>>
                where
                    #returning_ty: pgorm::FromRow,
                {
                    if rows.is_empty() {
                        return Ok(Vec::new());
                    }

                    #(#upsert_batch_init_lists)*

                    for row in rows {
                        let Self { #(#upsert_bind_idents),*, .. } = row;
                        #(#upsert_batch_push_lists)*
                    }

                    let sql = format!(
                        "WITH {table} AS ({upsert} RETURNING *) SELECT {} FROM {table} {}",
                        #returning_ty::SELECT_LIST,
                        #returning_ty::JOIN_CLAUSE,
                        table = #table_name,
                        upsert = #upsert_batch_sql,
                    );

                    #upsert_many_returning_query_expr.fetch_all_as::<#returning_ty>(conn).await
                }
            }
        } else {
            quote! {}
        };

        quote! {
            #upsert_method
            #upsert_many_method
            #upsert_returning_methods
        }
    } else {
        quote! {}
    };

    let returning_method = if let Some(returning_ty) = struct_attrs.returning.as_ref() {
        let returning_query_expr =
            bind_field_idents
                .iter()
                .fold(quote! { pgorm::query(sql) }, |acc, ident| {
                    quote! { #acc.bind(#ident) }
                });

        let insert_many_returning_method = if batch_bind_fields.is_empty() {
            quote! {
                /// Insert multiple rows and return created rows mapped as the configured returning type.
                ///
                /// Falls back to per-row inserts when bulk insert isn't applicable.
                pub async fn insert_many_returning(
                    conn: &impl pgorm::GenericClient,
                    rows: Vec<Self>,
                ) -> pgorm::OrmResult<Vec<#returning_ty>>
                where
                    #returning_ty: pgorm::FromRow,
                {
                    if rows.is_empty() {
                        return Ok(Vec::new());
                    }

                    let mut out = Vec::with_capacity(rows.len());
                    for row in rows {
                        out.push(row.insert_returning(conn).await?);
                    }
                    Ok(out)
                }
            }
        } else {
            let batch_columns: Vec<String> =
                batch_bind_fields.iter().map(|f| f.column.clone()).collect();
            let placeholders: Vec<String> = (1..=batch_bind_fields.len())
                .map(|i| format!("${}", i))
                .collect();
            let batch_insert_sql = format!(
                "INSERT INTO {} ({}) SELECT * FROM UNNEST({})",
                table_name,
                batch_columns.join(", "),
                placeholders.join(", ")
            );

            let list_idents: Vec<syn::Ident> = batch_bind_fields
                .iter()
                .map(|f| format_ident!("__pgorm_insert_{}_list", f.ident))
                .collect();
            let field_idents: Vec<syn::Ident> =
                batch_bind_fields.iter().map(|f| f.ident.clone()).collect();
            let field_tys: Vec<syn::Type> = batch_bind_fields.iter().map(|f| f.ty.clone()).collect();

            let init_lists: Vec<TokenStream> = list_idents
                .iter()
                .zip(field_tys.iter())
                .map(|(list_ident, ty)| {
                    quote! { let mut #list_ident: Vec<#ty> = Vec::with_capacity(rows.len()); }
                })
                .collect();

            let push_lists: Vec<TokenStream> = list_idents
                .iter()
                .zip(field_idents.iter())
                .map(|(list_ident, field_ident)| quote! { #list_ident.push(#field_ident); })
                .collect();

            let batch_returning_query_expr = list_idents.iter().fold(
                quote! { pgorm::query(sql) },
                |acc, list_ident| quote! { #acc.bind(#list_ident) },
            );

            quote! {
                /// Insert multiple rows and return created rows mapped as the configured returning type.
                pub async fn insert_many_returning(
                    conn: &impl pgorm::GenericClient,
                    rows: Vec<Self>,
                ) -> pgorm::OrmResult<Vec<#returning_ty>>
                where
                    #returning_ty: pgorm::FromRow,
                {
                    if rows.is_empty() {
                        return Ok(Vec::new());
                    }

                    #(#init_lists)*

                    for row in rows {
                        let Self { #(#field_idents),*, .. } = row;
                        #(#push_lists)*
                    }

                    let sql = format!(
                        "WITH {table} AS ({insert} RETURNING *) SELECT {} FROM {table} {}",
                        #returning_ty::SELECT_LIST,
                        #returning_ty::JOIN_CLAUSE,
                        table = #table_name,
                        insert = #batch_insert_sql,
                    );

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
                let sql = format!(
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
    } else {
        quote! {}
    };

    Ok(quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            pub const TABLE: &'static str = #table_name;

            #insert_method

            #insert_many_method

            #upsert_methods

            #returning_method
        }
    })
}

fn get_struct_attrs(input: &DeriveInput) -> Result<StructAttrs> {
    let mut table: Option<String> = None;
    let mut returning: Option<syn::Path> = None;

    for attr in &input.attrs {
        if !attr.path().is_ident("orm") {
            continue;
        }

        if let syn::Meta::List(meta_list) = &attr.meta {
            let parsed = syn::parse2::<StructAttrList>(meta_list.tokens.clone())?;
            if parsed.table.is_some() {
                table = parsed.table;
            }
            if parsed.returning.is_some() {
                returning = parsed.returning;
            }
        }
    }

    let table = table.ok_or_else(|| {
        syn::Error::new_spanned(
            input,
            "InsertModel requires #[orm(table = \"table_name\")] attribute",
        )
    })?;

    Ok(StructAttrs { table, returning })
}

fn get_field_attrs(field: &syn::Field) -> Result<FieldAttrs> {
    let mut merged = FieldAttrs {
        is_id: false,
        skip_insert: false,
        default: false,
        table: None,
        column: None,
    };

    for attr in &field.attrs {
        if !attr.path().is_ident("orm") {
            continue;
        }

        if let syn::Meta::List(meta_list) = &attr.meta {
            let parsed = syn::parse2::<FieldAttrs>(meta_list.tokens.clone())?;
            merged.is_id |= parsed.is_id;
            merged.skip_insert |= parsed.skip_insert;
            merged.default |= parsed.default;
            if parsed.table.is_some() {
                merged.table = parsed.table;
            }
            if parsed.column.is_some() {
                merged.column = parsed.column;
            }
        }
    }

    Ok(merged)
}
