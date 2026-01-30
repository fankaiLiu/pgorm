//! Graph attribute parsing for multi-table writes.

use proc_macro2::{Span, TokenStream};
use syn::Result;

use super::graph_decl::{
    BelongsTo, BelongsToMode, GraphDeclarations, HasRelation, HasRelationMode, InsertStep, StepMode,
};

/// Parse a graph-style attribute like `has_many(Type, field = "x", fk_field = "y")`.
pub(super) fn parse_graph_attr(tokens: &TokenStream, graph: &mut GraphDeclarations) -> Result<()> {
    // Parse the tokens to get the attribute name and content
    let tokens_str = tokens.to_string();

    // Handle graph_root_id_field = "..." (new attribute per doc §5)
    if tokens_str.starts_with("graph_root_id_field") {
        if let Some(value) = extract_string_value(&tokens_str, "graph_root_id_field") {
            graph.graph_root_id_field = Some(value);
        }
        return Ok(());
    }

    // Handle deprecated graph_root_key = "..." (ignored, use graph_root_id_field instead)
    if tokens_str.starts_with("graph_root_key") && !tokens_str.starts_with("graph_root_key_source")
    {
        // Silently ignore - deprecated. Use graph_root_id_field for explicit input mode,
        // or rely on returning + ModelPk::pk() for automatic ID extraction
        return Ok(());
    }

    // Handle deprecated graph_root_key_source = "..." (ignored, Input is now the only mode for graph_root_id_field)
    if tokens_str.starts_with("graph_root_key_source") {
        // Silently ignore - the new behavior is: if graph_root_id_field is set, use Input mode;
        // otherwise use Returning mode (via ModelPk::pk())
        return Ok(());
    }

    // Handle has_one(...) / has_many(...)
    if tokens_str.starts_with("has_one") || tokens_str.starts_with("has_many") {
        let is_many = tokens_str.starts_with("has_many");
        if let Some(rel) = parse_has_relation(tokens, is_many)? {
            graph.has_relations.push(rel);
        }
        return Ok(());
    }

    // Handle belongs_to(...)
    if tokens_str.starts_with("belongs_to") {
        if let Some(bt) = parse_belongs_to(tokens)? {
            graph.belongs_to.push(bt);
        }
        return Ok(());
    }

    // Handle before_insert(...) / after_insert(...)
    if tokens_str.starts_with("before_insert") || tokens_str.starts_with("after_insert") {
        let is_before = tokens_str.starts_with("before_insert");
        if let Some(step) = parse_insert_step(tokens, is_before)? {
            graph.insert_steps.push(step);
        }
        return Ok(());
    }

    Ok(())
}

/// Extract a string value from a simple "key = \"value\"" pattern.
fn extract_string_value(s: &str, key: &str) -> Option<String> {
    let pattern = format!("{key} = ");
    if let Some(idx) = s.find(&pattern) {
        let rest = &s[idx + pattern.len()..];
        // Find the quoted value
        if let Some(start) = rest.find('"') {
            let rest = &rest[start + 1..];
            if let Some(end) = rest.find('"') {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

/// Parse has_one/has_many attribute content.
fn parse_has_relation(tokens: &TokenStream, is_many: bool) -> Result<Option<HasRelation>> {
    // Parse: has_one(Type, field = "x", fk_field = "y", mode = "insert")
    // or:    has_many(Type, field = "x", fk_field = "y", mode = "upsert")
    let parsed: HasRelationAttr = syn::parse2(tokens.clone())?;
    Ok(Some(HasRelation {
        child_type: parsed.child_type,
        field: parsed.field,
        fk_field: parsed.fk_field,
        is_many,
        mode: parsed.mode,
    }))
}

/// Parsed has_one/has_many attribute.
struct HasRelationAttr {
    child_type: syn::Path,
    field: String,
    fk_field: String,
    mode: HasRelationMode,
}

impl syn::parse::Parse for HasRelationAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        // Skip the function name (has_one or has_many)
        let _name: syn::Ident = input.parse()?;

        // Parse the parenthesized content
        let content;
        syn::parenthesized!(content in input);

        // First argument: the child type
        let child_type: syn::Path = content.parse()?;

        let mut field: Option<String> = None;
        let mut fk_field: Option<String> = None;
        let mut mode = HasRelationMode::Insert;

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
                "fk_field" => fk_field = Some(value.value()),
                "mode" => {
                    mode = match value.value().as_str() {
                        "insert" => HasRelationMode::Insert,
                        "upsert" => HasRelationMode::Upsert,
                        _ => {
                            return Err(syn::Error::new(
                                value.span(),
                                "mode must be \"insert\" or \"upsert\"",
                            ));
                        }
                    };
                }
                // fk_wrap is deprecated - now always use with_* setter
                "fk_wrap" => { /* ignored for backward compatibility */ }
                _ => {}
            }
        }

        let field = field.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "has_one/has_many requires field = \"...\"",
            )
        })?;
        let fk_field = fk_field.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "has_one/has_many requires fk_field = \"...\"",
            )
        })?;

        Ok(Self {
            child_type,
            field,
            fk_field,
            mode,
        })
    }
}

/// Parse belongs_to attribute content.
fn parse_belongs_to(tokens: &TokenStream) -> Result<Option<BelongsTo>> {
    let parsed: BelongsToAttr = syn::parse2(tokens.clone())?;
    Ok(Some(BelongsTo {
        parent_type: parsed.parent_type,
        field: parsed.field,
        set_fk_field: parsed.set_fk_field,
        mode: parsed.mode,
        required: parsed.required,
    }))
}

/// Parsed belongs_to attribute.
struct BelongsToAttr {
    parent_type: syn::Path,
    field: String,
    set_fk_field: String,
    mode: BelongsToMode,
    required: bool,
}

impl syn::parse::Parse for BelongsToAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        // Skip the function name
        let _name: syn::Ident = input.parse()?;

        // Parse the parenthesized content
        let content;
        syn::parenthesized!(content in input);

        // First argument: the parent type
        let parent_type: syn::Path = content.parse()?;

        let mut field: Option<String> = None;
        let mut set_fk_field: Option<String> = None;
        let mut mode = BelongsToMode::InsertReturning;
        let mut required = false;

        // Parse remaining key = "value" pairs
        while !content.is_empty() {
            let _: syn::Token![,] = content.parse()?;
            if content.is_empty() {
                break;
            }

            let key: syn::Ident = content.parse()?;
            let _: syn::Token![=] = content.parse()?;

            match key.to_string().as_str() {
                "field" => {
                    let value: syn::LitStr = content.parse()?;
                    field = Some(value.value());
                }
                "set_fk_field" => {
                    let value: syn::LitStr = content.parse()?;
                    set_fk_field = Some(value.value());
                }
                "mode" => {
                    let value: syn::LitStr = content.parse()?;
                    mode = match value.value().as_str() {
                        "insert_returning" => BelongsToMode::InsertReturning,
                        "upsert_returning" => BelongsToMode::UpsertReturning,
                        _ => {
                            return Err(syn::Error::new(
                                value.span(),
                                "mode must be \"insert_returning\" or \"upsert_returning\"",
                            ));
                        }
                    };
                }
                "required" => {
                    let value: syn::LitBool = content.parse()?;
                    required = value.value();
                }
                _ => {
                    // Skip unknown attributes (including deprecated referenced_id_field)
                    let _: syn::LitStr = content.parse()?;
                }
            }
        }

        let field = field.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "belongs_to requires field = \"...\"")
        })?;
        let set_fk_field = set_fk_field.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "belongs_to requires set_fk_field = \"...\"",
            )
        })?;

        Ok(Self {
            parent_type,
            field,
            set_fk_field,
            mode,
            required,
        })
    }
}

/// Parse before_insert/after_insert attribute content.
fn parse_insert_step(tokens: &TokenStream, is_before: bool) -> Result<Option<InsertStep>> {
    let parsed: InsertStepAttr = syn::parse2(tokens.clone())?;
    Ok(Some(InsertStep {
        step_type: parsed.step_type,
        field: parsed.field,
        mode: parsed.mode,
        is_before,
    }))
}

/// Parsed before_insert/after_insert attribute.
struct InsertStepAttr {
    step_type: syn::Path,
    field: String,
    mode: StepMode,
}

impl syn::parse::Parse for InsertStepAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        // Skip the function name
        let _name: syn::Ident = input.parse()?;

        // Parse the parenthesized content
        let content;
        syn::parenthesized!(content in input);

        // First argument: the step type
        let step_type: syn::Path = content.parse()?;

        let mut field: Option<String> = None;
        let mut mode = StepMode::Insert;

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
                "mode" => {
                    mode = match value.value().as_str() {
                        "insert" => StepMode::Insert,
                        "upsert" => StepMode::Upsert,
                        _ => {
                            return Err(syn::Error::new(
                                value.span(),
                                "mode must be \"insert\" or \"upsert\"",
                            ));
                        }
                    };
                }
                _ => {}
            }
        }

        let field = field.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "before_insert/after_insert requires field = \"...\"",
            )
        })?;

        Ok(Self {
            step_type,
            field,
            mode,
        })
    }
}
