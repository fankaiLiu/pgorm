//! Attribute parsing for InsertModel derive macro.

use proc_macro2::Span;
use quote::format_ident;
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
    pub(super) input: Option<InputConfig>,
}

#[derive(Clone)]
pub(super) struct InputConfig {
    pub(super) name: syn::Ident,
    pub(super) vis: syn::Visibility,
}

pub(super) struct StructAttrList {
    pub(super) table: Option<String>,
    pub(super) returning: Option<syn::Path>,
    pub(super) conflict_target: Option<Vec<String>>,
    pub(super) conflict_constraint: Option<String>,
    pub(super) conflict_update: Option<Vec<String>>,
    pub(super) graph_root_id_field: Option<String>,
    pub(super) input: bool,
    pub(super) input_name: Option<syn::Ident>,
    pub(super) input_vis: Option<syn::Visibility>,
}

impl syn::parse::Parse for StructAttrList {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let mut table: Option<String> = None;
        let mut returning: Option<syn::Path> = None;
        let mut conflict_target: Option<Vec<String>> = None;
        let mut conflict_constraint: Option<String> = None;
        let mut conflict_update: Option<Vec<String>> = None;
        let mut graph_root_id_field: Option<String> = None;
        let mut input_flag = false;
        let mut input_name: Option<syn::Ident> = None;
        let mut input_vis: Option<syn::Visibility> = None;

        loop {
            if input.is_empty() {
                break;
            }

            let ident: syn::Ident = input.parse()?;
            let key = ident.to_string();

            if input.peek(syn::token::Paren) {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "unexpected function-style attribute",
                ));
            }

            if input.peek(syn::Token![=]) {
                let _: syn::Token![=] = input.parse()?;
                let value: syn::LitStr = input.parse()?;

                match key.as_str() {
                    "table" => table = Some(value.value()),
                    "returning" => {
                        let ty: syn::Path = syn::parse_str(&value.value()).map_err(|e| {
                            syn::Error::new(
                                Span::call_site(),
                                format!("invalid returning type: {e}"),
                            )
                        })?;
                        returning = Some(ty);
                    }
                    "conflict_target" => {
                        conflict_target =
                            Some(parse_sql_ident_list(&value, "conflict_target", false)?);
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
                    "input" => {
                        input_flag = true;
                        let ty: syn::Ident = syn::parse_str(&value.value()).map_err(|e| {
                            syn::Error::new(Span::call_site(), format!("invalid input type: {e}"))
                        })?;
                        input_name = Some(ty);
                    }
                    "input_vis" => {
                        let vis: syn::Visibility = syn::parse_str(&value.value()).map_err(|e| {
                            syn::Error::new(
                                Span::call_site(),
                                format!("invalid input_vis: {e}"),
                            )
                        })?;
                        input_vis = Some(vis);
                    }
                    _ => {}
                }
            } else if key.as_str() == "input" {
                input_flag = true;
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
            input: input_flag,
            input_name,
            input_vis,
        })
    }
}

pub(super) struct FieldAttrs {
    pub(super) is_id: bool,
    pub(super) skip_insert: bool,
    pub(super) default: bool,
    pub(super) auto_now_add: bool,
    pub(super) table: Option<String>,
    pub(super) column: Option<String>,
    pub(super) skip_input: bool,
    pub(super) input_as: Option<syn::Type>,
    // Validation rules (used by generated Input structs)
    pub(super) required: bool,
    pub(super) len: Option<String>,
    pub(super) range: Option<String>,
    pub(super) email: bool,
    pub(super) regex: Option<String>,
    pub(super) url: bool,
    pub(super) uuid: bool,
    pub(super) one_of: Option<String>,
    pub(super) custom: Option<syn::Path>,
}

impl syn::parse::Parse for FieldAttrs {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let mut attrs = FieldAttrs {
            is_id: false,
            skip_insert: false,
            default: false,
            auto_now_add: false,
            table: None,
            column: None,
            skip_input: false,
            input_as: None,
            required: false,
            len: None,
            range: None,
            email: false,
            regex: None,
            url: false,
            uuid: false,
            one_of: None,
            custom: None,
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
                "auto_now_add" => attrs.auto_now_add = true,
                "skip_input" => attrs.skip_input = true,
                "required" => attrs.required = true,
                "email" => attrs.email = true,
                "url" => attrs.url = true,
                "uuid" => attrs.uuid = true,
                _ => {
                    let _: syn::Token![=] = input.parse()?;
                    let value: syn::LitStr = input.parse()?;
                    match key.as_str() {
                        "table" => attrs.table = Some(value.value()),
                        "column" => attrs.column = Some(value.value()),
                        "input_as" => {
                            let ty: syn::Type = syn::parse_str(&value.value()).map_err(|e| {
                                syn::Error::new(
                                    Span::call_site(),
                                    format!("invalid input_as type: {e}"),
                                )
                            })?;
                            attrs.input_as = Some(ty);
                        }
                        "len" => attrs.len = Some(value.value()),
                        "range" => attrs.range = Some(value.value()),
                        "regex" => attrs.regex = Some(value.value()),
                        "one_of" => attrs.one_of = Some(value.value()),
                        "custom" => {
                            let path: syn::Path = syn::parse_str(&value.value()).map_err(|e| {
                                syn::Error::new(
                                    Span::call_site(),
                                    format!("invalid custom path: {e}"),
                                )
                            })?;
                            attrs.custom = Some(path);
                        }
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
        auto_now_add: false,
        table: None,
        column: None,
        skip_input: false,
        input_as: None,
        required: false,
        len: None,
        range: None,
        email: false,
        regex: None,
        url: false,
        uuid: false,
        one_of: None,
        custom: None,
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
            merged.auto_now_add |= parsed.auto_now_add;
            merged.skip_input |= parsed.skip_input;
            merged.required |= parsed.required;
            merged.email |= parsed.email;
            merged.url |= parsed.url;
            merged.uuid |= parsed.uuid;
            if parsed.table.is_some() {
                merged.table = parsed.table;
            }
            if parsed.column.is_some() {
                merged.column = parsed.column;
            }
            if let Some(ty) = parsed.input_as {
                if merged.input_as.is_some() {
                    return Err(syn::Error::new_spanned(field, "duplicate input_as"));
                }
                merged.input_as = Some(ty);
            }
            if let Some(v) = parsed.len {
                if merged.len.is_some() {
                    return Err(syn::Error::new_spanned(field, "duplicate len"));
                }
                merged.len = Some(v);
            }
            if let Some(v) = parsed.range {
                if merged.range.is_some() {
                    return Err(syn::Error::new_spanned(field, "duplicate range"));
                }
                merged.range = Some(v);
            }
            if let Some(v) = parsed.regex {
                if merged.regex.is_some() {
                    return Err(syn::Error::new_spanned(field, "duplicate regex"));
                }
                merged.regex = Some(v);
            }
            if let Some(v) = parsed.one_of {
                if merged.one_of.is_some() {
                    return Err(syn::Error::new_spanned(field, "duplicate one_of"));
                }
                merged.one_of = Some(v);
            }
            if let Some(v) = parsed.custom {
                if merged.custom.is_some() {
                    return Err(syn::Error::new_spanned(field, "duplicate custom"));
                }
                merged.custom = Some(v);
            }
        }
    }

    // Validate conflicts
    if merged.auto_now_add && merged.skip_insert {
        return Err(syn::Error::new_spanned(
            field,
            "auto_now_add and skip_insert are mutually exclusive",
        ));
    }
    if merged.auto_now_add && merged.default {
        return Err(syn::Error::new_spanned(
            field,
            "auto_now_add and default are mutually exclusive",
        ));
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
    let mut input_enabled = false;
    let mut input_name: Option<syn::Ident> = None;
    let mut input_vis: Option<syn::Visibility> = None;

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
                if parsed.input {
                    input_enabled = true;
                }
                if parsed.input_name.is_some() {
                    input_name = parsed.input_name;
                }
                if parsed.input_vis.is_some() {
                    input_vis = parsed.input_vis;
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
        input: if input_enabled {
            let name = input_name.unwrap_or_else(|| format_ident!("{}Input", input.ident));
            let vis = input_vis
                .unwrap_or_else(|| syn::parse_str::<syn::Visibility>("pub").expect("valid vis"));
            Some(InputConfig { name, vis })
        } else {
            None
        },
    })
}
