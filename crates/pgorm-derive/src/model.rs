//! Model derive macro implementation

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{Data, DeriveInput, Fields, Result};

/// Represents a has_many relationship: Parent has many Children
struct HasManyRelation {
    /// The related model type (e.g., Review)
    model: syn::Path,
    /// The foreign key column in the child table (e.g., "product_id")
    foreign_key: String,
    /// The method name to generate (e.g., "reviews" -> select_reviews)
    method_name: String,
}

/// Represents a belongs_to relationship: Child belongs to Parent
struct BelongsToRelation {
    /// The related model type (e.g., Category)
    model: syn::Path,
    /// The foreign key column in this table (e.g., "category_id")
    foreign_key: String,
    /// The method name to generate (e.g., "category" -> select_category)
    method_name: String,
}

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;

    let table_name = get_table_name(&input)?;
    let has_many_relations = get_has_many_relations(&input)?;
    let belongs_to_relations = get_belongs_to_relations(&input)?;

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

    let mut column_names = Vec::new();
    let mut id_column: Option<String> = None;
    let mut id_field_type: Option<&syn::Type> = None;
    let mut fk_fields: std::collections::HashMap<String, &syn::Type> = std::collections::HashMap::new();

    for field in fields.iter() {
        let field_name = field.ident.as_ref().unwrap().to_string();
        let column_name = get_column_name(field);
        column_names.push(column_name.clone());

        if is_id_field(field) {
            id_column = Some(column_name.clone());
            id_field_type = Some(&field.ty);
        }

        // Track all fields for belongs_to foreign key lookups
        fk_fields.insert(field_name, &field.ty);
        fk_fields.insert(column_name, &field.ty);
    }

    let select_list = column_names.join(", ");
    let id_const = if let Some(id) = &id_column {
        quote! { pub const ID: &'static str = #id; }
    } else {
        quote! {}
    };

    let column_names_for_alias = column_names.clone();

    // Generate select_one method only if there's an ID field
    let select_one_method = if let (Some(id_col), Some(id_ty)) = (&id_column, id_field_type) {
        quote! {
            /// Fetch a single record by its primary key.
            ///
            /// Returns `OrmError::NotFound` if no record is found.
            pub async fn select_one(
                conn: &impl pgorm::GenericClient,
                id: #id_ty,
            ) -> pgorm::OrmResult<Self>
            where
                Self: pgorm::FromRow,
            {
                let sql = format!(
                    "SELECT {} FROM {} WHERE {} = $1",
                    Self::SELECT_LIST,
                    Self::TABLE,
                    #id_col
                );
                let row = conn.query_one(&sql, &[&id]).await?;
                pgorm::FromRow::from_row(&row)
            }
        }
    } else {
        quote! {}
    };

    // Generate has_many methods (requires ID field)
    let has_many_methods: Vec<TokenStream> = if let Some(id_ty) = id_field_type {
        has_many_relations
            .iter()
            .map(|rel| {
                let method_name = format_ident!("select_{}", rel.method_name);
                let related_model = &rel.model;
                let fk = &rel.foreign_key;

                quote! {
                    /// Fetch all related records (has_many relationship).
                    pub async fn #method_name(
                        &self,
                        conn: &impl pgorm::GenericClient,
                    ) -> pgorm::OrmResult<Vec<#related_model>>
                    where
                        #related_model: pgorm::FromRow,
                        #id_ty: tokio_postgres::types::ToSql + Sync,
                    {
                        let sql = format!(
                            "SELECT {} FROM {} WHERE {} = $1",
                            #related_model::SELECT_LIST,
                            #related_model::TABLE,
                            #fk
                        );
                        let rows = conn.query(&sql, &[&self.id]).await?;
                        rows.iter().map(pgorm::FromRow::from_row).collect()
                    }
                }
            })
            .collect()
    } else {
        vec![]
    };

    // Generate belongs_to methods
    let belongs_to_methods: Vec<TokenStream> = belongs_to_relations
        .iter()
        .filter_map(|rel| {
            // Find the field type for the foreign key
            let fk_type = fk_fields.get(&rel.foreign_key)?;
            let method_name = format_ident!("select_{}", rel.method_name);
            let related_model = &rel.model;
            let fk_field = format_ident!("{}", rel.foreign_key.replace('-', "_"));

            Some(quote! {
                /// Fetch the related parent record (belongs_to relationship).
                pub async fn #method_name(
                    &self,
                    conn: &impl pgorm::GenericClient,
                ) -> pgorm::OrmResult<#related_model>
                where
                    #related_model: pgorm::FromRow,
                    #fk_type: tokio_postgres::types::ToSql + Sync,
                {
                    let sql = format!(
                        "SELECT {} FROM {} WHERE {} = $1",
                        #related_model::SELECT_LIST,
                        #related_model::TABLE,
                        #related_model::ID
                    );
                    let row = conn.query_one(&sql, &[&self.#fk_field]).await?;
                    pgorm::FromRow::from_row(&row)
                }
            })
        })
        .collect();

    Ok(quote! {
        impl #name {
            pub const TABLE: &'static str = #table_name;
            #id_const
            pub const SELECT_LIST: &'static str = #select_list;

            pub fn select_list_as(alias: &str) -> String {
                [#(#column_names_for_alias),*]
                    .iter()
                    .map(|col| format!("{}.{}", alias, col))
                    .collect::<Vec<_>>()
                    .join(", ")
            }

            /// Fetch all records from the table.
            pub async fn select_all(conn: &impl pgorm::GenericClient) -> pgorm::OrmResult<Vec<Self>>
            where
                Self: pgorm::FromRow,
            {
                let sql = format!("SELECT {} FROM {}", Self::SELECT_LIST, Self::TABLE);
                let rows = conn.query(&sql, &[]).await?;
                rows.iter().map(pgorm::FromRow::from_row).collect()
            }

            #select_one_method

            #(#has_many_methods)*

            #(#belongs_to_methods)*
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

/// Parse has_many relations from struct attributes.
/// Example: #[orm(has_many(Review, foreign_key = "product_id", as = "reviews"))]
fn get_has_many_relations(input: &DeriveInput) -> Result<Vec<HasManyRelation>> {
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
/// Example: #[orm(belongs_to(Category, foreign_key = "category_id", as = "category"))]
fn get_belongs_to_relations(input: &DeriveInput) -> Result<Vec<BelongsToRelation>> {
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
