//! FromRow derive macro implementation

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Result};

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "FromRow can only be derived for structs with named fields",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "FromRow can only be derived for structs",
            ))
        }
    };

    let field_extracts: Vec<_> = fields
        .iter()
        .map(|field| {
            let field_name = field.ident.as_ref().unwrap();
            let column_name = get_column_name(field);

            quote! {
                #field_name: row.try_get_column(#column_name)?
            }
        })
        .collect();

    Ok(quote! {
        impl #impl_generics pgorm::FromRow for #name #ty_generics #where_clause {
            fn from_row(row: &tokio_postgres::Row) -> pgorm::OrmResult<Self> {
                use pgorm::RowExt;
                Ok(Self {
                    #(#field_extracts),*
                })
            }
        }
    })
}

fn get_column_name(field: &syn::Field) -> String {
    for attr in &field.attrs {
        if attr.path().is_ident("orm") {
            if let Ok(nested) = attr.parse_args::<syn::MetaNameValue>() {
                if nested.path.is_ident("column") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(lit),
                        ..
                    }) = &nested.value
                    {
                        return lit.value();
                    }
                }
            }
        }
    }
    field.ident.as_ref().unwrap().to_string()
}
