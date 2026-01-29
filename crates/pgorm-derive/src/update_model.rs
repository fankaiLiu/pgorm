//! UpdateModel derive macro implementation

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Graph Declarations for UpdateModel (child table strategies)
// ─────────────────────────────────────────────────────────────────────────────

/// Strategy for updating has_many children.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum UpdateStrategy {
    /// Delete all old children, insert new ones.
    #[default]
    Replace,
    /// Only insert new children (don't delete old ones).
    Append,
    /// Upsert children (ON CONFLICT DO UPDATE).
    Upsert,
    /// Upsert + delete children not in the new list (sync to exact list).
    Diff,
}

/// has_many_update declaration.
#[derive(Clone)]
struct HasManyUpdate {
    /// The child InsertModel type.
    child_type: syn::Path,
    /// The Rust field name on this struct.
    field: String,
    /// The SQL column name for the foreign key.
    fk_column: String,
    /// The child's foreign key field name.
    fk_field: String,
    /// Update strategy.
    strategy: UpdateStrategy,
    /// For diff strategy: the key columns (comma-separated SQL column names).
    key_columns: Option<String>,
}

/// has_one_update declaration.
#[derive(Clone)]
struct HasOneUpdate {
    /// The child InsertModel type.
    child_type: syn::Path,
    /// The Rust field name on this struct.
    field: String,
    /// The SQL column name for the foreign key.
    fk_column: String,
    /// The child's foreign key field name.
    fk_field: String,
    /// Strategy: replace or upsert.
    strategy: UpdateStrategy,
}

/// All graph declarations for an UpdateModel.
#[derive(Clone, Default)]
struct UpdateGraphDeclarations {
    /// has_many_update relations.
    has_many: Vec<HasManyUpdate>,
    /// has_one_update relations.
    has_one: Vec<HasOneUpdate>,
}

impl UpdateGraphDeclarations {
    fn has_any(&self) -> bool {
        !self.has_many.is_empty() || !self.has_one.is_empty()
    }

    /// Get all field names that are used by graph declarations.
    fn graph_field_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for rel in &self.has_many {
            names.push(rel.field.clone());
        }
        for rel in &self.has_one {
            names.push(rel.field.clone());
        }
        names
    }
}

struct StructAttrs {
    table: String,
    id_column: Option<String>,
    model: Option<syn::Path>,
    returning: Option<syn::Path>,
    graph: UpdateGraphDeclarations,
}

struct StructAttrList {
    table: Option<String>,
    id_column: Option<String>,
    model: Option<syn::Path>,
    returning: Option<syn::Path>,
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
                "id_column" => id_column = Some(value.value()),
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

struct FieldAttrs {
    skip_update: bool,
    default: bool,
    table: Option<String>,
    column: Option<String>,
}

impl syn::parse::Parse for FieldAttrs {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let mut attrs = FieldAttrs {
            skip_update: false,
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
                "skip_update" => attrs.skip_update = true,
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

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let attrs = get_struct_attrs(&input)?;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "UpdateModel can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "UpdateModel can only be derived for structs",
            ));
        }
    };

    let id_col_expr = if let Some(model_ty) = attrs.model.as_ref() {
        quote! { #model_ty::ID }
    } else if let Some(id_col) = attrs.id_column.as_ref() {
        quote! { #id_col }
    } else if let Some(returning_ty) = attrs.returning.as_ref() {
        quote! { #returning_ty::ID }
    } else {
        return Err(syn::Error::new_spanned(
            &input,
            "UpdateModel requires #[orm(id_column = \"...\")] or #[orm(model = \"...\")] (or a returning type with ID)",
        ));
    };

    let mut destructure_idents: Vec<syn::Ident> = Vec::new();
    let mut set_stmts: Vec<TokenStream> = Vec::new();

    // Get field names used by graph declarations
    let graph_field_names = attrs.graph.graph_field_names();

    for field in fields.iter() {
        let field_ident = field.ident.clone().unwrap();
        let field_name = field_ident.to_string();
        let field_ty = &field.ty;

        // Skip fields used by graph declarations
        if graph_field_names.contains(&field_name) {
            continue;
        }

        let field_attrs = get_field_attrs(field)?;

        if let Some(field_table) = &field_attrs.table {
            if field_table != &attrs.table {
                return Err(syn::Error::new_spanned(
                    field,
                    "UpdateModel does not support fields from joined/other tables",
                ));
            }
        }

        if field_attrs.skip_update {
            continue;
        }

        let column_name = field_attrs.column.unwrap_or(field_name);

        if field_attrs.default {
            set_stmts.push(quote! {
                if !first {
                    q.push(", ");
                } else {
                    first = false;
                }
                q.push(#column_name);
                q.push(" = DEFAULT");
            });
            continue;
        }

        // Non-default fields need the value.
        destructure_idents.push(field_ident.clone());

        if let Some(inner) = option_inner(field_ty) {
            if option_inner(inner).is_some() {
                // Option<Option<T>>: Some(Some(v)) => bind; Some(None) => NULL; None => skip.
                set_stmts.push(quote! {
                    if let Some(v) = #field_ident {
                        if !first {
                            q.push(", ");
                        } else {
                            first = false;
                        }
                        q.push(#column_name);
                        q.push(" = ");
                        match v {
                            Some(vv) => {
                                q.push_bind(vv);
                            }
                            None => {
                                q.push("NULL");
                            }
                        }
                    }
                });
            } else {
                // Option<T>: Some(v) => bind; None => skip.
                set_stmts.push(quote! {
                    if let Some(v) = #field_ident {
                        if !first {
                            q.push(", ");
                        } else {
                            first = false;
                        }
                        q.push(#column_name);
                        q.push(" = ");
                        q.push_bind(v);
                    }
                });
            }
        } else {
            // T: always bind.
            set_stmts.push(quote! {
                if !first {
                    q.push(", ");
                } else {
                    first = false;
                }
                q.push(#column_name);
                q.push(" = ");
                q.push_bind(#field_ident);
            });
        }
    }

    let table_name = &attrs.table;

    let destructure = if destructure_idents.is_empty() {
        quote! { let _ = self; }
    } else {
        quote! { let Self { #(#destructure_idents),*, .. } = self; }
    };

    let update_by_id_method = quote! {
        /// Update columns by primary key (patch-style).
        pub async fn update_by_id<I>(
            self,
            conn: &impl pgorm::GenericClient,
            id: I,
        ) -> pgorm::OrmResult<u64>
        where
            I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
        {
            #destructure

            let mut q = pgorm::sql("UPDATE ");
            q.push(#table_name);
            q.push(" SET ");

            let mut first = true;
            #(#set_stmts)*

            if first {
                return Err(pgorm::OrmError::Validation(
                    "UpdateModel: no fields to update".to_string(),
                ));
            }

            q.push(" WHERE ");
            q.push(#table_name);
            q.push(".");
            q.push(#id_col_expr);
            q.push(" = ");
            q.push_bind(id);

            q.execute(conn).await
        }
    };

    let update_by_ids_method = quote! {
        /// Update columns by primary key for multiple rows (patch-style).
        ///
        /// The same patch is applied to every matched row.
        pub async fn update_by_ids<I>(
            self,
            conn: &impl pgorm::GenericClient,
            ids: ::std::vec::Vec<I>,
        ) -> pgorm::OrmResult<u64>
        where
            I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
        {
            if ids.is_empty() {
                return ::std::result::Result::Ok(0);
            }

            #destructure

            let mut q = pgorm::sql("UPDATE ");
            q.push(#table_name);
            q.push(" SET ");

            let mut first = true;
            #(#set_stmts)*

            if first {
                return Err(pgorm::OrmError::Validation(
                    "UpdateModel: no fields to update".to_string(),
                ));
            }

            q.push(" WHERE ");
            q.push(#table_name);
            q.push(".");
            q.push(#id_col_expr);
            q.push(" = ANY(");
            q.push_bind(ids);
            q.push(")");

            q.execute(conn).await
        }
    };

    let update_by_id_returning_method = if let Some(returning_ty) = attrs.returning.as_ref() {
        quote! {
            /// Update columns by primary key and return the updated row mapped as the configured returning type.
            pub async fn update_by_id_returning<I>(
                self,
                conn: &impl pgorm::GenericClient,
                id: I,
            ) -> pgorm::OrmResult<#returning_ty>
            where
                I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
                #returning_ty: pgorm::FromRow,
            {
                #destructure

                let mut q = pgorm::Sql::empty();
                q.push("WITH ");
                q.push(#table_name);
                q.push(" AS (UPDATE ");
                q.push(#table_name);
                q.push(" SET ");

                let mut first = true;
                #(#set_stmts)*

                if first {
                    return Err(pgorm::OrmError::Validation(
                        "UpdateModel: no fields to update".to_string(),
                    ));
                }

                q.push(" WHERE ");
                q.push(#table_name);
                q.push(".");
                q.push(#id_col_expr);
                q.push(" = ");
                q.push_bind(id);

                q.push(" RETURNING *) SELECT ");
                q.push(#returning_ty::SELECT_LIST);
                q.push(" FROM ");
                q.push(#table_name);
                q.push(" ");
                q.push(#returning_ty::JOIN_CLAUSE);

                q.fetch_one_as::<#returning_ty>(conn).await
            }

            /// Update columns by primary key for multiple rows and return updated rows
            /// mapped as the configured returning type.
            ///
            /// The same patch is applied to every matched row.
            pub async fn update_by_ids_returning<I>(
                self,
                conn: &impl pgorm::GenericClient,
                ids: ::std::vec::Vec<I>,
            ) -> pgorm::OrmResult<::std::vec::Vec<#returning_ty>>
            where
                I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + 'static,
                #returning_ty: pgorm::FromRow,
            {
                if ids.is_empty() {
                    return ::std::result::Result::Ok(::std::vec::Vec::new());
                }

                #destructure

                let mut q = pgorm::Sql::empty();
                q.push("WITH ");
                q.push(#table_name);
                q.push(" AS (UPDATE ");
                q.push(#table_name);
                q.push(" SET ");

                let mut first = true;
                #(#set_stmts)*

                if first {
                    return Err(pgorm::OrmError::Validation(
                        "UpdateModel: no fields to update".to_string(),
                    ));
                }

                q.push(" WHERE ");
                q.push(#table_name);
                q.push(".");
                q.push(#id_col_expr);
                q.push(" = ANY(");
                q.push_bind(ids);
                q.push(")");

                q.push(" RETURNING *) SELECT ");
                q.push(#returning_ty::SELECT_LIST);
                q.push(" FROM ");
                q.push(#table_name);
                q.push(" ");
                q.push(#returning_ty::JOIN_CLAUSE);

                q.fetch_all_as::<#returning_ty>(conn).await
            }
        }
    } else {
        quote! {}
    };

    // Generate update_by_id_graph methods
    let update_graph_methods = generate_update_graph_methods(&attrs, &id_col_expr)?;

    Ok(quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            pub const TABLE: &'static str = #table_name;

            #update_by_id_method

            #update_by_ids_method

            #update_by_id_returning_method

            #update_graph_methods
        }
    })
}

fn get_struct_attrs(input: &DeriveInput) -> Result<StructAttrs> {
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

/// Parse graph-style attributes for UpdateModel.
fn parse_update_graph_attr(tokens: &TokenStream, graph: &mut UpdateGraphDeclarations) -> Result<()> {
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
    key_columns: Option<String>,
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
        let mut key_columns: Option<String> = None;

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
                "fk_column" => fk_column = Some(value.value()),
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
                "key_columns" => key_columns = Some(value.value()),
                // Legacy key_field/key_column are ignored (replaced by key_columns)
                "key_field" | "key_column" => { /* ignored for backward compatibility */ }
                _ => {}
            }
        }

        let field = field.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "has_many_update requires field = \"...\"")
        })?;
        let fk_column = fk_column.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "has_many_update requires fk_column = \"...\"")
        })?;
        let fk_field = fk_field.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "has_many_update requires fk_field = \"...\"")
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
                "fk_column" => fk_column = Some(value.value()),
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
            syn::Error::new(Span::call_site(), "has_one_update requires fk_column = \"...\"")
        })?;
        let fk_field = fk_field.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "has_one_update requires fk_field = \"...\"")
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

fn get_field_attrs(field: &syn::Field) -> Result<FieldAttrs> {
    let mut merged = FieldAttrs {
        skip_update: false,
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
            merged.skip_update |= parsed.skip_update;
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

fn option_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    let seg = type_path.path.segments.last()?;
    if seg.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    if args.args.len() != 1 {
        return None;
    }
    let syn::GenericArgument::Type(inner) = args.args.first()? else {
        return None;
    };
    Some(inner)
}

// ─────────────────────────────────────────────────────────────────────────────
// update_by_id_graph methods generation
// ─────────────────────────────────────────────────────────────────────────────

/// Generate update_by_id_graph and update_by_id_graph_returning methods.
fn generate_update_graph_methods(attrs: &StructAttrs, id_col_expr: &TokenStream) -> Result<TokenStream> {
    let graph = &attrs.graph;

    // If no graph declarations, don't generate graph methods
    if !graph.has_any() {
        return Ok(quote! {});
    }

    let table_name = &attrs.table;

    // Generate child table handling code
    let has_many_code = generate_has_many_update_code(graph, table_name)?;
    let has_one_code = generate_has_one_update_code(graph, table_name)?;

    // Generate code to check if any child fields have values (Some(...))
    let mut check_children_stmts = Vec::new();
    for rel in &graph.has_many {
        let field_ident = format_ident!("{}", rel.field);
        check_children_stmts.push(quote! {
            if self.#field_ident.is_some() { __pgorm_has_child_ops = true; }
        });
    }
    for rel in &graph.has_one {
        let field_ident = format_ident!("{}", rel.field);
        check_children_stmts.push(quote! {
            if self.#field_ident.is_some() { __pgorm_has_child_ops = true; }
        });
    }
    let check_children_code = quote! { #(#check_children_stmts)* };

    // The graph methods need to handle (per doc §6.3):
    // 1. If root patch has main table fields and affected == 0: NotFound
    // 2. If root patch has no main table fields but children have changes: verify root exists first
    // 3. If nothing to do (no main fields, all children None): Validation error

    let update_by_id_graph_method = quote! {
        /// Update this struct and all related child tables by primary key.
        ///
        /// Child fields with `None` are not touched. `Some(vec)` triggers the configured strategy.
        ///
        /// Per doc §6.3:
        /// - If root has fields to update but affected == 0: returns NotFound
        /// - If root has no fields but children have changes: verifies root exists first
        /// - If nothing to do at all: returns Validation error
        pub async fn update_by_id_graph<I>(
            mut self,
            conn: &impl ::pgorm::GenericClient,
            id: I,
        ) -> ::pgorm::OrmResult<u64>
        where
            I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + ::core::clone::Clone + 'static,
        {
            let mut __pgorm_total_affected: u64 = 0;
            let __pgorm_id = id.clone();

            // Check if any child fields have operations
            let mut __pgorm_has_child_ops = false;
            #check_children_code

            // Try to update main table
            let __pgorm_main_update_result = self.update_by_id(conn, id).await;

            match __pgorm_main_update_result {
                ::std::result::Result::Ok(affected) => {
                    if affected == 0 {
                        // Main table had fields to update but no rows matched - NotFound
                        return ::std::result::Result::Err(::pgorm::OrmError::NotFound(
                            "update_by_id_graph: root row not found".to_string()
                        ));
                    }
                    __pgorm_total_affected += affected;
                }
                ::std::result::Result::Err(::pgorm::OrmError::Validation(msg)) if msg.contains("no fields to update") => {
                    // No main table fields to update
                    if !__pgorm_has_child_ops {
                        // Nothing to do at all - validation error
                        return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                            "WriteGraph: no operations to perform".to_string()
                        ));
                    }
                    // Verify root exists before touching children
                    let exists_sql = ::std::format!(
                        "SELECT 1 FROM {} WHERE {} = $1",
                        #table_name,
                        #id_col_expr
                    );
                    let exists_result = ::pgorm::query(exists_sql)
                        .bind(__pgorm_id.clone())
                        .fetch_opt(conn)
                        .await?;
                    if exists_result.is_none() {
                        return ::std::result::Result::Err(::pgorm::OrmError::NotFound(
                            "update_by_id_graph: root row not found".to_string()
                        ));
                    }
                }
                ::std::result::Result::Err(e) => return ::std::result::Result::Err(e),
            }

            // Process has_many child tables
            #has_many_code

            // Process has_one child tables
            #has_one_code

            ::std::result::Result::Ok(__pgorm_total_affected)
        }
    };

    let update_by_id_graph_returning_method = if let Some(returning_ty) = attrs.returning.as_ref() {
        quote! {
            /// Update this struct and all related child tables, returning the updated root row.
            ///
            /// Child fields with `None` are not touched. `Some(vec)` triggers the configured strategy.
            ///
            /// Per doc §6.3:
            /// - If root has fields to update but affected == 0: returns NotFound
            /// - If root has no fields but children have changes: verifies root exists first
            /// - If nothing to do at all: returns Validation error
            pub async fn update_by_id_graph_returning<I>(
                mut self,
                conn: &impl ::pgorm::GenericClient,
                id: I,
            ) -> ::pgorm::OrmResult<#returning_ty>
            where
                I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + ::core::clone::Clone + 'static,
                #returning_ty: ::pgorm::FromRow,
            {
                let mut __pgorm_total_affected: u64 = 0;
                let __pgorm_id = id.clone();
                let __pgorm_root_result: #returning_ty;

                // Check if any child fields have operations
                let mut __pgorm_has_child_ops = false;
                #check_children_code

                // Try to update main table
                let __pgorm_main_update_result = self.update_by_id_returning(conn, id).await;

                match __pgorm_main_update_result {
                    ::std::result::Result::Ok(result) => {
                        __pgorm_total_affected += 1;
                        __pgorm_root_result = result;
                    }
                    ::std::result::Result::Err(::pgorm::OrmError::Validation(msg)) if msg.contains("no fields to update") => {
                        // No main table fields to update
                        if !__pgorm_has_child_ops {
                            // Nothing to do at all - validation error
                            return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                                "WriteGraph: no operations to perform".to_string()
                            ));
                        }
                        // Fetch current row (also verifies it exists)
                        let sql = ::std::format!(
                            "SELECT {} FROM {} {} WHERE {}.{} = $1",
                            #returning_ty::SELECT_LIST,
                            #table_name,
                            #returning_ty::JOIN_CLAUSE,
                            #table_name,
                            #id_col_expr
                        );
                        __pgorm_root_result = ::pgorm::query(sql)
                            .bind(__pgorm_id.clone())
                            .fetch_one_as::<#returning_ty>(conn)
                            .await
                            .map_err(|e| match e {
                                ::pgorm::OrmError::NotFound(_) => ::pgorm::OrmError::NotFound(
                                    "update_by_id_graph: root row not found".to_string()
                                ),
                                other => other,
                            })?;
                    }
                    ::std::result::Result::Err(::pgorm::OrmError::NotFound(_)) => {
                        return ::std::result::Result::Err(::pgorm::OrmError::NotFound(
                            "update_by_id_graph: root row not found".to_string()
                        ));
                    }
                    ::std::result::Result::Err(e) => return ::std::result::Result::Err(e),
                }

                // Process has_many child tables
                #has_many_code

                // Process has_one child tables
                #has_one_code

                ::std::result::Result::Ok(__pgorm_root_result)
            }
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        #update_by_id_graph_method
        #update_by_id_graph_returning_method
    })
}

/// Generate code for has_many_update child tables.
/// Uses with_* setters to inject FK values (avoiding direct field access for cross-module compatibility).
fn generate_has_many_update_code(graph: &UpdateGraphDeclarations, _table_name: &str) -> Result<TokenStream> {
    if graph.has_many.is_empty() {
        return Ok(quote! {});
    }

    let mut code_blocks = Vec::new();

    for rel in &graph.has_many {
        let field_ident = format_ident!("{}", rel.field);
        let child_type = &rel.child_type;
        let fk_column = &rel.fk_column;

        // Use with_* setter to inject FK
        let setter_name = format_ident!("with_{}", rel.fk_field);

        let strategy_code = match rel.strategy {
            UpdateStrategy::Replace => {
                // Delete all old children, insert new ones
                quote! {
                    // Delete old children
                    let delete_sql = ::std::format!(
                        "DELETE FROM {} WHERE {} = $1",
                        #child_type::TABLE,
                        #fk_column
                    );
                    let deleted = ::pgorm::query(delete_sql).bind(__pgorm_id.clone()).execute(conn).await?;
                    __pgorm_total_affected += deleted;

                    // Insert new children
                    if !children.is_empty() {
                        let children_with_fk: ::std::vec::Vec<_> = children
                            .into_iter()
                            .map(|child| child.#setter_name(__pgorm_id.clone()))
                            .collect();
                        let inserted = #child_type::insert_many(conn, children_with_fk).await?;
                        __pgorm_total_affected += inserted;
                    }
                }
            }
            UpdateStrategy::Append => {
                // Only insert new children
                quote! {
                    if !children.is_empty() {
                        let children_with_fk: ::std::vec::Vec<_> = children
                            .into_iter()
                            .map(|child| child.#setter_name(__pgorm_id.clone()))
                            .collect();
                        let inserted = #child_type::insert_many(conn, children_with_fk).await?;
                        __pgorm_total_affected += inserted;
                    }
                }
            }
            UpdateStrategy::Upsert => {
                // Upsert children
                quote! {
                    if !children.is_empty() {
                        let children_with_fk: ::std::vec::Vec<_> = children
                            .into_iter()
                            .map(|child| child.#setter_name(__pgorm_id.clone()))
                            .collect();
                        let upserted = #child_type::upsert_many(conn, children_with_fk).await?;
                        __pgorm_total_affected += upserted;
                    }
                }
            }
            UpdateStrategy::Diff => {
                // Upsert + delete missing (uses __pgorm_diff_many_by_fk helper)
                let key_columns_str = rel.key_columns.as_ref().unwrap();
                // Split by comma and trim whitespace to support multi-column keys
                let key_columns_vec: Vec<_> = key_columns_str
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect();

                quote! {
                    // Use diff helper to upsert and delete missing children
                    let diff_affected = #child_type::__pgorm_diff_many_by_fk(
                        conn,
                        #fk_column,
                        __pgorm_id.clone(),
                        &[#(#key_columns_vec),*],
                        children
                            .into_iter()
                            .map(|child| child.#setter_name(__pgorm_id.clone()))
                            .collect(),
                    ).await?;
                    __pgorm_total_affected += diff_affected;
                }
            }
        };

        let code = quote! {
            // has_many_update: #field_ident
            if let ::std::option::Option::Some(children) = self.#field_ident {
                #strategy_code
            }
        };

        code_blocks.push(code);
    }

    Ok(quote! {
        #(#code_blocks)*
    })
}

/// Generate code for has_one_update child tables.
/// Uses with_* setters to inject FK values (avoiding direct field access for cross-module compatibility).
fn generate_has_one_update_code(graph: &UpdateGraphDeclarations, _table_name: &str) -> Result<TokenStream> {
    if graph.has_one.is_empty() {
        return Ok(quote! {});
    }

    let mut code_blocks = Vec::new();

    for rel in &graph.has_one {
        let field_ident = format_ident!("{}", rel.field);
        let child_type = &rel.child_type;
        let fk_column = &rel.fk_column;

        // Use with_* setter to inject FK
        let setter_name = format_ident!("with_{}", rel.fk_field);

        let strategy_code = match rel.strategy {
            UpdateStrategy::Replace => {
                quote! {
                    // Delete old child
                    let delete_sql = ::std::format!(
                        "DELETE FROM {} WHERE {} = $1",
                        #child_type::TABLE,
                        #fk_column
                    );
                    let deleted = ::pgorm::query(delete_sql).bind(__pgorm_id.clone()).execute(conn).await?;
                    __pgorm_total_affected += deleted;

                    // Insert new child if present
                    if let ::std::option::Option::Some(child) = inner_value {
                        let child_with_fk = child.#setter_name(__pgorm_id.clone());
                        let inserted = child_with_fk.insert(conn).await?;
                        __pgorm_total_affected += inserted;
                    }
                }
            }
            UpdateStrategy::Upsert => {
                quote! {
                    if let ::std::option::Option::Some(child) = inner_value {
                        let child_with_fk = child.#setter_name(__pgorm_id.clone());
                        let upserted = child_with_fk.upsert(conn).await?;
                        __pgorm_total_affected += upserted;
                    }
                }
            }
            _ => {
                // Other strategies not applicable for has_one
                quote! {}
            }
        };

        // has_one_update uses Option<Option<Child>>:
        // - None: don't touch
        // - Some(None): delete child
        // - Some(Some(child)): replace/upsert
        let code = quote! {
            // has_one_update: #field_ident
            if let ::std::option::Option::Some(inner_value) = self.#field_ident {
                #strategy_code
            }
        };

        code_blocks.push(code);
    }

    Ok(quote! {
        #(#code_blocks)*
    })
}
