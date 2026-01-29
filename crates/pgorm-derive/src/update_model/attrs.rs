//! Attribute parsing for UpdateModel derive macro.

use proc_macro2::Span;
use syn::Result;

use crate::sql_ident::parse_sql_ident;

use super::graph_decl::UpdateGraphDeclarations;
use super::graph_parse::parse_update_graph_attr;

pub(super) struct StructAttrs {
    pub(super) table: String,
    pub(super) id_column: Option<String>,
    pub(super) model: Option<syn::Path>,
    pub(super) returning: Option<syn::Path>,
    pub(super) graph: UpdateGraphDeclarations,
}

pub(super) struct StructAttrList {
    pub(super) table: Option<String>,
    pub(super) id_column: Option<String>,
    pub(super) model: Option<syn::Path>,
    pub(super) returning: Option<syn::Path>,
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
                "id_column" => id_column = Some(parse_sql_ident(&value, "id_column")?),
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

pub(super) struct FieldAttrs {
    pub(super) skip_update: bool,
    pub(super) default: bool,
    pub(super) auto_now: bool,
    pub(super) table: Option<String>,
    pub(super) column: Option<String>,
}

impl syn::parse::Parse for FieldAttrs {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let mut attrs = FieldAttrs {
            skip_update: false,
            default: false,
            auto_now: false,
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
                "auto_now" => attrs.auto_now = true,
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

pub(super) fn get_field_attrs(field: &syn::Field) -> Result<FieldAttrs> {
    let mut merged = FieldAttrs {
        skip_update: false,
        default: false,
        auto_now: false,
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
            merged.auto_now |= parsed.auto_now;
            if parsed.table.is_some() {
                merged.table = parsed.table;
            }
            if parsed.column.is_some() {
                merged.column = parsed.column;
            }
        }
    }

    // Validate conflicts
    if merged.auto_now && merged.skip_update {
        return Err(syn::Error::new_spanned(
            field,
            "auto_now and skip_update are mutually exclusive",
        ));
    }
    if merged.auto_now && merged.default {
        return Err(syn::Error::new_spanned(
            field,
            "auto_now and default are mutually exclusive",
        ));
    }

    Ok(merged)
}

pub(super) fn get_struct_attrs(input: &syn::DeriveInput) -> Result<StructAttrs> {
    let mut table: Option<String> = None;
    let mut id_column: Option<String> = None;
    let mut model: Option<syn::Path> = None;
    let mut returning: Option<syn::Path> = None;
    let mut graph = UpdateGraphDeclarations::default();

    for attr in &input.attrs {
        if !attr.path().is_ident("orm") {
            continue;
        }
        if let syn::Meta::List(meta_list) = &attr.meta {
            // Try to parse as simple key=value attributes first
            if let Ok(parsed) = syn::parse2::<StructAttrList>(meta_list.tokens.clone()) {
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
                continue;
            }

            // Try to parse as graph declarations
            parse_update_graph_attr(&meta_list.tokens, &mut graph)?;
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
        graph,
    })
}
