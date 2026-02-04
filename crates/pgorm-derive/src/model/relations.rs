//! Relation handling for Model derive macro.
//!
//! Parses:
//! - `#[orm(has_many(...))]`
//! - `#[orm(has_one(...))]`
//! - `#[orm(belongs_to(...))]`
//! - `#[orm(many_to_many(...))]`

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

/// Represents a has_one relationship: Parent has one Child (0..1 by default).
pub(super) struct HasOneRelation {
    /// The related model type (e.g., Profile)
    pub model: syn::Path,
    /// The foreign key column in the child table (e.g., "user_id")
    pub foreign_key: String,
    /// The method name to generate (e.g., "profile" -> load_profile_map)
    pub method_name: String,
}

/// Represents a many_to_many relationship: Parent has many Children through a join table.
pub(super) struct ManyToManyRelation {
    /// The related model type (e.g., Tag)
    pub model: syn::Path,
    /// Join table name (e.g., "post_tags")
    pub through: String,
    /// Join table column that references Self (e.g., "post_id")
    pub self_key: String,
    /// Join table column that references the other model (e.g., "tag_id")
    pub other_key: String,
    /// The method name to generate (e.g., "tags" -> load_tags_map)
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

/// Helper struct for parsing has_one attribute
struct HasOneAttr {
    model: syn::Path,
    foreign_key: String,
    method_name: String,
}

/// Helper struct for parsing many_to_many attribute
struct ManyToManyAttr {
    model: syn::Path,
    through: String,
    self_key: String,
    other_key: String,
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

impl syn::parse::Parse for HasOneAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident != "has_one" {
            return Err(syn::Error::new(ident.span(), "expected has_one"));
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
            syn::Error::new(Span::call_site(), "has_one requires foreign_key = \"...\"")
        })?;

        // Default method name: lowercase model name
        let name = method_name.unwrap_or_else(|| {
            let model_name = model.segments.last().unwrap().ident.to_string();
            model_name.to_lowercase()
        });

        Ok(HasOneAttr {
            model,
            foreign_key: fk,
            method_name: name,
        })
    }
}

impl syn::parse::Parse for ManyToManyAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident != "many_to_many" {
            return Err(syn::Error::new(ident.span(), "expected many_to_many"));
        }

        let content;
        syn::parenthesized!(content in input);

        let model: syn::Path = content.parse()?;

        let mut through: Option<String> = None;
        let mut self_key: Option<String> = None;
        let mut other_key: Option<String> = None;
        let mut method_name: Option<String> = None;

        while content.peek(syn::Token![,]) {
            let _: syn::Token![,] = content.parse()?;
            if content.is_empty() {
                break;
            }

            let key = syn::Ident::parse_any(&content)?;
            let _: syn::Token![=] = content.parse()?;
            let value: syn::LitStr = content.parse()?;

            if key == "through" {
                through = Some(value.value());
            } else if key == "self_key" {
                self_key = Some(value.value());
            } else if key == "other_key" {
                other_key = Some(value.value());
            } else if key == "as" || key == "name" {
                method_name = Some(value.value());
            }
        }

        let through = through.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "many_to_many requires through = \"...\"")
        })?;
        let self_key = self_key.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "many_to_many requires self_key = \"...\"")
        })?;
        let other_key = other_key.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "many_to_many requires other_key = \"...\"")
        })?;

        // Default method name: lowercase model name + 's'
        let name = method_name.unwrap_or_else(|| {
            let model_name = model.segments.last().unwrap().ident.to_string();
            format!("{}s", model_name.to_lowercase())
        });

        Ok(ManyToManyAttr {
            model,
            through,
            self_key,
            other_key,
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

/// Parse has_one relations from struct attributes.
///
/// Example: `#[orm(has_one(Profile, foreign_key = "user_id", as = "profile"))]`
pub(super) fn get_has_one_relations(input: &DeriveInput) -> Result<Vec<HasOneRelation>> {
    let mut relations = Vec::new();

    for attr in &input.attrs {
        if attr.path().is_ident("orm") {
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<HasOneAttr>(tokens) {
                    relations.push(HasOneRelation {
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

/// Parse many_to_many relations from struct attributes.
///
/// Example:
/// `#[orm(many_to_many(Tag, through = "post_tags", self_key = "post_id", other_key = "tag_id", as = "tags"))]`
pub(super) fn get_many_to_many_relations(input: &DeriveInput) -> Result<Vec<ManyToManyRelation>> {
    let mut relations = Vec::new();

    for attr in &input.attrs {
        if attr.path().is_ident("orm") {
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<ManyToManyAttr>(tokens) {
                    relations.push(ManyToManyRelation {
                        model: parsed.model,
                        through: parsed.through,
                        self_key: parsed.self_key,
                        other_key: parsed.other_key,
                        method_name: parsed.method_name,
                    });
                }
            }
        }
    }

    Ok(relations)
}
