//! Graph attribute parsing for multi-table update writes.

use proc_macro2::{Span, TokenStream};
use syn::Result;

use crate::sql_ident::{parse_sql_ident, parse_sql_ident_list};

use super::graph_decl::{HasManyUpdate, HasOneUpdate, UpdateGraphDeclarations, UpdateStrategy};

/// Parse graph-style attributes for UpdateModel.
pub(super) fn parse_update_graph_attr(
    tokens: &TokenStream,
    graph: &mut UpdateGraphDeclarations,
) -> Result<()> {
    let tokens_str = tokens.to_string();

    // Handle has_many_update(...)
    if tokens_str.starts_with("has_many_update") {
        if let Some(rel) = parse_has_many_update(tokens)? {
            graph.has_many.push(rel);
        }
        return Ok(());
    }

    // Handle has_one_update(...)
    if tokens_str.starts_with("has_one_update") {
        if let Some(rel) = parse_has_one_update(tokens)? {
            graph.has_one.push(rel);
        }
        return Ok(());
    }

    Ok(())
}

/// Parse has_many_update attribute content.
fn parse_has_many_update(tokens: &TokenStream) -> Result<Option<HasManyUpdate>> {
    let parsed: HasManyUpdateAttr = syn::parse2(tokens.clone())?;
    Ok(Some(HasManyUpdate {
        child_type: parsed.child_type,
        field: parsed.field,
        fk_column: parsed.fk_column,
        fk_field: parsed.fk_field,
        strategy: parsed.strategy,
        key_columns: parsed.key_columns,
    }))
}

/// Parsed has_many_update attribute.
struct HasManyUpdateAttr {
    child_type: syn::Path,
    field: String,
    fk_column: String,
    fk_field: String,
    strategy: UpdateStrategy,
    key_columns: Option<Vec<String>>,
}

impl syn::parse::Parse for HasManyUpdateAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        // Skip the function name
        let _name: syn::Ident = input.parse()?;

        // Parse the parenthesized content
        let content;
        syn::parenthesized!(content in input);

        // First argument: the child type
        let child_type: syn::Path = content.parse()?;

        let mut field: Option<String> = None;
        let mut fk_column: Option<String> = None;
        let mut fk_field: Option<String> = None;
        let mut strategy = UpdateStrategy::Replace;
        let mut key_columns: Option<Vec<String>> = None;

        // Parse remaining key = "value" pairs
        while !content.is_empty() {
            let _: syn::Token![,] = content.parse()?;
            if content.is_empty() {
                break;
            }

            let key: syn::Ident = content.parse()?;
            let _: syn::Token![=] = content.parse()?;
            let value: syn::LitStr = content.parse()?;

            match key.to_string().as_str() {
                "field" => field = Some(value.value()),
                "fk_column" => fk_column = Some(parse_sql_ident(&value, "fk_column")?),
                "fk_field" => fk_field = Some(value.value()),
                // fk_wrap is deprecated - now always use with_* setter
                "fk_wrap" => { /* ignored for backward compatibility */ }
                "strategy" => {
                    strategy = match value.value().as_str() {
                        "replace" => UpdateStrategy::Replace,
                        "append" => UpdateStrategy::Append,
                        "upsert" => UpdateStrategy::Upsert,
                        "diff" => UpdateStrategy::Diff,
                        _ => {
                            return Err(syn::Error::new(
                                value.span(),
                                "strategy must be \"replace\", \"append\", \"upsert\", or \"diff\"",
                            ));
                        }
                    };
                }
                "key_columns" => {
                    key_columns = Some(parse_sql_ident_list(&value, "key_columns", false)?);
                }
                // Legacy key_field/key_column are ignored (replaced by key_columns)
                "key_field" | "key_column" => { /* ignored for backward compatibility */ }
                _ => {}
            }
        }

        let field = field.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "has_many_update requires field = \"...\"",
            )
        })?;
        let fk_column = fk_column.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "has_many_update requires fk_column = \"...\"",
            )
        })?;
        let fk_field = fk_field.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "has_many_update requires fk_field = \"...\"",
            )
        })?;

        // Validate diff strategy requires key_columns
        if strategy == UpdateStrategy::Diff && key_columns.is_none() {
            return Err(syn::Error::new(
                Span::call_site(),
                "has_many_update with strategy=\"diff\" requires key_columns = \"...\"",
            ));
        }

        Ok(Self {
            child_type,
            field,
            fk_column,
            fk_field,
            strategy,
            key_columns,
        })
    }
}

/// Parse has_one_update attribute content.
fn parse_has_one_update(tokens: &TokenStream) -> Result<Option<HasOneUpdate>> {
    let parsed: HasOneUpdateAttr = syn::parse2(tokens.clone())?;
    Ok(Some(HasOneUpdate {
        child_type: parsed.child_type,
        field: parsed.field,
        fk_column: parsed.fk_column,
        fk_field: parsed.fk_field,
        strategy: parsed.strategy,
    }))
}

/// Parsed has_one_update attribute.
struct HasOneUpdateAttr {
    child_type: syn::Path,
    field: String,
    fk_column: String,
    fk_field: String,
    strategy: UpdateStrategy,
}

impl syn::parse::Parse for HasOneUpdateAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        // Skip the function name
        let _name: syn::Ident = input.parse()?;

        // Parse the parenthesized content
        let content;
        syn::parenthesized!(content in input);

        // First argument: the child type
        let child_type: syn::Path = content.parse()?;

        let mut field: Option<String> = None;
        let mut fk_column: Option<String> = None;
        let mut fk_field: Option<String> = None;
        let mut strategy = UpdateStrategy::Replace;

        // Parse remaining key = "value" pairs
        while !content.is_empty() {
            let _: syn::Token![,] = content.parse()?;
            if content.is_empty() {
                break;
            }

            let key: syn::Ident = content.parse()?;
            let _: syn::Token![=] = content.parse()?;
            let value: syn::LitStr = content.parse()?;

            match key.to_string().as_str() {
                "field" => field = Some(value.value()),
                "fk_column" => fk_column = Some(parse_sql_ident(&value, "fk_column")?),
                "fk_field" => fk_field = Some(value.value()),
                // fk_wrap is deprecated - now always use with_* setter
                "fk_wrap" => { /* ignored for backward compatibility */ }
                "strategy" => {
                    strategy = match value.value().as_str() {
                        "replace" => UpdateStrategy::Replace,
                        "upsert" => UpdateStrategy::Upsert,
                        _ => {
                            return Err(syn::Error::new(
                                value.span(),
                                "has_one_update strategy must be \"replace\" or \"upsert\"",
                            ));
                        }
                    };
                }
                _ => {}
            }
        }

        let field = field.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "has_one_update requires field = \"...\"")
        })?;
        let fk_column = fk_column.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "has_one_update requires fk_column = \"...\"",
            )
        })?;
        let fk_field = fk_field.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "has_one_update requires fk_field = \"...\"",
            )
        })?;

        Ok(Self {
            child_type,
            field,
            fk_column,
            fk_field,
            strategy,
        })
    }
}
