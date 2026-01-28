//! Model derive macro implementation

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Result};

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;

    let table_name = get_table_name(&input)?;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "Model can only be derived for structs with named fields",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "Model can only be derived for structs",
            ))
        }
    };

    let mut column_consts = Vec::new();
    let mut column_names = Vec::new();
    let mut id_column: Option<String> = None;

    for field in fields.iter() {
        let field_name = field.ident.as_ref().unwrap();
        let column_name = get_column_name(field);
        let const_name = format_ident!("COL_{}", field_name.to_string().to_uppercase());

        column_consts.push(quote! {
            pub const #const_name: &'static str = #column_name;
        });
        column_names.push(column_name.clone());

        if is_id_field(field) {
            id_column = Some(column_name);
        }
    }

    let select_list = column_names.join(", ");
    let id_const = if let Some(id) = &id_column {
        quote! { pub const ID: &'static str = #id; }
    } else {
        quote! {}
    };

    let column_names_for_alias = column_names.clone();

    Ok(quote! {
        impl #name {
            pub const TABLE: &'static str = #table_name;
            #id_const
            #(#column_consts)*
            pub const SELECT_LIST: &'static str = #select_list;

            pub fn select_list_as(alias: &str) -> String {
                [#(#column_names_for_alias),*]
                    .iter()
                    .map(|col| format!("{}.{}", alias, col))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        }
    })
}

fn get_table_name(input: &DeriveInput) -> Result<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("orm") {
            if let Ok(nested) = attr.parse_args::<syn::MetaNameValue>() {
                if nested.path.is_ident("table") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(lit),
                        ..
                    }) = &nested.value
                    {
                        return Ok(lit.value());
                    }
                }
            }
        }
    }
    Err(syn::Error::new_spanned(
        input,
        "Model requires #[orm(table = \"table_name\")] attribute",
    ))
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

fn is_id_field(field: &syn::Field) -> bool {
    for attr in &field.attrs {
        if attr.path().is_ident("orm") {
            if let Ok(path) = attr.parse_args::<syn::Path>() {
                if path.is_ident("id") {
                    return true;
                }
            }
        }
    }
    false
}
