//! HasMany/BelongsTo relation handling for Model derive macro.
//!
//! Parses `#[orm(has_many(...))]` and `#[orm(belongs_to(...))]` attributes.

use proc_macro2::Span;
use syn::ext::IdentExt;
use syn::{DeriveInput, Result};

/// Represents a has_many relationship: Parent has many Children
pub(super) struct HasManyRelation {
    /// The related model type (e.g., Review)
    pub model: syn::Path,
    /// The foreign key column in the child table (e.g., "product_id")
    pub foreign_key: String,
    /// The method name to generate (e.g., "reviews" -> select_reviews)
    pub method_name: String,
}

/// Represents a belongs_to relationship: Child belongs to Parent
pub(super) struct BelongsToRelation {
    /// The related model type (e.g., Category)
    pub model: syn::Path,
    /// The foreign key column in this table (e.g., "category_id")
    pub foreign_key: String,
    /// The method name to generate (e.g., "category" -> select_category)
    pub method_name: String,
}

/// Helper struct for parsing has_many attribute
struct HasManyAttr {
    model: syn::Path,
    foreign_key: String,
    method_name: String,
}

impl syn::parse::Parse for HasManyAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident != "has_many" {
            return Err(syn::Error::new(ident.span(), "expected has_many"));
        }

        let content;
        syn::parenthesized!(content in input);

        let model: syn::Path = content.parse()?;

        let mut foreign_key: Option<String> = None;
        let mut method_name: Option<String> = None;

        while content.peek(syn::Token![,]) {
            let _: syn::Token![,] = content.parse()?;
            if content.is_empty() {
                break;
            }

            // Use parse_any to handle keywords like 'as'
            let key = syn::Ident::parse_any(&content)?;
            let _: syn::Token![=] = content.parse()?;
            let value: syn::LitStr = content.parse()?;

            if key == "foreign_key" {
                foreign_key = Some(value.value());
            } else if key == "as" || key == "name" {
                method_name = Some(value.value());
            }
        }

        let fk = foreign_key.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "has_many requires foreign_key = \"...\"")
        })?;

        // Default method name: lowercase model name + 's'
        let name = method_name.unwrap_or_else(|| {
            let model_name = model.segments.last().unwrap().ident.to_string();
            format!("{}s", model_name.to_lowercase())
        });

        Ok(HasManyAttr {
            model,
            foreign_key: fk,
            method_name: name,
        })
    }
}

/// Helper struct for parsing belongs_to attribute
struct BelongsToAttr {
    model: syn::Path,
    foreign_key: String,
    method_name: String,
}

impl syn::parse::Parse for BelongsToAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident != "belongs_to" {
            return Err(syn::Error::new(ident.span(), "expected belongs_to"));
        }

        let content;
        syn::parenthesized!(content in input);

        let model: syn::Path = content.parse()?;

        let mut foreign_key: Option<String> = None;
        let mut method_name: Option<String> = None;

        while content.peek(syn::Token![,]) {
            let _: syn::Token![,] = content.parse()?;
            if content.is_empty() {
                break;
            }

            // Use parse_any to handle keywords like 'as'
            let key = syn::Ident::parse_any(&content)?;
            let _: syn::Token![=] = content.parse()?;
            let value: syn::LitStr = content.parse()?;

            if key == "foreign_key" {
                foreign_key = Some(value.value());
            } else if key == "as" || key == "name" {
                method_name = Some(value.value());
            }
        }

        let fk = foreign_key.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "belongs_to requires foreign_key = \"...\"",
            )
        })?;

        // Default method name: lowercase model name
        let name = method_name.unwrap_or_else(|| {
            let model_name = model.segments.last().unwrap().ident.to_string();
            model_name.to_lowercase()
        });

        Ok(BelongsToAttr {
            model,
            foreign_key: fk,
            method_name: name,
        })
    }
}

/// Parse has_many relations from struct attributes.
///
/// Example: `#[orm(has_many(Review, foreign_key = "product_id", as = "reviews"))]`
pub(super) fn get_has_many_relations(input: &DeriveInput) -> Result<Vec<HasManyRelation>> {
    let mut relations = Vec::new();

    for attr in &input.attrs {
        if attr.path().is_ident("orm") {
            // Try to parse as a function-style attribute: orm(has_many(...))
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<HasManyAttr>(tokens) {
                    relations.push(HasManyRelation {
                        model: parsed.model,
                        foreign_key: parsed.foreign_key,
                        method_name: parsed.method_name,
                    });
                }
            }
        }
    }

    Ok(relations)
}

/// Parse belongs_to relations from struct attributes.
///
/// Example: `#[orm(belongs_to(Category, foreign_key = "category_id", as = "category"))]`
pub(super) fn get_belongs_to_relations(input: &DeriveInput) -> Result<Vec<BelongsToRelation>> {
    let mut relations = Vec::new();

    for attr in &input.attrs {
        if attr.path().is_ident("orm") {
            // Try to parse as a function-style attribute: orm(belongs_to(...))
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<BelongsToAttr>(tokens) {
                    relations.push(BelongsToRelation {
                        model: parsed.model,
                        foreign_key: parsed.foreign_key,
                        method_name: parsed.method_name,
                    });
                }
            }
        }
    }

    Ok(relations)
}
