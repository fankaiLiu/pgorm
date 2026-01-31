//! QueryParams derive macro implementation.
//!
//! This macro generates `apply()`/`into_query()` helpers for a "query params" struct,
//! reducing boilerplate when building dynamic queries from `Option<T>` inputs.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Result};

use crate::common::syn_types::{option_inner, vec_inner};

#[derive(Clone)]
struct FilterOp {
    kind: FilterOpKind,
    col: Option<syn::Expr>,
    map: Option<syn::Expr>,
    per_page: Option<syn::Expr>,
    force_string: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterOpKind {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Like,
    Ilike,
    NotLike,
    NotIlike,
    IsNull,
    IsNotNull,
    InList,
    NotIn,
    Between,
    NotBetween,
    // Ordering / Pagination
    OrderBy,
    OrderByAsc,
    OrderByDesc,
    OrderByRaw,
    Paginate,
    Limit,
    Offset,
    Page,
    Raw,
    And,
    Or,
}

fn parse_struct_model_attr(input: &DeriveInput) -> Result<syn::Path> {
    for attr in &input.attrs {
        if !attr.path().is_ident("orm") {
            continue;
        }

        let items = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        )?;

        for meta in items {
            let syn::Meta::NameValue(nv) = meta else {
                continue;
            };
            if !nv.path.is_ident("model") {
                continue;
            }
            let syn::Expr::Lit(expr_lit) = nv.value else {
                return Err(syn::Error::new_spanned(
                    nv,
                    "orm(model = \"...\") expects a string literal",
                ));
            };
            let syn::Lit::Str(lit) = expr_lit.lit else {
                return Err(syn::Error::new_spanned(
                    expr_lit,
                    "orm(model = \"...\") expects a string literal",
                ));
            };
            let model: syn::Path = syn::parse_str(&lit.value()).map_err(|e| {
                syn::Error::new(
                    Span::call_site(),
                    format!("invalid orm(model) type path: {e}"),
                )
            })?;
            return Ok(model);
        }
    }

    Err(syn::Error::new_spanned(
        input,
        "QueryParams requires #[orm(model = \"TypePath\")]",
    ))
}

fn query_path_from_model(model: &syn::Path) -> Result<syn::Path> {
    let mut query_path = model.clone();
    let Some(last) = query_path.segments.last_mut() else {
        return Err(syn::Error::new(Span::call_site(), "empty model path"));
    };
    if !matches!(last.arguments, syn::PathArguments::None) {
        return Err(syn::Error::new_spanned(
            last,
            "orm(model) must be a plain type path (no generics)",
        ));
    }
    last.ident = format_ident!("{}Query", last.ident);
    Ok(query_path)
}

fn is_stringish(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Reference(r) => match r.elem.as_ref() {
            syn::Type::Path(p) => p.path.segments.last().is_some_and(|s| s.ident == "str"),
            _ => false,
        },
        syn::Type::Path(p) => p.path.segments.last().is_some_and(|s| s.ident == "String"),
        _ => false,
    }
}

fn is_reference_type(ty: &syn::Type) -> bool {
    matches!(ty, syn::Type::Reference(_))
}

fn parse_field_filter(field: &syn::Field) -> Result<Option<FilterOp>> {
    let mut op_kind: Option<FilterOpKind> = None;
    let mut col: Option<syn::Expr> = None;
    let mut map: Option<syn::Expr> = None;
    let mut per_page: Option<syn::Expr> = None;
    let mut force_string = false;

    let mut set_op = |new_kind: FilterOpKind| -> Result<()> {
        if op_kind.is_some() {
            return Err(syn::Error::new_spanned(
                field,
                "multiple filter ops on a single field are not supported",
            ));
        }
        op_kind = Some(new_kind);
        Ok(())
    };

    for attr in &field.attrs {
        if !attr.path().is_ident("orm") {
            continue;
        }

        let items = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        )?;

        for meta in items {
            match meta {
                syn::Meta::Path(p) => {
                    if p.is_ident("skip") {
                        return Ok(None);
                    }
                    if p.is_ident("raw") {
                        set_op(FilterOpKind::Raw)?;
                        continue;
                    }
                    if p.is_ident("and") {
                        set_op(FilterOpKind::And)?;
                        continue;
                    }
                    if p.is_ident("or") {
                        set_op(FilterOpKind::Or)?;
                        continue;
                    }
                    if p.is_ident("order_by") {
                        set_op(FilterOpKind::OrderBy)?;
                        continue;
                    }
                    if p.is_ident("order_by_asc") {
                        set_op(FilterOpKind::OrderByAsc)?;
                        continue;
                    }
                    if p.is_ident("order_by_desc") {
                        set_op(FilterOpKind::OrderByDesc)?;
                        continue;
                    }
                    if p.is_ident("order_by_raw") {
                        set_op(FilterOpKind::OrderByRaw)?;
                        continue;
                    }
                    if p.is_ident("paginate") {
                        set_op(FilterOpKind::Paginate)?;
                        continue;
                    }
                    if p.is_ident("limit") {
                        set_op(FilterOpKind::Limit)?;
                        continue;
                    }
                    if p.is_ident("offset") {
                        set_op(FilterOpKind::Offset)?;
                        continue;
                    }
                    if p.is_ident("page") {
                        set_op(FilterOpKind::Page)?;
                        continue;
                    }
                }
                syn::Meta::List(list) => {
                    let ident = list.path.get_ident().map(|i| i.to_string());
                    let Some(ident) = ident else {
                        continue;
                    };

                    match ident.as_str() {
                        "map" => {
                            let map_expr: syn::Expr = list.parse_args()?;
                            if map.is_some() {
                                return Err(syn::Error::new_spanned(
                                    list,
                                    "map(...) can only be specified once",
                                ));
                            }
                            map = Some(map_expr);
                        }
                        "page" => {
                            set_op(FilterOpKind::Page)?;
                            let items = list.parse_args_with(
                                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                            )?;
                            for meta in items {
                                let syn::Meta::NameValue(nv) = meta else {
                                    return Err(syn::Error::new_spanned(
                                        meta,
                                        "page(...) only supports name-value args like per_page = 20",
                                    ));
                                };
                                if nv.path.is_ident("per_page") {
                                    if per_page.is_some() {
                                        return Err(syn::Error::new_spanned(
                                            nv,
                                            "per_page can only be specified once",
                                        ));
                                    }
                                    per_page = Some(nv.value);
                                } else {
                                    return Err(syn::Error::new_spanned(
                                        nv,
                                        "unknown page(...) argument (supported: per_page = <expr>)",
                                    ));
                                }
                            }
                        }
                        "eq" => {
                            set_op(FilterOpKind::Eq)?;
                            col = Some(list.parse_args()?);
                        }
                        "eq_str" => {
                            set_op(FilterOpKind::Eq)?;
                            force_string = true;
                            col = Some(list.parse_args()?);
                        }
                        "eq_map" => {
                            let args = list.parse_args_with(
                                syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
                            )?;
                            if args.len() != 2 {
                                return Err(syn::Error::new_spanned(
                                    list,
                                    "eq_map expects 2 args: eq_map(column, map_fn)",
                                ));
                            }
                            let mut it = args.into_iter();
                            col = Some(it.next().unwrap());
                            if map.is_some() {
                                return Err(syn::Error::new_spanned(
                                    list,
                                    "map(...) can only be specified once",
                                ));
                            }
                            map = Some(it.next().unwrap());
                            set_op(FilterOpKind::Eq)?;
                        }
                        "ne" => {
                            set_op(FilterOpKind::Ne)?;
                            col = Some(list.parse_args()?);
                        }
                        "gt" => {
                            set_op(FilterOpKind::Gt)?;
                            col = Some(list.parse_args()?);
                        }
                        "gte" => {
                            set_op(FilterOpKind::Gte)?;
                            col = Some(list.parse_args()?);
                        }
                        "lt" => {
                            set_op(FilterOpKind::Lt)?;
                            col = Some(list.parse_args()?);
                        }
                        "lte" => {
                            set_op(FilterOpKind::Lte)?;
                            col = Some(list.parse_args()?);
                        }
                        "like" => {
                            set_op(FilterOpKind::Like)?;
                            col = Some(list.parse_args()?);
                        }
                        "ilike" => {
                            set_op(FilterOpKind::Ilike)?;
                            col = Some(list.parse_args()?);
                        }
                        "not_like" => {
                            set_op(FilterOpKind::NotLike)?;
                            col = Some(list.parse_args()?);
                        }
                        "not_ilike" => {
                            set_op(FilterOpKind::NotIlike)?;
                            col = Some(list.parse_args()?);
                        }
                        "is_null" => {
                            set_op(FilterOpKind::IsNull)?;
                            col = Some(list.parse_args()?);
                        }
                        "is_not_null" => {
                            set_op(FilterOpKind::IsNotNull)?;
                            col = Some(list.parse_args()?);
                        }
                        "in_list" => {
                            set_op(FilterOpKind::InList)?;
                            col = Some(list.parse_args()?);
                        }
                        "not_in" => {
                            set_op(FilterOpKind::NotIn)?;
                            col = Some(list.parse_args()?);
                        }
                        "between" => {
                            set_op(FilterOpKind::Between)?;
                            col = Some(list.parse_args()?);
                        }
                        "not_between" => {
                            set_op(FilterOpKind::NotBetween)?;
                            col = Some(list.parse_args()?);
                        }
                        _ => continue,
                    }
                }
                syn::Meta::NameValue(_) => {}
            }
        }
    }

    let Some(kind) = op_kind else {
        return Ok(None);
    };

    // Validate "only one op per field".
    // We can't detect duplicates perfectly from the loop above without more state,
    // but we can catch most cases by checking if multiple col(...) were set for
    // different ops (col overwritten).
    // If you need multiple filters, split them into multiple fields.
    Ok(Some(FilterOp {
        kind,
        col,
        map,
        per_page,
        force_string,
    }))
}

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let model = parse_struct_model_attr(&input)?;
    let query = query_path_from_model(&model)?;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "QueryParams can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "QueryParams can only be derived for structs",
            ));
        }
    };

    let all_field_idents: Vec<syn::Ident> = fields.iter().filter_map(|f| f.ident.clone()).collect();
    let mut apply_stmts: Vec<TokenStream> = Vec::new();

    for field in fields {
        let Some(field_ident) = field.ident.clone() else {
            continue;
        };

        let Some(filter) = parse_field_filter(field)? else {
            continue;
        };

        let field_ty = &field.ty;
        let opt_inner = option_inner(field_ty);
        let field_is_option = opt_inner.is_some();

        let kind = filter.kind;
        let col = filter.col.clone();
        let map_expr = filter.map.clone();
        let per_page_expr = filter.per_page.clone();
        let force_string = filter.force_string;

        let col_required = matches!(
            kind,
            FilterOpKind::Eq
                | FilterOpKind::Ne
                | FilterOpKind::Gt
                | FilterOpKind::Gte
                | FilterOpKind::Lt
                | FilterOpKind::Lte
                | FilterOpKind::Like
                | FilterOpKind::Ilike
                | FilterOpKind::NotLike
                | FilterOpKind::NotIlike
                | FilterOpKind::IsNull
                | FilterOpKind::IsNotNull
                | FilterOpKind::InList
                | FilterOpKind::NotIn
                | FilterOpKind::Between
                | FilterOpKind::NotBetween
        );

        if col_required && col.is_none() {
            return Err(syn::Error::new_spanned(
                field,
                "this operation requires a column argument, e.g. #[orm(eq(ModelQuery::COL_FOO))]",
            ));
        }

        if !col_required && col.is_some() {
            return Err(syn::Error::new_spanned(
                field,
                "this operation does not take a column argument",
            ));
        }

        let stmt = match kind {
            FilterOpKind::Eq
            | FilterOpKind::Ne
            | FilterOpKind::Gt
            | FilterOpKind::Gte
            | FilterOpKind::Lt
            | FilterOpKind::Lte
            | FilterOpKind::Like
            | FilterOpKind::Ilike
            | FilterOpKind::NotLike
            | FilterOpKind::NotIlike => {
                let col_expr = col.clone().unwrap();
                let method = match kind {
                    FilterOpKind::Eq => quote!(eq),
                    FilterOpKind::Ne => quote!(ne),
                    FilterOpKind::Gt => quote!(gt),
                    FilterOpKind::Gte => quote!(gte),
                    FilterOpKind::Lt => quote!(lt),
                    FilterOpKind::Lte => quote!(lte),
                    FilterOpKind::Like => quote!(like),
                    FilterOpKind::Ilike => quote!(ilike),
                    FilterOpKind::NotLike => quote!(not_like),
                    FilterOpKind::NotIlike => quote!(not_ilike),
                    _ => unreachable!(),
                };

                if let Some(map_expr) = map_expr {
                    if field_is_option {
                        quote! {
                            let q = q.apply_if_some(#field_ident, |q, v| {
                                match (#map_expr)(v) {
                                    ::std::option::Option::Some(vv) => q.#method(#col_expr, vv),
                                    ::std::option::Option::None => ::std::result::Result::Ok(q),
                                }
                            })?;
                        }
                    } else {
                        quote! {
                            let q = match (#map_expr)(#field_ident) {
                                ::std::option::Option::Some(v) => q.#method(#col_expr, v)?,
                                ::std::option::Option::None => q,
                            };
                        }
                    }
                } else if let Some(inner) = opt_inner {
                    if is_stringish(inner) {
                        // &str/String inside Option: convert to owned for `'static`.
                        if matches!(kind, FilterOpKind::Eq) && !force_string {
                            quote! { let q = q.eq_opt_str(#col_expr, #field_ident)?; }
                        } else {
                            quote! {
                                let q = q.apply_if_some(#field_ident, |q, v| q.#method(#col_expr, ::std::string::ToString::to_string(&v)))?;
                            }
                        }
                    } else if is_reference_type(inner) {
                        return Err(syn::Error::new_spanned(
                            field,
                            "Option<&T> is not supported here (use owned types or map(...) to convert)",
                        ));
                    } else if matches!(kind, FilterOpKind::Eq) {
                        quote! { let q = q.eq_opt(#col_expr, #field_ident)?; }
                    } else if matches!(kind, FilterOpKind::Gte) {
                        quote! { let q = q.gte_opt(#col_expr, #field_ident)?; }
                    } else if matches!(kind, FilterOpKind::Lte) {
                        quote! { let q = q.lte_opt(#col_expr, #field_ident)?; }
                    } else {
                        quote! { let q = q.apply_if_some(#field_ident, |q, v| q.#method(#col_expr, v))?; }
                    }
                } else if is_stringish(field_ty) {
                    if matches!(kind, FilterOpKind::Eq) && !force_string {
                        quote! { let q = q.eq_str(#col_expr, #field_ident)?; }
                    } else if is_reference_type(field_ty) {
                        quote! { let q = q.#method(#col_expr, ::std::string::ToString::to_string(&#field_ident))?; }
                    } else {
                        quote! { let q = q.#method(#col_expr, #field_ident)?; }
                    }
                } else if is_reference_type(field_ty) {
                    return Err(syn::Error::new_spanned(
                        field,
                        "&T is not supported here (use owned types or map(...) to convert)",
                    ));
                } else {
                    quote! { let q = q.#method(#col_expr, #field_ident)?; }
                }
            }
            FilterOpKind::IsNull | FilterOpKind::IsNotNull => {
                let col_expr = col.clone().unwrap();
                let method = match kind {
                    FilterOpKind::IsNull => quote!(is_null),
                    FilterOpKind::IsNotNull => quote!(is_not_null),
                    _ => unreachable!(),
                };

                let is_bool = matches!(field_ty, syn::Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "bool"));
                let is_opt_bool = opt_inner.is_some_and(|inner| matches!(inner, syn::Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "bool")));

                if is_bool {
                    quote! { let q = q.apply_if(#field_ident, |q| q.#method(#col_expr))?; }
                } else if is_opt_bool {
                    quote! { let q = q.apply_if(#field_ident == ::std::option::Option::Some(true), |q| q.#method(#col_expr))?; }
                } else {
                    return Err(syn::Error::new_spanned(
                        field,
                        "is_null/is_not_null requires a bool or Option<bool> field",
                    ));
                }
            }
            FilterOpKind::InList | FilterOpKind::NotIn => {
                let col_expr = col.clone().unwrap();
                let method = match kind {
                    FilterOpKind::InList => quote!(in_list),
                    FilterOpKind::NotIn => quote!(not_in),
                    _ => unreachable!(),
                };

                if let Some(inner) = opt_inner {
                    let Some(_) = vec_inner(inner) else {
                        return Err(syn::Error::new_spanned(
                            field,
                            "in_list/not_in requires Vec<T> or Option<Vec<T>>",
                        ));
                    };
                    quote! { let q = q.apply_if_some(#field_ident, |q, v| q.#method(#col_expr, v))?; }
                } else {
                    let Some(_) = vec_inner(field_ty) else {
                        return Err(syn::Error::new_spanned(
                            field,
                            "in_list/not_in requires Vec<T> or Option<Vec<T>>",
                        ));
                    };
                    quote! { let q = q.#method(#col_expr, #field_ident)?; }
                }
            }
            FilterOpKind::Between | FilterOpKind::NotBetween => {
                let col_expr = col.clone().unwrap();
                let method = match kind {
                    FilterOpKind::Between => quote!(between),
                    FilterOpKind::NotBetween => quote!(not_between),
                    _ => unreachable!(),
                };

                let tuple_inner = if let Some(inner) = opt_inner {
                    inner
                } else {
                    field_ty
                };
                let syn::Type::Tuple(tuple) = tuple_inner else {
                    return Err(syn::Error::new_spanned(
                        field,
                        "between/not_between requires (T, T) or Option<(T, T)>",
                    ));
                };
                if tuple.elems.len() != 2 {
                    return Err(syn::Error::new_spanned(
                        field,
                        "between/not_between requires a 2-tuple: (T, T)",
                    ));
                }

                if let Some(map_expr) = map_expr {
                    if field_is_option {
                        quote! {
                            let q = q.apply_if_some(#field_ident, |q, v| {
                                match (#map_expr)(v) {
                                    ::std::option::Option::Some((from, to)) => q.#method(#col_expr, from, to),
                                    ::std::option::Option::None => ::std::result::Result::Ok(q),
                                }
                            })?;
                        }
                    } else {
                        quote! {
                            let q = match (#map_expr)(#field_ident) {
                                ::std::option::Option::Some((from, to)) => q.#method(#col_expr, from, to)?,
                                ::std::option::Option::None => q,
                            };
                        }
                    }
                } else if field_is_option {
                    quote! { let q = q.apply_if_some(#field_ident, |q, (from, to)| q.#method(#col_expr, from, to))?; }
                } else {
                    quote! {
                        let (from, to) = #field_ident;
                        let q = q.#method(#col_expr, from, to)?;
                    }
                }
            }
            FilterOpKind::OrderBy => {
                if let Some(map_expr) = map_expr {
                    if field_is_option {
                        quote! {
                            let q = q.apply_if_some(#field_ident, |q, v| {
                                match (#map_expr)(v) {
                                    ::std::option::Option::Some(vv) => ::std::result::Result::Ok(q.order_by(vv)),
                                    ::std::option::Option::None => ::std::result::Result::Ok(q),
                                }
                            })?;
                        }
                    } else {
                        quote! {
                            let q = match (#map_expr)(#field_ident) {
                                ::std::option::Option::Some(v) => q.order_by(v),
                                ::std::option::Option::None => q,
                            };
                        }
                    }
                } else if field_is_option {
                    quote! { let q = q.apply_if_some(#field_ident, |q, v| ::std::result::Result::Ok(q.order_by(v)))?; }
                } else {
                    quote! { let q = q.order_by(#field_ident); }
                }
            }
            FilterOpKind::OrderByAsc | FilterOpKind::OrderByDesc => {
                let method = match kind {
                    FilterOpKind::OrderByAsc => quote!(order_by_asc),
                    FilterOpKind::OrderByDesc => quote!(order_by_desc),
                    _ => unreachable!(),
                };

                if let Some(map_expr) = map_expr {
                    if field_is_option {
                        quote! {
                            let q = q.apply_if_some(#field_ident, |q, v| {
                                match (#map_expr)(v) {
                                    ::std::option::Option::Some(vv) => q.#method(vv),
                                    ::std::option::Option::None => ::std::result::Result::Ok(q),
                                }
                            })?;
                        }
                    } else {
                        quote! {
                            let q = match (#map_expr)(#field_ident) {
                                ::std::option::Option::Some(v) => q.#method(v)?,
                                ::std::option::Option::None => q,
                            };
                        }
                    }
                } else if field_is_option {
                    quote! { let q = q.apply_if_some(#field_ident, |q, v| q.#method(v))?; }
                } else {
                    quote! { let q = q.#method(#field_ident)?; }
                }
            }
            FilterOpKind::OrderByRaw => {
                if let Some(map_expr) = map_expr {
                    if field_is_option {
                        quote! {
                            let q = q.apply_if_some(#field_ident, |q, v| {
                                match (#map_expr)(v) {
                                    ::std::option::Option::Some(vv) => ::std::result::Result::Ok(q.order_by_raw(vv)),
                                    ::std::option::Option::None => ::std::result::Result::Ok(q),
                                }
                            })?;
                        }
                    } else {
                        quote! {
                            let q = match (#map_expr)(#field_ident) {
                                ::std::option::Option::Some(v) => q.order_by_raw(v),
                                ::std::option::Option::None => q,
                            };
                        }
                    }
                } else if field_is_option {
                    quote! { let q = q.apply_if_some(#field_ident, |q, v| ::std::result::Result::Ok(q.order_by_raw(v)))?; }
                } else {
                    quote! { let q = q.order_by_raw(#field_ident); }
                }
            }
            FilterOpKind::Paginate => {
                if let Some(map_expr) = map_expr {
                    if field_is_option {
                        quote! {
                            let q = q.apply_if_some(#field_ident, |q, v| {
                                match (#map_expr)(v) {
                                    ::std::option::Option::Some(vv) => ::std::result::Result::Ok(q.paginate(vv)),
                                    ::std::option::Option::None => ::std::result::Result::Ok(q),
                                }
                            })?;
                        }
                    } else {
                        quote! {
                            let q = match (#map_expr)(#field_ident) {
                                ::std::option::Option::Some(v) => q.paginate(v),
                                ::std::option::Option::None => q,
                            };
                        }
                    }
                } else if field_is_option {
                    quote! { let q = q.apply_if_some(#field_ident, |q, v| ::std::result::Result::Ok(q.paginate(v)))?; }
                } else {
                    quote! { let q = q.paginate(#field_ident); }
                }
            }
            FilterOpKind::Limit | FilterOpKind::Offset => {
                let method = match kind {
                    FilterOpKind::Limit => quote!(limit),
                    FilterOpKind::Offset => quote!(offset),
                    _ => unreachable!(),
                };

                if let Some(map_expr) = map_expr {
                    if field_is_option {
                        quote! {
                            let q = q.apply_if_some(#field_ident, |q, v| {
                                match (#map_expr)(v) {
                                    ::std::option::Option::Some(vv) => ::std::result::Result::Ok(q.#method(vv)),
                                    ::std::option::Option::None => ::std::result::Result::Ok(q),
                                }
                            })?;
                        }
                    } else {
                        quote! {
                            let q = match (#map_expr)(#field_ident) {
                                ::std::option::Option::Some(v) => q.#method(v),
                                ::std::option::Option::None => q,
                            };
                        }
                    }
                } else if field_is_option {
                    quote! { let q = q.apply_if_some(#field_ident, |q, v| ::std::result::Result::Ok(q.#method(v)))?; }
                } else {
                    quote! { let q = q.#method(#field_ident); }
                }
            }
            FilterOpKind::Page => {
                let tuple_ty = if let Some(inner) = opt_inner {
                    inner
                } else {
                    field_ty
                };
                let is_tuple = matches!(tuple_ty, syn::Type::Tuple(t) if t.elems.len() == 2);

                if is_tuple {
                    if per_page_expr.is_some() {
                        return Err(syn::Error::new_spanned(
                            field,
                            "page(per_page = ...) cannot be used on a tuple field; use a page number field instead",
                        ));
                    }

                    if let Some(map_expr) = map_expr {
                        if field_is_option {
                            quote! {
                                let q = q.apply_if_some(#field_ident, |q, v| {
                                    match (#map_expr)(v) {
                                        ::std::option::Option::Some((page, per_page)) => q.page(page, per_page),
                                        ::std::option::Option::None => ::std::result::Result::Ok(q),
                                    }
                                })?;
                            }
                        } else {
                            quote! {
                                let q = match (#map_expr)(#field_ident) {
                                    ::std::option::Option::Some((page, per_page)) => q.page(page, per_page)?,
                                    ::std::option::Option::None => q,
                                };
                            }
                        }
                    } else if field_is_option {
                        quote! { let q = q.apply_if_some(#field_ident, |q, (page, per_page)| q.page(page, per_page))?; }
                    } else {
                        quote! {
                            let (page, per_page) = #field_ident;
                            let q = q.page(page, per_page)?;
                        }
                    }
                } else {
                    // page number + per_page expr
                    let Some(per_page_expr) = per_page_expr else {
                        return Err(syn::Error::new_spanned(
                            field,
                            "page requires (i64, i64) / Option<(i64, i64)> or #[orm(page(per_page = ...))] on an i64/Option<i64> field",
                        ));
                    };

                    if let Some(map_expr) = map_expr {
                        if field_is_option {
                            quote! {
                                let q = q.apply_if_some(#field_ident, |q, v| {
                                    match (#map_expr)(v) {
                                        ::std::option::Option::Some(vv) => q.page(vv, #per_page_expr),
                                        ::std::option::Option::None => ::std::result::Result::Ok(q),
                                    }
                                })?;
                            }
                        } else {
                            quote! {
                                let q = match (#map_expr)(#field_ident) {
                                    ::std::option::Option::Some(v) => q.page(v, #per_page_expr)?,
                                    ::std::option::Option::None => q,
                                };
                            }
                        }
                    } else if field_is_option {
                        quote! { let q = q.apply_if_some(#field_ident, |q, v| q.page(v, #per_page_expr))?; }
                    } else {
                        quote! { let q = q.page(#field_ident, #per_page_expr)?; }
                    }
                }
            }
            FilterOpKind::Raw => {
                if field_is_option {
                    quote! { let q = q.apply_if_some(#field_ident, |q, v| ::std::result::Result::Ok(q.raw(v)))?; }
                } else {
                    quote! { let q = q.raw(#field_ident); }
                }
            }
            FilterOpKind::And | FilterOpKind::Or => {
                let method = match kind {
                    FilterOpKind::And => quote!(and),
                    FilterOpKind::Or => quote!(or),
                    _ => unreachable!(),
                };
                if field_is_option {
                    quote! { let q = q.apply_if_some(#field_ident, |q, v| ::std::result::Result::Ok(q.#method(v)))?; }
                } else {
                    quote! { let q = q.#method(#field_ident); }
                }
            }
        };

        apply_stmts.push(stmt);
    }

    let destructure = quote! { let Self { #(#all_field_idents,)* } = self; };

    Ok(quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            /// Apply the params to an existing query builder.
            #[allow(unused_variables)]
            pub fn apply(self, q: #query) -> pgorm::OrmResult<#query> {
                #destructure
                #(#apply_stmts)*
                ::std::result::Result::Ok(q)
            }

            /// Build a new query builder from the model and apply the params.
            pub fn into_query(self) -> pgorm::OrmResult<#query> {
                self.apply(#model::query())
            }
        }
    })
}
