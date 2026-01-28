//! UpdateModel derive macro implementation

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Fields, Result};

struct StructAttrs {
    table: String,
    id_column: Option<String>,
    model: Option<syn::Path>,
    returning: Option<syn::Path>,
}

struct StructAttrList {
    table: Option<String>,
    id_column: Option<String>,
    model: Option<syn::Path>,
    returning: Option<syn::Path>,
}

impl syn::parse::Parse for StructAttrList {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let mut table: Option<String> = None;
        let mut id_column: Option<String> = None;
        let mut model: Option<syn::Path> = None;
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
                "id_column" => id_column = Some(value.value()),
                "model" => {
                    let ty: syn::Path = syn::parse_str(&value.value()).map_err(|e| {
                        syn::Error::new(Span::call_site(), format!("invalid model type: {e}"))
                    })?;
                    model = Some(ty);
                }
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

        Ok(Self {
            table,
            id_column,
            model,
            returning,
        })
    }
}

struct FieldAttrs {
    skip_update: bool,
    default: bool,
    table: Option<String>,
    column: Option<String>,
}

impl syn::parse::Parse for FieldAttrs {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let mut attrs = FieldAttrs {
            skip_update: false,
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
                "skip_update" => attrs.skip_update = true,
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

    for field in fields.iter() {
        let field_ident = field.ident.clone().unwrap();
        let field_name = field_ident.to_string();
        let field_ty = &field.ty;

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
            I: tokio_postgres::types::ToSql + Sync + Send + 'static,
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
            ids: Vec<I>,
        ) -> pgorm::OrmResult<u64>
        where
            I: tokio_postgres::types::ToSql + Sync + Send + 'static,
        {
            if ids.is_empty() {
                return Ok(0);
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
                I: tokio_postgres::types::ToSql + Sync + Send + 'static,
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
                ids: Vec<I>,
            ) -> pgorm::OrmResult<Vec<#returning_ty>>
            where
                I: tokio_postgres::types::ToSql + Sync + Send + 'static,
                #returning_ty: pgorm::FromRow,
            {
                if ids.is_empty() {
                    return Ok(Vec::new());
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

    Ok(quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            pub const TABLE: &'static str = #table_name;

            #update_by_id_method

            #update_by_ids_method

            #update_by_id_returning_method
        }
    })
}

fn get_struct_attrs(input: &DeriveInput) -> Result<StructAttrs> {
    let mut table: Option<String> = None;
    let mut id_column: Option<String> = None;
    let mut model: Option<syn::Path> = None;
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
            if parsed.id_column.is_some() {
                id_column = parsed.id_column;
            }
            if parsed.model.is_some() {
                model = parsed.model;
            }
            if parsed.returning.is_some() {
                returning = parsed.returning;
            }
        }
    }

    let table = table.ok_or_else(|| {
        syn::Error::new_spanned(
            input,
            "UpdateModel requires #[orm(table = \"table_name\")] attribute",
        )
    })?;

    Ok(StructAttrs {
        table,
        id_column,
        model,
        returning,
    })
}

fn get_field_attrs(field: &syn::Field) -> Result<FieldAttrs> {
    let mut merged = FieldAttrs {
        skip_update: false,
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
            merged.skip_update |= parsed.skip_update;
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

fn option_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    let seg = type_path.path.segments.last()?;
    if seg.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    if args.args.len() != 1 {
        return None;
    }
    let syn::GenericArgument::Type(inner) = args.args.first()? else {
        return None;
    };
    Some(inner)
}
