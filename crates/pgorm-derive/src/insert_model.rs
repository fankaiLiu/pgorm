//! InsertModel derive macro implementation

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Result};

struct BindField {
    ident: syn::Ident,
    ty: syn::Type,
    column: String,
}

/// Info needed for generating inline INSERT SQL in graph methods.
struct InsertSqlInfo {
    /// The INSERT SQL string (without RETURNING).
    sql: String,
    /// Field identifiers that need to be bound to the SQL.
    bind_idents: Vec<syn::Ident>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Graph Declarations (for multi-table writes)
// ─────────────────────────────────────────────────────────────────────────────

/// has_one / has_many declaration.
#[derive(Clone)]
struct HasRelation {
    /// The child InsertModel type.
    child_type: syn::Path,
    /// The Rust field name on this struct.
    field: String,
    /// The child's foreign key field name.
    fk_field: String,
    /// Is this has_one (single) or has_many (vec)?
    is_many: bool,
    /// Mode: "insert" or "upsert" (default: insert).
    mode: HasRelationMode,
}

/// Mode for has_one/has_many operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum HasRelationMode {
    #[default]
    Insert,
    Upsert,
}

/// belongs_to declaration (pre-insert dependency).
#[derive(Clone)]
struct BelongsTo {
    /// The parent InsertModel type.
    parent_type: syn::Path,
    /// The Rust field name on this struct.
    field: String,
    /// The field to set with parent's id.
    set_fk_field: String,
    /// Mode: "insert_returning" or "upsert_returning".
    mode: BelongsToMode,
    /// Whether this relation is required.
    required: bool,
}

/// before_insert / after_insert step.
#[derive(Clone)]
struct InsertStep {
    /// The InsertModel type to insert.
    step_type: syn::Path,
    /// The Rust field name on this struct.
    field: String,
    /// Mode: "insert" or "upsert".
    mode: StepMode,
    /// Is this before or after the root insert?
    is_before: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum BelongsToMode {
    #[default]
    InsertReturning,
    UpsertReturning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum StepMode {
    #[default]
    Insert,
    Upsert,
}

/// All graph declarations for an InsertModel.
#[derive(Clone, Default)]
struct GraphDeclarations {
    /// The root ID field name (from input, for UUID/snowflake scenarios).
    /// When set, root_id is taken from self.<field> instead of from returning.
    /// Per doc §5: If both returning and graph_root_id_field are set, graph_root_id_field wins.
    graph_root_id_field: Option<String>,
    /// has_one / has_many relations.
    has_relations: Vec<HasRelation>,
    /// belongs_to relations (pre-insert).
    belongs_to: Vec<BelongsTo>,
    /// before_insert / after_insert steps.
    insert_steps: Vec<InsertStep>,
}

impl GraphDeclarations {
    fn has_any(&self) -> bool {
        !self.has_relations.is_empty()
            || !self.belongs_to.is_empty()
            || !self.insert_steps.is_empty()
    }

    /// Get all field names that are used by graph declarations (should not be inserted into main table).
    fn graph_field_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for rel in &self.has_relations {
            names.push(rel.field.clone());
        }
        for bt in &self.belongs_to {
            names.push(bt.field.clone());
        }
        for step in &self.insert_steps {
            names.push(step.field.clone());
        }
        names
    }
}

struct StructAttrs {
    table: String,
    returning: Option<syn::Path>,
    conflict_target: Option<Vec<String>>,
    conflict_constraint: Option<String>,
    conflict_update: Option<Vec<String>>,
    graph: GraphDeclarations,
}

struct StructAttrList {
    table: Option<String>,
    returning: Option<syn::Path>,
    conflict_target: Option<Vec<String>>,
    conflict_constraint: Option<String>,
    conflict_update: Option<Vec<String>>,
    graph_root_id_field: Option<String>,
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
                    let cols: Vec<String> = value
                        .value()
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if cols.is_empty() {
                        return Err(syn::Error::new(
                            value.span(),
                            "conflict_target must specify at least one column",
                        ));
                    }
                    conflict_target = Some(cols);
                }
                "conflict_constraint" => {
                    let constraint_name = value.value().trim().to_string();
                    if constraint_name.is_empty() {
                        return Err(syn::Error::new(
                            value.span(),
                            "conflict_constraint must specify a constraint name",
                        ));
                    }
                    conflict_constraint = Some(constraint_name);
                }
                "conflict_update" => {
                    let cols: Vec<String> = value
                        .value()
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    conflict_update = Some(cols);
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

        Ok(Self { table, returning, conflict_target, conflict_constraint, conflict_update, graph_root_id_field })
    }
}

struct FieldAttrs {
    is_id: bool,
    skip_insert: bool,
    default: bool,
    table: Option<String>,
    column: Option<String>,
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

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let struct_attrs = get_struct_attrs(&input)?;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "InsertModel can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "InsertModel can only be derived for structs",
            ));
        }
    };

    let mut insert_columns: Vec<String> = Vec::new();
    let mut insert_value_exprs: Vec<String> = Vec::new();
    let mut bind_field_idents: Vec<syn::Ident> = Vec::new();
    let mut batch_bind_fields: Vec<BindField> = Vec::new();
    let mut id_field: Option<BindField> = None;

    // Get field names used by graph declarations (should not be inserted into main table)
    let graph_field_names = struct_attrs.graph.graph_field_names();

    let mut param_idx = 0_usize;

    for field in fields.iter() {
        let field_ident = field.ident.clone().unwrap();
        let field_name = field_ident.to_string();
        let field_ty = field.ty.clone();

        // Skip fields used by graph declarations
        if graph_field_names.contains(&field_name) {
            continue;
        }

        let field_attrs = get_field_attrs(field)?;

        if let Some(field_table) = &field_attrs.table {
            if field_table != &struct_attrs.table {
                return Err(syn::Error::new_spanned(
                    field,
                    "InsertModel does not support fields from joined/other tables",
                ));
            }
        }

        let column_name = field_attrs
            .column
            .clone()
            .unwrap_or_else(|| field_name.clone());

        if field_attrs.is_id {
            if id_field.is_some() {
                return Err(syn::Error::new_spanned(
                    field,
                    "InsertModel supports only one #[orm(id)] field",
                ));
            }
            id_field = Some(BindField {
                ident: field_ident.clone(),
                ty: field_ty.clone(),
                column: column_name.clone(),
            });
        }

        if field_attrs.skip_insert || field_attrs.is_id {
            continue;
        }

        insert_columns.push(column_name.clone());

        if field_attrs.default {
            insert_value_exprs.push("DEFAULT".to_string());
        } else {
            param_idx += 1;
            insert_value_exprs.push(format!("${}", param_idx));
            bind_field_idents.push(field_ident.clone());
            batch_bind_fields.push(BindField {
                ident: field_ident,
                ty: field_ty,
                column: column_name,
            });
        }
    }

    let table_name = &struct_attrs.table;
    let insert_sql = if insert_columns.is_empty() {
        format!("INSERT INTO {} DEFAULT VALUES", table_name)
    } else {
        format!(
            "INSERT INTO {} ({}) VALUES ({})",
            table_name,
            insert_columns.join(", "),
            insert_value_exprs.join(", ")
        )
    };

    let destructure = if bind_field_idents.is_empty() {
        quote! { let _ = self; }
    } else {
        quote! { let Self { #(#bind_field_idents),*, .. } = self; }
    };

    let insert_query_expr = bind_field_idents.iter().fold(
        quote! { pgorm::query(#insert_sql) },
        |acc, ident| quote! { #acc.bind(#ident) },
    );

    let insert_method = quote! {
        /// Insert a new row into the target table.
        pub async fn insert(self, conn: &impl pgorm::GenericClient) -> pgorm::OrmResult<u64> {
            #destructure
            #insert_query_expr.execute(conn).await
        }
    };

    let insert_many_method = if batch_bind_fields.is_empty() {
        quote! {
            /// Insert multiple rows.
            ///
            /// Falls back to per-row inserts when bulk insert isn't applicable.
            pub async fn insert_many(
                conn: &impl pgorm::GenericClient,
                rows: ::std::vec::Vec<Self>,
            ) -> pgorm::OrmResult<u64> {
                if rows.is_empty() {
                    return ::std::result::Result::Ok(0);
                }

                let mut affected = 0_u64;
                for row in rows {
                    affected += row.insert(conn).await?;
                }
                ::std::result::Result::Ok(affected)
            }
        }
    } else {
        let batch_columns: Vec<String> = batch_bind_fields.iter().map(|f| f.column.clone()).collect();
        let batch_columns_str = batch_columns.join(", ");
        let list_idents: Vec<syn::Ident> = batch_bind_fields
            .iter()
            .map(|f| format_ident!("__pgorm_insert_{}_list", f.ident))
            .collect();
        let field_idents: Vec<syn::Ident> = batch_bind_fields.iter().map(|f| f.ident.clone()).collect();
        let field_tys: Vec<syn::Type> = batch_bind_fields.iter().map(|f| f.ty.clone()).collect();

        let init_lists: Vec<TokenStream> = list_idents
            .iter()
            .zip(field_tys.iter())
            .map(|(list_ident, ty)| {
                quote! { let mut #list_ident: ::std::vec::Vec<#ty> = ::std::vec::Vec::with_capacity(rows.len()); }
            })
            .collect();

        let push_lists: Vec<TokenStream> = list_idents
            .iter()
            .zip(field_idents.iter())
            .map(|(list_ident, field_ident)| quote! { #list_ident.push(#field_ident); })
            .collect();

        // Generate type casts using PgType trait at runtime
        let type_cast_exprs: Vec<TokenStream> = field_tys
            .iter()
            .enumerate()
            .map(|(i, ty)| {
                let idx = i + 1;
                quote! { ::std::format!("${}::{}", #idx, <#ty as pgorm::PgType>::pg_array_type()) }
            })
            .collect();

        let bind_lists_expr = list_idents.iter().fold(
            quote! { pgorm::query(&sql) },
            |acc, list_ident| quote! { #acc.bind(#list_ident) },
        );

        quote! {
            /// Insert multiple rows using a single statement (UNNEST bulk insert).
            pub async fn insert_many(
                conn: &impl pgorm::GenericClient,
                rows: ::std::vec::Vec<Self>,
            ) -> pgorm::OrmResult<u64> {
                if rows.is_empty() {
                    return ::std::result::Result::Ok(0);
                }

                #(#init_lists)*

                for row in rows {
                    let Self { #(#field_idents),*, .. } = row;
                    #(#push_lists)*
                }

                let type_casts: ::std::vec::Vec<::std::string::String> = ::std::vec![#(#type_cast_exprs),*];
                let sql = ::std::format!(
                    "INSERT INTO {} ({}) SELECT * FROM UNNEST({})",
                    #table_name,
                    #batch_columns_str,
                    type_casts.join(", ")
                );

                #bind_lists_expr.execute(conn).await
            }
        }
    };

    // Determine conflict specification for UPSERT:
    // - conflict_constraint: ON CONFLICT ON CONSTRAINT constraint_name
    // - conflict_target: ON CONFLICT (col1, col2, ...)
    // - fallback: ON CONFLICT (id_column)

    enum ConflictSpec {
        Constraint(String),
        Columns(Vec<String>),
    }

    let conflict_spec: Option<ConflictSpec> = if let Some(constraint) = struct_attrs.conflict_constraint.clone() {
        Some(ConflictSpec::Constraint(constraint))
    } else if let Some(cols) = struct_attrs.conflict_target.clone() {
        Some(ConflictSpec::Columns(cols))
    } else {
        id_field.as_ref().map(|f| ConflictSpec::Columns(vec![f.column.clone()]))
    };

    let upsert_methods = if let Some(conflict_spec) = conflict_spec {
        // Build the ON CONFLICT clause
        let (on_conflict_clause, conflict_cols_for_exclusion): (String, Vec<String>) = match &conflict_spec {
            ConflictSpec::Constraint(name) => {
                (format!("ON CONFLICT ON CONSTRAINT {}", name), vec![])
            }
            ConflictSpec::Columns(cols) => {
                (format!("ON CONFLICT ({})", cols.join(", ")), cols.clone())
            }
        };

        // For upsert, we need all fields that should be in the INSERT (including conflict columns)
        // If conflict_target is set, we use all bind fields + any fields matching conflict columns
        // If using id field, we include id + bind fields
        // For conflict_constraint, we use bind fields only (constraint defines uniqueness)
        let (upsert_columns, upsert_bind_idents, upsert_bind_field_tys): (Vec<String>, Vec<syn::Ident>, Vec<syn::Type>) =
            match &conflict_spec {
                ConflictSpec::Constraint(_) => {
                    // With constraint-based conflict, include all bind fields
                    // (the constraint already defines which columns make up uniqueness)
                    (
                        batch_bind_fields.iter().map(|f| f.column.clone()).collect(),
                        batch_bind_fields.iter().map(|f| f.ident.clone()).collect(),
                        batch_bind_fields.iter().map(|f| f.ty.clone()).collect(),
                    )
                }
                ConflictSpec::Columns(conflict_cols) => {
                    if struct_attrs.conflict_target.is_some() {
                        // With explicit conflict_target, include all insert columns (bind fields)
                        // but we need to also include conflict columns if they're not already in bind_field_idents
                        let mut columns: Vec<String> = batch_bind_fields.iter().map(|f| f.column.clone()).collect();
                        let mut idents: Vec<syn::Ident> = batch_bind_fields.iter().map(|f| f.ident.clone()).collect();
                        let mut tys: Vec<syn::Type> = batch_bind_fields.iter().map(|f| f.ty.clone()).collect();

                        // If we have an id field and it's in the conflict columns, add it
                        if let Some(id_f) = id_field.as_ref() {
                            if conflict_cols.contains(&id_f.column) && !columns.contains(&id_f.column) {
                                columns.insert(0, id_f.column.clone());
                                idents.insert(0, id_f.ident.clone());
                                tys.insert(0, id_f.ty.clone());
                            }
                        }

                        (columns, idents, tys)
                    } else if let Some(id_f) = id_field.as_ref() {
                        // Using id field as conflict target (original behavior)
                        let columns: Vec<String> = std::iter::once(id_f.column.clone())
                            .chain(batch_bind_fields.iter().map(|f| f.column.clone()))
                            .collect();
                        let idents: Vec<syn::Ident> = std::iter::once(id_f.ident.clone())
                            .chain(batch_bind_fields.iter().map(|f| f.ident.clone()))
                            .collect();
                        let tys: Vec<syn::Type> = std::iter::once(id_f.ty.clone())
                            .chain(batch_bind_fields.iter().map(|f| f.ty.clone()))
                            .collect();
                        (columns, idents, tys)
                    } else {
                        // Should not happen since we have conflict_cols
                        (vec![], vec![], vec![])
                    }
                }
            };

        let placeholders: Vec<String> = (1..=upsert_bind_idents.len()).map(|i| format!("${}", i)).collect();

        // Update assignments:
        // - If conflict_update is set, only update specified columns
        // - Otherwise, update all columns except the conflict columns
        let mut update_assignments: Vec<String> = if let Some(update_cols) = &struct_attrs.conflict_update {
            // Only update specified columns
            update_cols
                .iter()
                .map(|col| format!("{} = EXCLUDED.{}", col, col))
                .collect()
        } else {
            // Default: update all columns except conflict columns
            upsert_columns
                .iter()
                .filter(|col| !conflict_cols_for_exclusion.contains(col))
                .map(|col| format!("{} = EXCLUDED.{}", col, col))
                .collect()
        };
        if update_assignments.is_empty() {
            // If no columns to update, generate a no-op update (assign first column to itself)
            if let Some(first_col) = upsert_columns.first() {
                update_assignments.push(format!("{} = EXCLUDED.{}", first_col, first_col));
            }
        }

        let upsert_sql = format!(
            "INSERT INTO {} ({}) VALUES ({}) {} DO UPDATE SET {}",
            table_name,
            upsert_columns.join(", "),
            placeholders.join(", "),
            on_conflict_clause,
            update_assignments.join(", ")
        );

        let upsert_destructure = quote! { let Self { #(#upsert_bind_idents),*, .. } = self; };
        let upsert_query_expr = upsert_bind_idents.iter().fold(
            quote! { ::pgorm::query(#upsert_sql) },
            |acc, ident| quote! { #acc.bind(#ident) },
        );

        let upsert_method = quote! {
            /// Insert a row, or update it if a conflict occurs on the conflict target (Postgres UPSERT).
            pub async fn upsert(self, conn: &impl ::pgorm::GenericClient) -> ::pgorm::OrmResult<u64> {
                #upsert_destructure
                #upsert_query_expr.execute(conn).await
            }
        };

        let upsert_batch_list_idents: Vec<syn::Ident> = upsert_bind_idents
            .iter()
            .map(|ident| format_ident!("__pgorm_upsert_{}_list", ident))
            .collect();

        let upsert_batch_init_lists: Vec<TokenStream> = upsert_batch_list_idents
            .iter()
            .zip(upsert_bind_field_tys.iter())
            .map(|(list_ident, ty)| {
                quote! { let mut #list_ident: ::std::vec::Vec<#ty> = ::std::vec::Vec::with_capacity(rows.len()); }
            })
            .collect();

        let upsert_batch_push_lists: Vec<TokenStream> = upsert_batch_list_idents
            .iter()
            .zip(upsert_bind_idents.iter())
            .map(|(list_ident, field_ident)| quote! { #list_ident.push(#field_ident); })
            .collect();

        // Generate type casts for upsert batch using PgType trait
        let upsert_type_cast_exprs: Vec<TokenStream> = upsert_bind_field_tys
            .iter()
            .enumerate()
            .map(|(i, ty)| {
                let idx = i + 1;
                quote! { ::std::format!("${}::{}", #idx, <#ty as ::pgorm::PgType>::pg_array_type()) }
            })
            .collect();

        let upsert_many_query_expr = upsert_batch_list_idents.iter().fold(
            quote! { ::pgorm::query(&upsert_batch_sql) },
            |acc, list_ident| quote! { #acc.bind(#list_ident) },
        );

        let upsert_columns_str = upsert_columns.join(", ");
        let update_assignments_str = update_assignments.join(", ");

        let upsert_many_method = quote! {
            /// Insert or update multiple rows using a single statement (UNNEST + ON CONFLICT).
            pub async fn upsert_many(
                conn: &impl ::pgorm::GenericClient,
                rows: ::std::vec::Vec<Self>,
            ) -> ::pgorm::OrmResult<u64> {
                if rows.is_empty() {
                    return ::std::result::Result::Ok(0);
                }

                #(#upsert_batch_init_lists)*

                for row in rows {
                    let Self { #(#upsert_bind_idents),*, .. } = row;
                    #(#upsert_batch_push_lists)*
                }

                let type_casts: ::std::vec::Vec<::std::string::String> = ::std::vec![#(#upsert_type_cast_exprs),*];
                let upsert_batch_sql = ::std::format!(
                    "INSERT INTO {} ({}) SELECT * FROM UNNEST({}) {} DO UPDATE SET {}",
                    #table_name,
                    #upsert_columns_str,
                    type_casts.join(", "),
                    #on_conflict_clause,
                    #update_assignments_str
                );

                #upsert_many_query_expr.execute(conn).await
            }
        };

        let upsert_returning_methods = if let Some(returning_ty) = struct_attrs.returning.as_ref() {
            let upsert_returning_query_expr = upsert_bind_idents.iter().fold(
                quote! { ::pgorm::query(sql) },
                |acc, ident| quote! { #acc.bind(#ident) },
            );
            let upsert_many_returning_query_expr = upsert_batch_list_idents.iter().fold(
                quote! { ::pgorm::query(sql) },
                |acc, list_ident| quote! { #acc.bind(#list_ident) },
            );

            quote! {
                /// UPSERT and return the resulting row mapped as the configured returning type.
                pub async fn upsert_returning(
                    self,
                    conn: &impl ::pgorm::GenericClient,
                ) -> ::pgorm::OrmResult<#returning_ty>
                where
                    #returning_ty: ::pgorm::FromRow,
                {
                    #upsert_destructure
                    let sql = ::std::format!(
                        "WITH {table} AS ({upsert} RETURNING *) SELECT {} FROM {table} {}",
                        #returning_ty::SELECT_LIST,
                        #returning_ty::JOIN_CLAUSE,
                        table = #table_name,
                        upsert = #upsert_sql,
                    );
                    #upsert_returning_query_expr.fetch_one_as::<#returning_ty>(conn).await
                }

                /// UPSERT multiple rows and return resulting rows mapped as the configured returning type.
                pub async fn upsert_many_returning(
                    conn: &impl ::pgorm::GenericClient,
                    rows: ::std::vec::Vec<Self>,
                ) -> ::pgorm::OrmResult<::std::vec::Vec<#returning_ty>>
                where
                    #returning_ty: ::pgorm::FromRow,
                {
                    if rows.is_empty() {
                        return ::std::result::Result::Ok(::std::vec::Vec::new());
                    }

                    #(#upsert_batch_init_lists)*

                    for row in rows {
                        let Self { #(#upsert_bind_idents),*, .. } = row;
                        #(#upsert_batch_push_lists)*
                    }

                    let type_casts: ::std::vec::Vec<::std::string::String> = ::std::vec![#(#upsert_type_cast_exprs),*];
                    let upsert_batch_sql = ::std::format!(
                        "INSERT INTO {} ({}) SELECT * FROM UNNEST({}) {} DO UPDATE SET {}",
                        #table_name,
                        #upsert_columns_str,
                        type_casts.join(", "),
                        #on_conflict_clause,
                        #update_assignments_str
                    );

                    let sql = ::std::format!(
                        "WITH {table} AS ({upsert} RETURNING *) SELECT {} FROM {table} {}",
                        #returning_ty::SELECT_LIST,
                        #returning_ty::JOIN_CLAUSE,
                        table = #table_name,
                        upsert = upsert_batch_sql,
                    );

                    #upsert_many_returning_query_expr.fetch_all_as::<#returning_ty>(conn).await
                }
            }
        } else {
            quote! {}
        };

        // Generate __pgorm_diff_many_by_fk helper for diff strategy
        // This uses a CTE to:
        // 1. UPSERT all rows using UNNEST
        // 2. DELETE rows with matching fk that are not in the upserted set
        // 3. Return combined affected count
        let diff_helper = {
            // We need the same batch bind fields and their types
            let field_idents: Vec<syn::Ident> = upsert_bind_idents.clone();
            let field_tys: Vec<syn::Type> = upsert_bind_field_tys.clone();
            let upsert_list_idents: Vec<syn::Ident> = field_idents
                .iter()
                .map(|ident| format_ident!("__pgorm_diff_{}_list", ident))
                .collect();

            let init_lists: Vec<TokenStream> = upsert_list_idents
                .iter()
                .zip(field_tys.iter())
                .map(|(list_ident, ty)| {
                    quote! { let mut #list_ident: ::std::vec::Vec<#ty> = ::std::vec::Vec::with_capacity(rows.len()); }
                })
                .collect();

            let push_lists: Vec<TokenStream> = upsert_list_idents
                .iter()
                .zip(field_idents.iter())
                .map(|(list_ident, field_ident)| quote! { #list_ident.push(#field_ident); })
                .collect();

            // Generate type casts using PgType trait
            let type_cast_exprs: Vec<TokenStream> = field_tys
                .iter()
                .enumerate()
                .map(|(i, ty)| {
                    // fk_value is $1, so columns start at $2
                    let idx = i + 2;
                    quote! { ::std::format!("${}::{}", #idx, <#ty as ::pgorm::PgType>::pg_array_type()) }
                })
                .collect();

            let bind_lists_expr = upsert_list_idents.iter().fold(
                quote! { ::pgorm::query(&sql).bind(fk_value) },
                |acc, list_ident| quote! { #acc.bind(#list_ident) },
            );

            quote! {
                /// Internal helper for diff strategy. Upserts rows and deletes rows not in the new list.
                ///
                /// This method is used by UpdateModel's has_many_update with strategy = "diff".
                #[doc(hidden)]
                pub async fn __pgorm_diff_many_by_fk<I>(
                    conn: &impl ::pgorm::GenericClient,
                    fk_column: &'static str,
                    fk_value: I,
                    key_columns: &'static [&'static str],
                    rows: ::std::vec::Vec<Self>,
                ) -> ::pgorm::OrmResult<u64>
                where
                    I: ::tokio_postgres::types::ToSql + ::core::marker::Sync + ::core::marker::Send + ::core::clone::Clone + 'static,
                {
                    let rows_count = rows.len() as u64;

                    if rows.is_empty() {
                        // Empty list means delete all children with this fk
                        let delete_sql = ::std::format!(
                            "DELETE FROM {} WHERE {} = $1",
                            #table_name,
                            fk_column
                        );
                        return ::pgorm::query(delete_sql).bind(fk_value).execute(conn).await;
                    }

                    // Collect columns into arrays
                    #(#init_lists)*

                    for row in rows {
                        let Self { #(#field_idents),*, .. } = row;
                        #(#push_lists)*
                    }

                    // Build the CTE query
                    let type_casts: ::std::vec::Vec<::std::string::String> = ::std::vec![#(#type_cast_exprs),*];

                    // Build key column equality conditions for NOT EXISTS
                    let key_conditions: ::std::string::String = key_columns
                        .iter()
                        .map(|col| ::std::format!("u.{} = c.{}", col, col))
                        .collect::<::std::vec::Vec<_>>()
                        .join(" AND ");

                    // Build RETURNING clause for key columns
                    let returning_keys = key_columns.join(", ");

                    let sql = ::std::format!(
                        "WITH upserted AS (\
                            INSERT INTO {} ({}) \
                            SELECT * FROM UNNEST({}) AS t({}) \
                            {} DO UPDATE SET {} \
                            RETURNING {}\
                        ), \
                        deleted AS (\
                            DELETE FROM {} c \
                            WHERE c.{} = $1 \
                            AND NOT EXISTS (\
                                SELECT 1 FROM upserted u WHERE {}\
                            ) \
                            RETURNING 1\
                        ) \
                        SELECT (SELECT COUNT(*) FROM deleted) AS deleted_count",
                        #table_name,
                        #upsert_columns_str,
                        type_casts.join(", "),
                        #upsert_columns_str,
                        #on_conflict_clause,
                        #update_assignments_str,
                        returning_keys,
                        #table_name,
                        fk_column,
                        key_conditions
                    );

                    let row = #bind_lists_expr.fetch_one(conn).await?;
                    let deleted_count: i64 = row.get(0);

                    ::std::result::Result::Ok(rows_count + deleted_count as u64)
                }
            }
        };

        quote! {
            #upsert_method
            #upsert_many_method
            #upsert_returning_methods
            #diff_helper
        }
    } else {
        quote! {}
    };

    let returning_method = if let Some(returning_ty) = struct_attrs.returning.as_ref() {
        let returning_query_expr =
            bind_field_idents
                .iter()
                .fold(quote! { pgorm::query(sql) }, |acc, ident| {
                    quote! { #acc.bind(#ident) }
                });

        let insert_many_returning_method = if batch_bind_fields.is_empty() {
            quote! {
                /// Insert multiple rows and return created rows mapped as the configured returning type.
                ///
                /// Falls back to per-row inserts when bulk insert isn't applicable.
                pub async fn insert_many_returning(
                    conn: &impl pgorm::GenericClient,
                    rows: ::std::vec::Vec<Self>,
                ) -> pgorm::OrmResult<::std::vec::Vec<#returning_ty>>
                where
                    #returning_ty: pgorm::FromRow,
                {
                    if rows.is_empty() {
                        return ::std::result::Result::Ok(::std::vec::Vec::new());
                    }

                    let mut out = ::std::vec::Vec::with_capacity(rows.len());
                    for row in rows {
                        out.push(row.insert_returning(conn).await?);
                    }
                    ::std::result::Result::Ok(out)
                }
            }
        } else {
            let batch_columns: Vec<String> =
                batch_bind_fields.iter().map(|f| f.column.clone()).collect();

            let list_idents: Vec<syn::Ident> = batch_bind_fields
                .iter()
                .map(|f| format_ident!("__pgorm_insert_{}_list", f.ident))
                .collect();
            let field_idents: Vec<syn::Ident> =
                batch_bind_fields.iter().map(|f| f.ident.clone()).collect();
            let field_tys: Vec<syn::Type> = batch_bind_fields.iter().map(|f| f.ty.clone()).collect();

            let init_lists: Vec<TokenStream> = list_idents
                .iter()
                .zip(field_tys.iter())
                .map(|(list_ident, ty)| {
                    quote! { let mut #list_ident: ::std::vec::Vec<#ty> = ::std::vec::Vec::with_capacity(rows.len()); }
                })
                .collect();

            let push_lists: Vec<TokenStream> = list_idents
                .iter()
                .zip(field_idents.iter())
                .map(|(list_ident, field_ident)| quote! { #list_ident.push(#field_ident); })
                .collect();

            let batch_returning_query_expr = list_idents.iter().fold(
                quote! { pgorm::query(&sql) },
                |acc, list_ident| quote! { #acc.bind(#list_ident) },
            );

            // Generate type casts for batch insert returning using PgType trait
            let batch_type_cast_exprs: Vec<TokenStream> = field_tys
                .iter()
                .enumerate()
                .map(|(i, ty)| {
                    let idx = i + 1;
                    quote! { ::std::format!("${}::{}", #idx, <#ty as pgorm::PgType>::pg_array_type()) }
                })
                .collect();

            let batch_columns_str = batch_columns.join(", ");

            quote! {
                /// Insert multiple rows and return created rows mapped as the configured returning type.
                pub async fn insert_many_returning(
                    conn: &impl pgorm::GenericClient,
                    rows: ::std::vec::Vec<Self>,
                ) -> pgorm::OrmResult<::std::vec::Vec<#returning_ty>>
                where
                    #returning_ty: pgorm::FromRow,
                {
                    if rows.is_empty() {
                        return ::std::result::Result::Ok(::std::vec::Vec::new());
                    }

                    #(#init_lists)*

                    for row in rows {
                        let Self { #(#field_idents),*, .. } = row;
                        #(#push_lists)*
                    }

                    let type_casts: ::std::vec::Vec<::std::string::String> = ::std::vec![#(#batch_type_cast_exprs),*];
                    let batch_insert_sql = ::std::format!(
                        "INSERT INTO {} ({}) SELECT * FROM UNNEST({})",
                        #table_name,
                        #batch_columns_str,
                        type_casts.join(", ")
                    );

                    let sql = ::std::format!(
                        "WITH {table} AS ({insert} RETURNING *) SELECT {} FROM {table} {}",
                        #returning_ty::SELECT_LIST,
                        #returning_ty::JOIN_CLAUSE,
                        table = #table_name,
                        insert = batch_insert_sql,
                    );

                    #batch_returning_query_expr.fetch_all_as::<#returning_ty>(conn).await
                }
            }
        };

        quote! {
            /// Insert and return the created row mapped as the configured returning type.
            pub async fn insert_returning(
                self,
                conn: &impl pgorm::GenericClient,
            ) -> pgorm::OrmResult<#returning_ty>
            where
                #returning_ty: pgorm::FromRow,
            {
                #destructure
                let sql = ::std::format!(
                    "WITH {table} AS ({insert} RETURNING *) SELECT {} FROM {table} {}",
                    #returning_ty::SELECT_LIST,
                    #returning_ty::JOIN_CLAUSE,
                    table = #table_name,
                    insert = #insert_sql,
                );
                #returning_query_expr.fetch_one_as::<#returning_ty>(conn).await
            }

            #insert_many_returning_method
        }
    } else {
        quote! {}
    };

    // Generate insert_graph methods if there are any graph declarations
    // Generate with_* setters for all fields
    let with_setters = generate_with_setters(fields);

    // Create insert SQL info for graph methods
    let insert_sql_info = InsertSqlInfo {
        sql: insert_sql.clone(),
        bind_idents: bind_field_idents.clone(),
    };

    let insert_graph_methods = generate_insert_graph_methods(
        &struct_attrs,
        &input,
        fields,
        &insert_sql_info,
    )?;

    Ok(quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            pub const TABLE: &'static str = #table_name;

            #insert_method

            #insert_many_method

            #upsert_methods

            #returning_method

            #insert_graph_methods

            #with_setters
        }
    })
}

fn get_struct_attrs(input: &DeriveInput) -> Result<StructAttrs> {
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

    Ok(StructAttrs { table, returning, conflict_target, conflict_constraint, conflict_update, graph })
}

/// Parse a graph-style attribute like `has_many(Type, field = "x", fk_field = "y")`.
fn parse_graph_attr(tokens: &TokenStream, graph: &mut GraphDeclarations) -> Result<()> {
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
    if tokens_str.starts_with("graph_root_key") && !tokens_str.starts_with("graph_root_key_source") {
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
    let pattern = format!("{} = ", key);
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
            syn::Error::new(Span::call_site(), "has_one/has_many requires field = \"...\"")
        })?;
        let fk_field = fk_field.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "has_one/has_many requires fk_field = \"...\"")
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
            syn::Error::new(Span::call_site(), "belongs_to requires set_fk_field = \"...\"")
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

fn get_field_attrs(field: &syn::Field) -> Result<FieldAttrs> {
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

// ─────────────────────────────────────────────────────────────────────────────
// insert_graph methods generation
// ─────────────────────────────────────────────────────────────────────────────

/// Generate insert_graph, insert_graph_returning, and insert_graph_report methods.
fn generate_insert_graph_methods(
    struct_attrs: &StructAttrs,
    input: &DeriveInput,
    fields: &syn::punctuated::Punctuated<syn::Field, syn::Token![,]>,
    insert_sql_info: &InsertSqlInfo,
) -> Result<TokenStream> {
    let graph = &struct_attrs.graph;

    // If no graph declarations, don't generate graph methods
    if !graph.has_any() {
        return Ok(quote! {});
    }

    // Compile-time check: has_one/has_many without graph_root_id_field requires returning type
    // because we need the root ID (via ModelPk::pk()) to inject into children.
    if !graph.has_relations.is_empty() && graph.graph_root_id_field.is_none() {
        if struct_attrs.returning.is_none() {
            let relation_names: Vec<_> = graph.has_relations.iter().map(|r| &r.field).collect();
            return Err(syn::Error::new_spanned(
                input,
                format!(
                    "InsertModel with has_one/has_many relations ({:?}) requires either: \
                    \n  1. #[orm(returning = \"YourModel\")] where YourModel implements ModelPk, or \
                    \n  2. #[orm(graph_root_id_field = \"id\")] to get root_id from input field. \
                    \nThe root ID is needed to set foreign keys on child records.",
                    relation_names,
                ),
            ));
        }
    }

    let table_name = &struct_attrs.table;

    // Collect all field idents that need to be extracted from self
    let mut all_field_idents: Vec<syn::Ident> = Vec::new();
    for field in fields.iter() {
        if let Some(ident) = &field.ident {
            all_field_idents.push(ident.clone());
        }
    }

    // Placeholder ident for root_key (not actually used now)
    let root_key_ident = format_ident!("id");

    // Generate code for belongs_to (pre-insert) steps
    let belongs_to_code = generate_belongs_to_code(graph, &root_key_ident)?;

    // Generate code for before_insert steps
    let before_insert_code = generate_insert_step_code(graph, true)?;

    // Generate code to extract graph fields before consuming self
    let (extract_graph_fields_code, direct_t_fields) = generate_extract_graph_fields_code(graph, fields)?;

    // Check if we have direct T fields (non-Option, non-Vec)
    // If so, we need to use inline SQL instead of self.insert() because we can't call .take()
    let has_direct_t_fields = !direct_t_fields.is_empty();

    // Generate code for direct T field extraction (if any)
    // For direct T fields, we need to destructure self and extract them before the SQL insert
    let direct_t_extract_code = if has_direct_t_fields {
        let mut stmts = Vec::new();
        for field_name in &direct_t_fields {
            let field_ident = format_ident!("{}", field_name);
            let extracted_ident = format_ident!("__pgorm_graph_{}", field_name);
            // Find if it's a has_relation or insert_step
            let is_has_relation = graph.has_relations.iter().any(|r| r.field == *field_name);
            if is_has_relation {
                stmts.push(quote! {
                    let #extracted_ident = ::std::option::Option::Some(#field_ident);
                });
            } else {
                let extracted_step_ident = format_ident!("__pgorm_step_{}", field_name);
                stmts.push(quote! {
                    let #extracted_step_ident = ::std::option::Option::Some(#field_ident);
                });
            }
        }
        quote! { #(#stmts)* }
    } else {
        quote! {}
    };

    // Generate code for has_one/has_many (post-insert) steps
    let has_relation_code = generate_has_relation_code(graph, &root_key_ident)?;

    // Generate code for after_insert steps
    let after_insert_code = generate_insert_step_code(graph, false)?;

    // Build tag for root insert
    let root_tag = format!("graph:root:{}", table_name);

    // Check if we need returning type
    let returning_ty = struct_attrs.returning.as_ref();

    // Determine root ID source:
    // 1. If graph_root_id_field is set, use Input mode (get from self.<field>)
    // 2. Otherwise use Returning mode (get from ModelPk::pk() on returning type)
    let has_graph_root_id_field = graph.graph_root_id_field.is_some();
    let needs_root_id = !graph.has_relations.is_empty();

    // Generate inline SQL execution code for when we have direct T fields
    // This replaces the call to self.insert() / self.insert_returning()
    let insert_sql = &insert_sql_info.sql;
    let bind_idents = &insert_sql_info.bind_idents;

    let _inline_insert_query = bind_idents.iter().fold(
        quote! { ::pgorm::query(#insert_sql) },
        |acc, ident| quote! { #acc.bind(&#ident) },
    );

    // Generate destructure pattern for direct T fields mode
    let full_destructure = quote! {
        let Self { #(#all_field_idents),* } = self;
    };

    // The pre-insert execution logic depends on whether we have direct T fields
    let pre_insert_code = if has_direct_t_fields {
        // Direct T mode: destructure self first, then extract graph fields into Option wrappers
        quote! {
            let mut __pgorm_steps: ::std::vec::Vec<::pgorm::WriteStepReport> = ::std::vec::Vec::new();
            let mut __pgorm_total_affected: u64 = 0;

            // Destructure self into field variables (allows moving direct T fields)
            #full_destructure

            // Extract graph fields for direct T types (wrap in Some)
            #direct_t_extract_code

            // Extract graph fields for Option/Vec types - these are now local variables
            // (The extract_graph_fields_code uses self.field, but we've destructured into local vars)
            // We need to generate different code for direct T mode
        }
    } else {
        // Normal mode: use &mut self and .take() methods
        quote! {
            let mut __pgorm_steps: ::std::vec::Vec<::pgorm::WriteStepReport> = ::std::vec::Vec::new();
            let mut __pgorm_total_affected: u64 = 0;

            // 0. Extract graph fields before consuming self
            #extract_graph_fields_code

            // 1. belongs_to (pre-insert): write parent tables and get their IDs
            #belongs_to_code

            // 2. before_insert steps
            #before_insert_code
        }
    };

    // For direct T mode, we need to regenerate graph field extraction to use local variables
    // Note: These are currently unused but kept for future full direct T support (option B)
    let (_direct_t_mode_extract_code, _direct_t_mode_belongs_to, _direct_t_mode_before_insert) = if has_direct_t_fields {
        // Generate extraction code that uses local variables (not self.field)
        let mut extract_stmts = Vec::new();
        for rel in &graph.has_relations {
            let field_ident = format_ident!("{}", rel.field);
            let extracted_field_ident = format_ident!("__pgorm_graph_{}", rel.field);

            // Skip direct T fields - they're already extracted above
            if direct_t_fields.contains(&rel.field) {
                continue;
            }

            // Find field type
            let field_ty = fields
                .iter()
                .find(|f| f.ident.as_ref().map(|i| i.to_string()) == Some(rel.field.clone()))
                .map(|f| &f.ty);

            if let Some(ty) = field_ty {
                if extract_option_inner_type(ty).is_some() {
                    extract_stmts.push(quote! {
                        let #extracted_field_ident = #field_ident; // Already Option<T>
                    });
                } else if extract_vec_inner_type(ty).is_some() {
                    extract_stmts.push(quote! {
                        let #extracted_field_ident = ::std::option::Option::Some(#field_ident);
                    });
                }
            }
        }

        // For after_insert steps
        for step in &graph.insert_steps {
            if !step.is_before {
                let field_ident = format_ident!("{}", step.field);
                let extracted_field_ident = format_ident!("__pgorm_step_{}", step.field);

                if direct_t_fields.contains(&step.field) {
                    continue;
                }

                let field_ty = fields
                    .iter()
                    .find(|f| f.ident.as_ref().map(|i| i.to_string()) == Some(step.field.clone()))
                    .map(|f| &f.ty);

                if let Some(ty) = field_ty {
                    if extract_option_inner_type(ty).is_some() {
                        extract_stmts.push(quote! {
                            let #extracted_field_ident = #field_ident;
                        });
                    } else if extract_vec_inner_type(ty).is_some() {
                        extract_stmts.push(quote! {
                            let #extracted_field_ident = ::std::option::Option::Some(#field_ident);
                        });
                    }
                }
            }
        }

        let extract_code = quote! { #(#extract_stmts)* };

        // Generate belongs_to code that uses local variables
        let mut bt_blocks = Vec::new();
        for bt in &graph.belongs_to {
            let field_ident = format_ident!("{}", bt.field);
            let set_fk_field_ident = format_ident!("{}", bt.set_fk_field);
            let tag = format!("graph:belongs_to:{}", bt.field);
            let field_name = &bt.field;
            let set_fk_field_name = &bt.set_fk_field;

            let insert_call = match bt.mode {
                BelongsToMode::InsertReturning => quote! { insert_returning(conn).await? },
                BelongsToMode::UpsertReturning => quote! { upsert_returning(conn).await? },
            };

            // For direct T mode, we need to use mutable local variables
            // Since belongs_to modifies set_fk_field, we need it mutable
            let code = if bt.required {
                quote! {
                    let __fk_has_value = #set_fk_field_ident.is_some();
                    let __field_has_value = #field_ident.is_some();

                    if __fk_has_value && __field_has_value {
                        return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                            ::std::format!(
                                "belongs_to '{}': '{}' and '{}' are mutually exclusive but both are set",
                                #field_name, #set_fk_field_name, #field_name
                            )
                        ));
                    } else if __fk_has_value {
                        // FK already set, skip
                    } else if let ::std::option::Option::Some(parent_data) = #field_ident {
                        let parent_result = parent_data.#insert_call;
                        let parent_id = ::pgorm::ModelPk::pk(&parent_result).clone();
                        #set_fk_field_ident = ::std::option::Option::Some(parent_id);
                        __pgorm_steps.push(::pgorm::WriteStepReport { tag: #tag, affected: 1 });
                        __pgorm_total_affected += 1;
                    } else {
                        return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                            ::std::format!(
                                "belongs_to '{}' is required but neither '{}' nor '{}' is set",
                                #field_name, #set_fk_field_name, #field_name
                            )
                        ));
                    }
                }
            } else {
                quote! {
                    let __fk_has_value = #set_fk_field_ident.is_some();
                    let __field_has_value = #field_ident.is_some();

                    if __fk_has_value && __field_has_value {
                        return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                            ::std::format!(
                                "belongs_to '{}': '{}' and '{}' are mutually exclusive but both are set",
                                #field_name, #set_fk_field_name, #field_name
                            )
                        ));
                    } else if !__fk_has_value {
                        if let ::std::option::Option::Some(parent_data) = #field_ident {
                            let parent_result = parent_data.#insert_call;
                            let parent_id = ::pgorm::ModelPk::pk(&parent_result).clone();
                            #set_fk_field_ident = ::std::option::Option::Some(parent_id);
                            __pgorm_steps.push(::pgorm::WriteStepReport { tag: #tag, affected: 1 });
                            __pgorm_total_affected += 1;
                        }
                    }
                }
            };
            bt_blocks.push(code);
        }
        let bt_code = quote! { #(#bt_blocks)* };

        // Generate before_insert code that uses local variables
        let mut bi_blocks = Vec::new();
        for step in graph.insert_steps.iter().filter(|s| s.is_before) {
            let field_ident = format_ident!("{}", step.field);
            let tag = format!("graph:before_insert:{}", step.field);

            let insert_call = match step.mode {
                StepMode::Insert => quote! {
                    let affected = step_data.insert(conn).await?;
                    __pgorm_steps.push(::pgorm::WriteStepReport { tag: #tag, affected });
                    __pgorm_total_affected += affected;
                },
                StepMode::Upsert => quote! {
                    let affected = step_data.upsert(conn).await?;
                    __pgorm_steps.push(::pgorm::WriteStepReport { tag: #tag, affected });
                    __pgorm_total_affected += affected;
                },
            };

            let field_ty = fields
                .iter()
                .find(|f| f.ident.as_ref().map(|i| i.to_string()) == Some(step.field.clone()))
                .map(|f| &f.ty);

            let code = if let Some(ty) = field_ty {
                if extract_option_inner_type(ty).is_some() {
                    quote! {
                        if let ::std::option::Option::Some(step_data) = #field_ident {
                            #insert_call
                        }
                    }
                } else if extract_vec_inner_type(ty).is_some() {
                    quote! {
                        for step_data in #field_ident {
                            #insert_call
                        }
                    }
                } else {
                    // Direct T - just use it
                    quote! {
                        let step_data = #field_ident;
                        #insert_call
                    }
                }
            } else {
                quote! {
                    if let ::std::option::Option::Some(step_data) = #field_ident {
                        #insert_call
                    }
                }
            };
            bi_blocks.push(code);
        }
        let bi_code = quote! { #(#bi_blocks)* };

        (extract_code, bt_code, bi_code)
    } else {
        (quote! {}, quote! {}, quote! {})
    };

    // Generate code to get root_id based on the source mode
    // For graph_root_id_field, we handle both T and Option<T> field types at macro expansion time
    let (get_root_id_code, needs_model_pk_bound, _extract_helper) = if let Some(ref root_id_field) = graph.graph_root_id_field {
        // Input mode: get root_id from self.<field> before insert
        // Field can be T or Option<T>. Detect at macro time.
        let root_id_field_ident = format_ident!("{}", root_id_field);
        let root_id_field_name = root_id_field.clone();

        // Find the field type
        let field_ty = fields
            .iter()
            .find(|f| f.ident.as_ref().map(|i| i.to_string()) == Some(root_id_field.clone()))
            .map(|f| &f.ty);

        let (code, helper) = if let Some(ty) = field_ty {
            if extract_option_inner_type(ty).is_some() {
                // Field is Option<T>: need to unwrap and return error if None
                let code = quote! {
                    // Get root_id from input field (graph_root_id_field mode, Option<T>)
                    let __pgorm_root_id = self.#root_id_field_ident.clone().ok_or_else(|| {
                        ::pgorm::OrmError::Validation(
                            ::std::format!("graph_root_id_field '{}' is None but required for has_* relations", #root_id_field_name)
                        )
                    })?;
                };
                (code, quote! {})
            } else {
                // Field is T: just clone it
                let code = quote! {
                    // Get root_id from input field (graph_root_id_field mode, T)
                    let __pgorm_root_id = self.#root_id_field_ident.clone();
                };
                (code, quote! {})
            }
        } else {
            // Field not found - this will cause a compile error anyway
            let code = quote! {
                let __pgorm_root_id = self.#root_id_field_ident.clone();
            };
            (code, quote! {})
        };
        (code, false, helper)
    } else if needs_root_id {
        // Returning mode: get root_id from ModelPk::pk() after insert_returning
        // This requires returning type to implement ModelPk
        (quote! {}, true, quote! {}) // root_id will be extracted after insert_returning
    } else {
        // No root_id needed (no has_relations)
        (quote! {}, false, quote! {})
    };

    // Generate the three methods with proper handling of root_id source
    let insert_graph_method = if has_graph_root_id_field {
        // Input mode: we can get root_id before insert, so we don't need returning for has_*
        let returning_handling = if let Some(ret_ty) = returning_ty {
            quote! {
                // Root insert with returning
                let __pgorm_root_result: #ret_ty = self.insert_returning(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: 1,
                });
                __pgorm_total_affected += 1;
            }
        } else {
            quote! {
                // Root insert (without returning, but we have root_id from input)
                let __pgorm_root_affected = self.insert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: __pgorm_root_affected,
                });
                __pgorm_total_affected += __pgorm_root_affected;
            }
        };

        quote! {
            /// Insert this struct and all related graph nodes.
            ///
            /// Execution order: belongs_to → before_insert → root → has_one/has_many → after_insert
            pub async fn insert_graph(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<u64> {
                #pre_insert_code

                // Get root_id from input field before insert
                #get_root_id_code

                #returning_handling

                // Execute post-insert steps with root ID
                #has_relation_code
                #after_insert_code

                ::std::result::Result::Ok(__pgorm_total_affected)
            }
        }
    } else if let Some(ret_ty) = returning_ty {
        // Returning mode with returning type configured
        let where_clause = if needs_model_pk_bound {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow + ::pgorm::ModelPk,
            }
        } else {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow,
            }
        };

        let root_id_extraction = if needs_root_id {
            quote! {
                let __pgorm_root_id = ::pgorm::ModelPk::pk(&__pgorm_root_result).clone();
            }
        } else {
            quote! {}
        };

        quote! {
            /// Insert this struct and all related graph nodes.
            ///
            /// Execution order: belongs_to → before_insert → root → has_one/has_many → after_insert
            pub async fn insert_graph(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<u64>
            #where_clause
            {
                #pre_insert_code

                // Root insert with returning to get the ID
                let __pgorm_root_result: #ret_ty = self.insert_returning(conn).await?;
                #root_id_extraction
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: 1,
                });
                __pgorm_total_affected += 1;

                // Execute post-insert steps with root ID
                #has_relation_code
                #after_insert_code

                ::std::result::Result::Ok(__pgorm_total_affected)
            }
        }
    } else {
        // No returning type - graph operations with has_one/has_many won't work
        // This case is handled by task #3 (compile error for has_* without returning)
        quote! {
            /// Insert this struct and all related graph nodes.
            ///
            /// Note: has_one/has_many relations require either `#[orm(returning = "...")]`
            /// or `#[orm(graph_root_id_field = "...")]` to be set.
            pub async fn insert_graph(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<u64> {
                #pre_insert_code

                // Root insert
                let __pgorm_root_affected = self.insert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: __pgorm_root_affected,
                });
                __pgorm_total_affected += __pgorm_root_affected;

                ::std::result::Result::Ok(__pgorm_total_affected)
            }
        }
    };

    let insert_graph_returning_method = if let Some(ret_ty) = returning_ty {
        let where_clause = if needs_model_pk_bound && !has_graph_root_id_field {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow + ::pgorm::ModelPk,
            }
        } else {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow,
            }
        };

        // Code to run before insert (for graph_root_id_field mode)
        let pre_insert_root_id = if has_graph_root_id_field {
            quote! {
                #get_root_id_code
            }
        } else {
            quote! {}
        };

        // Code to run after insert (for returning mode)
        let post_insert_root_id = if !has_graph_root_id_field && needs_root_id {
            quote! {
                let __pgorm_root_id = ::pgorm::ModelPk::pk(&__pgorm_root_result).clone();
            }
        } else {
            quote! {}
        };

        quote! {
            /// Insert this struct and all related graph nodes, returning the root row.
            ///
            /// Execution order: belongs_to → before_insert → root → has_one/has_many → after_insert
            pub async fn insert_graph_returning(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<#ret_ty>
            #where_clause
            {
                #pre_insert_code

                // Get root_id from input field before insert (if graph_root_id_field mode)
                #pre_insert_root_id

                // Root insert with returning
                let __pgorm_root_result: #ret_ty = self.insert_returning(conn).await?;

                // Get root_id from returning result (if not graph_root_id_field mode)
                #post_insert_root_id

                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: 1,
                });
                __pgorm_total_affected += 1;

                // Execute post-insert steps with root ID
                #has_relation_code
                #after_insert_code

                ::std::result::Result::Ok(__pgorm_root_result)
            }
        }
    } else {
        quote! {}
    };

    let insert_graph_report_method = if let Some(ret_ty) = returning_ty {
        let where_clause = if needs_model_pk_bound && !has_graph_root_id_field {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow + ::pgorm::ModelPk,
            }
        } else {
            quote! {
                where
                    #ret_ty: ::pgorm::FromRow,
            }
        };

        // Code to run before insert (for graph_root_id_field mode)
        let pre_insert_root_id = if has_graph_root_id_field {
            quote! {
                #get_root_id_code
            }
        } else {
            quote! {}
        };

        // Code to run after insert (for returning mode)
        let post_insert_root_id = if !has_graph_root_id_field && needs_root_id {
            quote! {
                let __pgorm_root_id = ::pgorm::ModelPk::pk(&__pgorm_root_result).clone();
            }
        } else {
            quote! {}
        };

        quote! {
            /// Insert this struct and all related graph nodes, returning a detailed report.
            ///
            /// Execution order: belongs_to → before_insert → root → has_one/has_many → after_insert
            pub async fn insert_graph_report(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<::pgorm::WriteReport<#ret_ty>>
            #where_clause
            {
                #pre_insert_code

                // Get root_id from input field before insert (if graph_root_id_field mode)
                #pre_insert_root_id

                // Root insert with returning
                let __pgorm_root_result: #ret_ty = self.insert_returning(conn).await?;

                // Get root_id from returning result (if not graph_root_id_field mode)
                #post_insert_root_id

                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: 1,
                });
                __pgorm_total_affected += 1;

                // Execute post-insert steps with root ID
                #has_relation_code
                #after_insert_code

                ::std::result::Result::Ok(::pgorm::WriteReport {
                    affected: __pgorm_total_affected,
                    steps: __pgorm_steps,
                    root: ::std::option::Option::Some(__pgorm_root_result),
                })
            }
        }
    } else if has_graph_root_id_field {
        // No returning type but we have graph_root_id_field, so we can still do has_* operations
        quote! {
            /// Insert this struct and all related graph nodes, returning a detailed report.
            ///
            /// Returns `WriteReport<()>` with `root: None` since no returning type is configured.
            pub async fn insert_graph_report(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<::pgorm::WriteReport<()>> {
                #pre_insert_code

                // Get root_id from input field before insert
                #get_root_id_code

                // Root insert (without returning)
                let __pgorm_root_affected = self.insert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: __pgorm_root_affected,
                });
                __pgorm_total_affected += __pgorm_root_affected;

                // Execute post-insert steps with root ID
                #has_relation_code
                #after_insert_code

                ::std::result::Result::Ok(::pgorm::WriteReport {
                    affected: __pgorm_total_affected,
                    steps: __pgorm_steps,
                    root: ::std::option::Option::None,
                })
            }
        }
    } else {
        // When no returning type is configured, return WriteReport<()>
        // Note: has_one/has_many won't work in this case
        quote! {
            /// Insert this struct and all related graph nodes, returning a detailed report.
            ///
            /// Note: has_one/has_many relations require `#[orm(returning = "...")]` or
            /// `#[orm(graph_root_id_field = "...")]` to be set.
            /// Without either, this method returns `WriteReport<()>` with `root: None`.
            pub async fn insert_graph_report(
                mut self,
                conn: &impl ::pgorm::GenericClient,
            ) -> ::pgorm::OrmResult<::pgorm::WriteReport<()>> {
                #pre_insert_code

                // Root insert (without returning)
                let __pgorm_root_affected = self.insert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #root_tag,
                    affected: __pgorm_root_affected,
                });
                __pgorm_total_affected += __pgorm_root_affected;

                ::std::result::Result::Ok(::pgorm::WriteReport {
                    affected: __pgorm_total_affected,
                    steps: __pgorm_steps,
                    root: ::std::option::Option::None,
                })
            }
        }
    };

    Ok(quote! {
        #insert_graph_method
        #insert_graph_returning_method
        #insert_graph_report_method
    })
}

/// Generate code to extract graph fields (has_one/has_many and after_insert) before consuming self.
///
/// For has_one/has_many fields, we support `Option<T>`, `Vec<T>`, and direct `T` types:
/// - `Option<T>`: uses `.take()` to extract
/// - `Vec<T>`: uses `std::mem::take()` to extract, wrap in Some
/// - `T`: requires special handling - see below
///
/// For direct T fields (non-Option, non-Vec), we can't use .take() because there's no
/// "empty" state. According to option B, we restructure to destructure self into field
/// variables first, then execute SQL directly. This is handled by the calling function.
///
/// This function generates extraction code for Option<T> and Vec<T> fields only.
/// Direct T fields are signaled back to the caller via the returned list.
fn generate_extract_graph_fields_code(
    graph: &GraphDeclarations,
    fields: &syn::punctuated::Punctuated<syn::Field, syn::Token![,]>,
) -> Result<(TokenStream, Vec<String>)> {
    let mut extract_stmts = Vec::new();
    let mut direct_t_fields: Vec<String> = Vec::new();

    // Helper to find field type by name
    let find_field_type = |field_name: &str| -> Option<&syn::Type> {
        fields
            .iter()
            .find(|f| f.ident.as_ref().map(|i| i.to_string()) == Some(field_name.to_string()))
            .map(|f| &f.ty)
    };

    // Extract has_relations fields
    for rel in &graph.has_relations {
        let field_ident = format_ident!("{}", rel.field);
        let extracted_field_ident = format_ident!("__pgorm_graph_{}", rel.field);

        // Detect field type at macro time
        if let Some(ty) = find_field_type(&rel.field) {
            if extract_option_inner_type(ty).is_some() {
                // Option<T>: use take()
                extract_stmts.push(quote! {
                    let #extracted_field_ident = self.#field_ident.take();
                });
            } else if extract_vec_inner_type(ty).is_some() {
                // Vec<T>: use std::mem::take() and wrap in Some
                extract_stmts.push(quote! {
                    let #extracted_field_ident = ::std::option::Option::Some(::std::mem::take(&mut self.#field_ident));
                });
            } else {
                // Direct T: signal that this needs special handling
                direct_t_fields.push(rel.field.clone());
            }
        } else {
            // Field not found - will fail at compile time
            direct_t_fields.push(rel.field.clone());
        }
    }

    // Extract after_insert step fields
    for step in &graph.insert_steps {
        if !step.is_before {
            let field_ident = format_ident!("{}", step.field);
            let extracted_field_ident = format_ident!("__pgorm_step_{}", step.field);

            if let Some(ty) = find_field_type(&step.field) {
                if extract_option_inner_type(ty).is_some() {
                    // Option<T>: use take()
                    extract_stmts.push(quote! {
                        let #extracted_field_ident = self.#field_ident.take();
                    });
                } else if extract_vec_inner_type(ty).is_some() {
                    // Vec<T>: use std::mem::take() and wrap in Some
                    extract_stmts.push(quote! {
                        let #extracted_field_ident = ::std::option::Option::Some(::std::mem::take(&mut self.#field_ident));
                    });
                } else {
                    // Direct T: signal that this needs special handling
                    direct_t_fields.push(step.field.clone());
                }
            } else {
                direct_t_fields.push(step.field.clone());
            }
        }
    }

    // Check if we need the helper trait (for before_insert steps)
    let has_before_insert = graph.insert_steps.iter().any(|s| s.is_before);

    if extract_stmts.is_empty() && !has_before_insert {
        return Ok((quote! {}, direct_t_fields));
    }

    // Generate the helper trait only if needed for before_insert
    let helper_code = if has_before_insert {
        quote! {
            // Helper trait to extract graph fields uniformly for various types
            trait __PgormTakeGraphField<T> {
                fn __pgorm_take(&mut self) -> ::std::option::Option<T>;
            }

            // Implementation for Option<T>: use take()
            impl<T> __PgormTakeGraphField<T> for ::std::option::Option<T> {
                fn __pgorm_take(&mut self) -> ::std::option::Option<T> {
                    self.take()
                }
            }

            // Implementation for Vec<T>: take ownership by replacing with empty vec, wrap in Some
            impl<T> __PgormTakeGraphField<::std::vec::Vec<T>> for ::std::vec::Vec<T> {
                fn __pgorm_take(&mut self) -> ::std::option::Option<::std::vec::Vec<T>> {
                    ::std::option::Option::Some(::std::mem::take(self))
                }
            }

            fn __pgorm_take_graph_field<T, F: __PgormTakeGraphField<T>>(field: &mut F) -> ::std::option::Option<T> {
                field.__pgorm_take()
            }
        }
    } else {
        quote! {}
    };

    Ok((quote! {
        #helper_code
        #(#extract_stmts)*
    }, direct_t_fields))
}

/// Generate code for belongs_to (pre-insert) steps.
/// Uses ModelPk::pk() to get parent ID (avoiding direct field access for cross-module compatibility).
///
/// Semantics (per doc §6.1.2):
/// - set_fk_field and field are mutually exclusive (both set => Validation error)
/// - If set_fk_field already has a value (Some), skip belongs_to write
/// - If required=true but both set_fk_field is None and field is None, return Validation error
fn generate_belongs_to_code(graph: &GraphDeclarations, _root_key_ident: &syn::Ident) -> Result<TokenStream> {
    if graph.belongs_to.is_empty() {
        return Ok(quote! {});
    }

    let mut code_blocks = Vec::new();

    for bt in &graph.belongs_to {
        let field_ident = format_ident!("{}", bt.field);
        let set_fk_field_ident = format_ident!("{}", bt.set_fk_field);
        let _parent_type = &bt.parent_type;
        let tag = format!("graph:belongs_to:{}", bt.field);
        let field_name = &bt.field;
        let set_fk_field_name = &bt.set_fk_field;

        let insert_call = match bt.mode {
            BelongsToMode::InsertReturning => quote! { insert_returning(conn).await? },
            BelongsToMode::UpsertReturning => quote! { upsert_returning(conn).await? },
        };

        let code = if bt.required {
            // Required: must have either fk field set or parent field set (but not both)
            quote! {
                // belongs_to: #field_ident (required)
                // Check mutual exclusion: set_fk_field and field cannot both have values
                let __fk_has_value = self.#set_fk_field_ident.is_some();
                let __field_has_value = self.#field_ident.is_some();

                if __fk_has_value && __field_has_value {
                    return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                        ::std::format!(
                            "belongs_to '{}': '{}' and '{}' are mutually exclusive but both are set",
                            #field_name,
                            #set_fk_field_name,
                            #field_name
                        )
                    ));
                } else if __fk_has_value {
                    // FK already set, skip belongs_to write
                } else if let ::std::option::Option::Some(parent_data) = self.#field_ident.take() {
                    let parent_result = parent_data.#insert_call;
                    let parent_id = ::pgorm::ModelPk::pk(&parent_result).clone();
                    self.#set_fk_field_ident = ::std::option::Option::Some(parent_id);
                    __pgorm_steps.push(::pgorm::WriteStepReport {
                        tag: #tag,
                        affected: 1,
                    });
                    __pgorm_total_affected += 1;
                } else {
                    return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                        ::std::format!(
                            "belongs_to '{}' is required but neither '{}' nor '{}' is set",
                            #field_name,
                            #set_fk_field_name,
                            #field_name
                        )
                    ));
                }
            }
        } else {
            // Optional: if set_fk_field already has value, skip. Otherwise process parent if present.
            // Still check mutual exclusion
            quote! {
                // belongs_to: #field_ident (optional)
                // Check mutual exclusion: set_fk_field and field cannot both have values
                let __fk_has_value = self.#set_fk_field_ident.is_some();
                let __field_has_value = self.#field_ident.is_some();

                if __fk_has_value && __field_has_value {
                    return ::std::result::Result::Err(::pgorm::OrmError::Validation(
                        ::std::format!(
                            "belongs_to '{}': '{}' and '{}' are mutually exclusive but both are set",
                            #field_name,
                            #set_fk_field_name,
                            #field_name
                        )
                    ));
                } else if !__fk_has_value {
                    if let ::std::option::Option::Some(parent_data) = self.#field_ident.take() {
                        let parent_result = parent_data.#insert_call;
                        let parent_id = ::pgorm::ModelPk::pk(&parent_result).clone();
                        self.#set_fk_field_ident = ::std::option::Option::Some(parent_id);
                        __pgorm_steps.push(::pgorm::WriteStepReport {
                            tag: #tag,
                            affected: 1,
                        });
                        __pgorm_total_affected += 1;
                    }
                }
            }
        };

        code_blocks.push(code);
    }

    Ok(quote! {
        #(#code_blocks)*
    })
}

/// Generate code for before_insert/after_insert steps.
fn generate_insert_step_code(graph: &GraphDeclarations, is_before: bool) -> Result<TokenStream> {
    let steps: Vec<_> = graph
        .insert_steps
        .iter()
        .filter(|s| s.is_before == is_before)
        .collect();

    if steps.is_empty() {
        return Ok(quote! {});
    }

    let mut code_blocks = Vec::new();

    for step in steps {
        let field_ident = format_ident!("{}", step.field);
        let _step_type = &step.step_type;
        let tag = format!(
            "graph:{}:{}",
            if is_before { "before_insert" } else { "after_insert" },
            step.field
        );

        let insert_call = match step.mode {
            StepMode::Insert => quote! {
                let affected = step_data.insert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #tag,
                    affected,
                });
                __pgorm_total_affected += affected;
            },
            StepMode::Upsert => quote! {
                let affected = step_data.upsert(conn).await?;
                __pgorm_steps.push(::pgorm::WriteStepReport {
                    tag: #tag,
                    affected,
                });
                __pgorm_total_affected += affected;
            },
        };

        // For before_insert, use helper to extract (supports Option<T>, Vec<T>, etc.)
        // For after_insert, use the pre-extracted variable
        let code = if is_before {
            quote! {
                // before_insert step: #field_ident
                if let ::std::option::Option::Some(step_data) = __pgorm_take_graph_field(&mut self.#field_ident) {
                    #insert_call
                }
            }
        } else {
            let extracted_field_ident = format_ident!("__pgorm_step_{}", step.field);
            quote! {
                // after_insert step: #field_ident
                if let ::std::option::Option::Some(step_data) = #extracted_field_ident {
                    #insert_call
                }
            }
        };

        code_blocks.push(code);
    }

    Ok(quote! {
        #(#code_blocks)*
    })
}

/// Generate code for has_one/has_many (post-insert) steps.
/// Uses with_* setters to inject FK values (avoiding direct field access for cross-module compatibility).
/// Supports mode = "insert" (default) or "upsert" per doc §6.1.1.
fn generate_has_relation_code(graph: &GraphDeclarations, _root_key_ident: &syn::Ident) -> Result<TokenStream> {
    if graph.has_relations.is_empty() {
        return Ok(quote! {});
    }

    let mut code_blocks = Vec::new();

    for rel in &graph.has_relations {
        let _field_ident = format_ident!("{}", rel.field);
        let extracted_field_ident = format_ident!("__pgorm_graph_{}", rel.field);
        let child_type = &rel.child_type;
        let tag = format!(
            "graph:{}:{}",
            if rel.is_many { "has_many" } else { "has_one" },
            rel.field
        );

        // Generate setter call using with_* method
        let setter_name = format_ident!("with_{}", rel.fk_field);

        let code = if rel.is_many {
            // has_many: Vec<Child> or Option<Vec<Child>>
            let insert_call = match rel.mode {
                HasRelationMode::Insert => quote! { #child_type::insert_many(conn, children_with_fk).await? },
                HasRelationMode::Upsert => quote! { #child_type::upsert_many(conn, children_with_fk).await? },
            };
            quote! {
                // has_many: #_field_ident
                if let ::std::option::Option::Some(children) = #extracted_field_ident {
                    if !children.is_empty() {
                        // Inject root ID into each child's fk field using with_* setter
                        let children_with_fk: ::std::vec::Vec<_> = children
                            .into_iter()
                            .map(|child| child.#setter_name(__pgorm_root_id.clone()))
                            .collect();
                        // Insert/upsert all children
                        let affected = #insert_call;
                        __pgorm_steps.push(::pgorm::WriteStepReport {
                            tag: #tag,
                            affected,
                        });
                        __pgorm_total_affected += affected;
                    }
                }
            }
        } else {
            // has_one: Child or Option<Child>
            let insert_call = match rel.mode {
                HasRelationMode::Insert => quote! { child_with_fk.insert(conn).await? },
                HasRelationMode::Upsert => quote! { child_with_fk.upsert(conn).await? },
            };
            quote! {
                // has_one: #_field_ident
                if let ::std::option::Option::Some(child) = #extracted_field_ident {
                    // Inject root ID into child's fk field using with_* setter
                    let child_with_fk = child.#setter_name(__pgorm_root_id.clone());
                    // Insert/upsert child
                    let affected = #insert_call;
                    __pgorm_steps.push(::pgorm::WriteStepReport {
                        tag: #tag,
                        affected,
                    });
                    __pgorm_total_affected += affected;
                }
            }
        };

        code_blocks.push(code);
    }

    Ok(quote! {
        #(#code_blocks)*
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// with_* setters generation (§2.2)
// ─────────────────────────────────────────────────────────────────────────────

/// Generate builder-style `with_*` setters for every field.
///
/// For `T` fields:
///   - `pub fn with_<field>(mut self, v: T) -> Self`
///
/// For `Option<T>` fields:
///   - `pub fn with_<field>(mut self, v: T) -> Self` (wraps in Some)
///   - `pub fn with_<field>_opt(mut self, v: Option<T>) -> Self`
fn generate_with_setters(
    fields: &syn::punctuated::Punctuated<syn::Field, syn::Token![,]>,
) -> TokenStream {
    let mut setters = Vec::new();

    for field in fields.iter() {
        let field_ident = match &field.ident {
            Some(ident) => ident.clone(),
            None => continue,
        };
        let field_ty = &field.ty;
        let setter_name = format_ident!("with_{}", field_ident);

        // Check if the field type is Option<T>
        if let Some(inner_ty) = extract_option_inner_type(field_ty) {
            // For Option<T>: generate both with_<field>(T) and with_<field>_opt(Option<T>)
            let setter_opt_name = format_ident!("with_{}_opt", field_ident);

            setters.push(quote! {
                /// Builder-style setter: sets the field to `Some(v)`.
                #[inline]
                pub fn #setter_name(mut self, v: #inner_ty) -> Self {
                    self.#field_ident = ::std::option::Option::Some(v);
                    self
                }

                /// Builder-style setter: sets the field to the given Option value.
                #[inline]
                pub fn #setter_opt_name(mut self, v: ::std::option::Option<#inner_ty>) -> Self {
                    self.#field_ident = v;
                    self
                }
            });
        } else {
            // For non-Option types: generate with_<field>(T)
            setters.push(quote! {
                /// Builder-style setter.
                #[inline]
                pub fn #setter_name(mut self, v: #field_ty) -> Self {
                    self.#field_ident = v;
                    self
                }
            });
        }
    }

    quote! {
        #(#setters)*
    }
}

/// Extract the inner type T from Option<T>, or return None if not an Option type.
fn extract_option_inner_type(ty: &syn::Type) -> Option<&syn::Type> {
    if let syn::Type::Path(type_path) = ty {
        let path = &type_path.path;
        // Check for Option or std::option::Option or core::option::Option
        let last_segment = path.segments.last()?;
        if last_segment.ident != "Option" {
            return None;
        }
        // Extract the generic argument
        if let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments {
            if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                return Some(inner_ty);
            }
        }
    }
    None
}

/// Extract the inner type T from Vec<T>, or return None if not a Vec type.
fn extract_vec_inner_type(ty: &syn::Type) -> Option<&syn::Type> {
    if let syn::Type::Path(type_path) = ty {
        let path = &type_path.path;
        // Check for Vec or std::vec::Vec
        let last_segment = path.segments.last()?;
        if last_segment.ident != "Vec" {
            return None;
        }
        // Extract the generic argument
        if let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments {
            if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                return Some(inner_ty);
            }
        }
    }
    None
}
