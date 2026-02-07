//! `#[derive(PgComposite)]` — map a Rust struct to a PostgreSQL composite type.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, ExprLit, Fields, Lit, Meta, Result};

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;

    // Must be a struct with named fields
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "PgComposite requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "PgComposite can only be derived for structs",
            ));
        }
    };

    let pg_type = parse_pg_type(&input)?;
    let pg_type_array = format!("{pg_type}[]");
    let field_count = fields.len() as i32;

    // Generate encoding for each field
    let mut encode_fields = Vec::new();
    let mut decode_fields = Vec::new();
    let mut field_names = Vec::new();

    for (idx, field) in fields.iter().enumerate() {
        let field_name = field.ident.as_ref().unwrap();
        let field_ty = &field.ty;
        let idx_i32 = idx as i32;
        let _ = idx_i32; // used below

        field_names.push(field_name.clone());

        // Encode: write OID (0 = anonymous), length, value
        encode_fields.push(quote! {
            // Field OID — use 0 (anonymous / inferred by server)
            out.extend_from_slice(&0_u32.to_be_bytes());

            // Encode the field value
            {
                let len_pos = out.len();
                out.extend_from_slice(&[0u8; 4]); // placeholder for length
                let start = out.len();

                match ::pgorm::tokio_postgres::types::ToSql::to_sql(
                    &self.#field_name,
                    // Use TEXT type as a fallback for composite fields.
                    // The server handles the actual type resolution.
                    &::pgorm::tokio_postgres::types::Type::TEXT,
                    out,
                )? {
                    ::pgorm::tokio_postgres::types::IsNull::Yes => {
                        out[len_pos..len_pos + 4].copy_from_slice(&(-1_i32).to_be_bytes());
                    }
                    ::pgorm::tokio_postgres::types::IsNull::No => {
                        let written = (out.len() - start) as i32;
                        out[len_pos..len_pos + 4].copy_from_slice(&written.to_be_bytes());
                    }
                }
            }
        });

        // Decode: read OID, length, value
        decode_fields.push(quote! {
            // Skip field OID (4 bytes)
            if __pgorm_pos + 4 > raw.len() {
                return Err("composite: insufficient data for field OID".into());
            }
            __pgorm_pos += 4;

            // Read field length
            if __pgorm_pos + 4 > raw.len() {
                return Err("composite: insufficient data for field length".into());
            }
            let __pgorm_field_len = i32::from_be_bytes(
                raw[__pgorm_pos..__pgorm_pos + 4].try_into().unwrap(),
            );
            __pgorm_pos += 4;

            let #field_name: #field_ty = if __pgorm_field_len < 0 {
                // NULL — try FromSql::from_sql_null
                <#field_ty as ::pgorm::tokio_postgres::types::FromSql>::from_sql_null(
                    &::pgorm::tokio_postgres::types::Type::TEXT,
                )?
            } else {
                let __pgorm_end = __pgorm_pos + __pgorm_field_len as usize;
                if __pgorm_end > raw.len() {
                    return Err("composite: insufficient data for field value".into());
                }
                let __pgorm_val = <#field_ty as ::pgorm::tokio_postgres::types::FromSql>::from_sql(
                    &::pgorm::tokio_postgres::types::Type::TEXT,
                    &raw[__pgorm_pos..__pgorm_end],
                )?;
                __pgorm_pos = __pgorm_end;
                __pgorm_val
            };
        });
    }

    let expanded = quote! {
        impl ::pgorm::tokio_postgres::types::ToSql for #name {
            fn to_sql(
                &self,
                _ty: &::pgorm::tokio_postgres::types::Type,
                out: &mut ::bytes::BytesMut,
            ) -> ::std::result::Result<
                ::pgorm::tokio_postgres::types::IsNull,
                ::std::boxed::Box<dyn ::std::error::Error + ::std::marker::Sync + ::std::marker::Send>,
            > {
                // Write field count
                out.extend_from_slice(&(#field_count as i32).to_be_bytes());

                #(#encode_fields)*

                Ok(::pgorm::tokio_postgres::types::IsNull::No)
            }

            fn accepts(ty: &::pgorm::tokio_postgres::types::Type) -> bool {
                ty.name() == #pg_type
            }

            ::pgorm::tokio_postgres::types::to_sql_checked!();
        }

        impl<'__pgorm_a> ::pgorm::tokio_postgres::types::FromSql<'__pgorm_a> for #name {
            fn from_sql(
                _ty: &::pgorm::tokio_postgres::types::Type,
                raw: &'__pgorm_a [u8],
            ) -> ::std::result::Result<
                Self,
                ::std::boxed::Box<dyn ::std::error::Error + ::std::marker::Sync + ::std::marker::Send>,
            > {
                if raw.len() < 4 {
                    return Err("composite: insufficient data for field count".into());
                }

                let __pgorm_field_count = i32::from_be_bytes(raw[0..4].try_into().unwrap());
                if __pgorm_field_count != #field_count {
                    return Err(::std::format!(
                        "composite {}: expected {} fields, got {}",
                        #pg_type,
                        #field_count,
                        __pgorm_field_count,
                    ).into());
                }

                let mut __pgorm_pos: usize = 4;

                #(#decode_fields)*

                Ok(#name { #(#field_names),* })
            }

            fn accepts(ty: &::pgorm::tokio_postgres::types::Type) -> bool {
                ty.name() == #pg_type
            }
        }

        impl ::pgorm::PgType for #name {
            fn pg_array_type() -> &'static str {
                #pg_type_array
            }
        }
    };

    Ok(expanded)
}

/// Parse `#[orm(pg_type = "...")]` from the derive input attributes.
fn parse_pg_type(input: &DeriveInput) -> Result<String> {
    for attr in &input.attrs {
        if !attr.path().is_ident("orm") {
            continue;
        }
        let nested = attr.parse_args_with(
            syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
        )?;
        for meta in &nested {
            if let Meta::NameValue(nv) = meta {
                if nv.path.is_ident("pg_type") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = &nv.value
                    {
                        return Ok(s.value());
                    }
                }
            }
        }
    }
    Err(syn::Error::new_spanned(
        &input.ident,
        "PgComposite requires #[orm(pg_type = \"...\")] attribute",
    ))
}
