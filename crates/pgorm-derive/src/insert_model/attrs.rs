//! Attribute parsing for InsertModel derive macro.

use proc_macro2::Span;
use syn::Result;

use crate::sql_ident::{parse_sql_ident, parse_sql_ident_list};

use super::graph_decl::GraphDeclarations;

pub(super) struct StructAttrs {
    pub(super) table: String,
    pub(super) returning: Option<syn::Path>,
    pub(super) conflict_target: Option<Vec<String>>,
    pub(super) conflict_constraint: Option<String>,
    pub(super) conflict_update: Option<Vec<String>>,
    pub(super) graph: GraphDeclarations,
}

pub(super) struct StructAttrList {
    pub(super) table: Option<String>,
    pub(super) returning: Option<syn::Path>,
    pub(super) conflict_target: Option<Vec<String>>,
    pub(super) conflict_constraint: Option<String>,
    pub(super) conflict_update: Option<Vec<String>>,
    pub(super) graph_root_id_field: Option<String>,
}

impl syn::parse::Parse for StructAttrList {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let mut table: Option<String> = None;
        let mut returning: Option<syn::Path> = None;
        let mut conflict_target: Option<Vec<String>> = None;
        let mut conflict_constraint: Option<String> = None;
        let mut conflict_update: Option<Vec<String>> = None;
        let mut graph_root_id_field: Option<String> = None;

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
                "conflict_target" => {
                    conflict_target = Some(parse_sql_ident_list(&value, "conflict_target", false)?);
                }
                "conflict_constraint" => {
                    conflict_constraint = Some(parse_sql_ident(&value, "conflict_constraint")?);
                }
                "conflict_update" => {
                    conflict_update = Some(parse_sql_ident_list(&value, "conflict_update", true)?);
                }
                "graph_root_id_field" => {
                    graph_root_id_field = Some(value.value());
                }
                "graph_root_key" => {
                    // Deprecated and ignored - use graph_root_id_field for explicit input mode,
                    // or rely on returning + ModelPk::pk() for automatic ID extraction
                }
                "graph_root_key_source" => {
                    // Deprecated and ignored - the new behavior uses graph_root_id_field directly
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
            returning,
            conflict_target,
            conflict_constraint,
            conflict_update,
            graph_root_id_field,
        })
    }
}

pub(super) struct FieldAttrs {
    pub(super) is_id: bool,
    pub(super) skip_insert: bool,
    pub(super) default: bool,
    pub(super) table: Option<String>,
    pub(super) column: Option<String>,
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

pub(super) fn get_field_attrs(field: &syn::Field) -> Result<FieldAttrs> {
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

pub(super) fn get_struct_attrs(input: &syn::DeriveInput) -> Result<StructAttrs> {
    use super::graph_parse::parse_graph_attr;

    let mut table: Option<String> = None;
    let mut returning: Option<syn::Path> = None;
    let mut conflict_target: Option<Vec<String>> = None;
    let mut conflict_constraint: Option<String> = None;
    let mut conflict_update: Option<Vec<String>> = None;
    let mut graph = GraphDeclarations::default();

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
                if parsed.returning.is_some() {
                    returning = parsed.returning;
                }
                if parsed.conflict_target.is_some() {
                    conflict_target = parsed.conflict_target;
                }
                if parsed.conflict_constraint.is_some() {
                    conflict_constraint = parsed.conflict_constraint;
                }
                if parsed.conflict_update.is_some() {
                    conflict_update = parsed.conflict_update;
                }
                if parsed.graph_root_id_field.is_some() {
                    graph.graph_root_id_field = parsed.graph_root_id_field;
                }
                continue;
            }

            // Try to parse as graph declarations (function-style attributes)
            parse_graph_attr(&meta_list.tokens, &mut graph)?;
        }
    }

    let table = table.ok_or_else(|| {
        syn::Error::new_spanned(
            input,
            "InsertModel requires #[orm(table = \"table_name\")] attribute",
        )
    })?;

    // Validate: conflict_target and conflict_constraint are mutually exclusive
    if conflict_target.is_some() && conflict_constraint.is_some() {
        return Err(syn::Error::new_spanned(
            input,
            "conflict_target and conflict_constraint are mutually exclusive; use only one",
        ));
    }

    Ok(StructAttrs {
        table,
        returning,
        conflict_target,
        conflict_constraint,
        conflict_update,
        graph,
    })
}
