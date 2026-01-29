use std::collections::HashSet;

use proc_macro2::Span;
use syn::{Error, LitStr, Result};

pub(crate) fn is_valid_sql_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn parse_sql_ident_list(
    lit: &LitStr,
    what: &str,
    allow_empty: bool,
) -> Result<Vec<String>> {
    let raw = lit.value();
    let cols: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if cols.is_empty() && !allow_empty {
        return Err(Error::new(
            lit.span(),
            format!("{what} must specify at least one column"),
        ));
    }

    let mut seen = HashSet::<String>::new();
    for col in &cols {
        if !is_valid_sql_ident(col) {
            return Err(Error::new(
                lit.span(),
                format!(
                    "{what} contains invalid SQL identifier '{col}' (expected [A-Za-z_][A-Za-z0-9_]*)"
                ),
            ));
        }
        if !seen.insert(col.clone()) {
            return Err(Error::new(
                lit.span(),
                format!("{what} contains duplicate column '{col}'"),
            ));
        }
    }

    Ok(cols)
}

pub(crate) fn parse_sql_ident(lit: &LitStr, what: &str) -> Result<String> {
    parse_sql_ident_with_span(lit.value().trim(), lit.span(), what)
}

pub(crate) fn parse_sql_ident_with_span(s: &str, span: Span, what: &str) -> Result<String> {
    let s = s.trim();
    if s.is_empty() {
        return Err(Error::new(span, format!("{what} must not be empty")));
    }
    if !is_valid_sql_ident(s) {
        return Err(Error::new(
            span,
            format!("{what} must be a valid SQL identifier (expected [A-Za-z_][A-Za-z0-9_]*)"),
        ));
    }
    Ok(s.to_string())
}
