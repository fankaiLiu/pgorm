//! Safe SQL identifier handling.
//!
//! This module provides [`Ident`] which represents a SQL identifier (schema/table/column),
//! supporting dotted notation and quoted identifiers.
//!
//! - Unquoted parts are validated against: `[A-Za-z_][A-Za-z0-9_$]*`
//! - Quoted parts allow any characters except NUL and escape `"` as `""`
//!
//! # Example
//! ```ignore
//! use pgorm::Ident;
//!
//! let t = Ident::parse("public.users")?;
//! let c = Ident::parse(r#""CamelCase"."UserTable""#)?;
//! # Ok::<(), pgorm::OrmError>(())
//! ```

use crate::error::{OrmError, OrmResult};

/// A part of a SQL identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentPart {
    /// Unquoted identifier: must match `[A-Za-z_][A-Za-z0-9_$]*`.
    Unquoted(String),
    /// Quoted identifier: allows any characters except NUL.
    Quoted(String),
}

/// A SQL identifier (column, table, or schema name).
///
/// Supports dotted notation (e.g., `schema.table.column`) and quoted identifiers
/// (e.g., `"CamelCase"."User"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub parts: Vec<IdentPart>,
}

impl Ident {
    /// Create a quoted identifier.
    pub fn quoted(name: &str) -> OrmResult<Self> {
        if name.is_empty() {
            return Err(OrmError::validation("Empty quoted identifier"));
        }
        if name.contains('\0') {
            return Err(OrmError::validation(
                "Identifier cannot contain NUL character",
            ));
        }
        Ok(Self {
            parts: vec![IdentPart::Quoted(name.to_string())],
        })
    }

    /// Parse an identifier string, supporting dotted and quoted forms.
    ///
    /// - Dotted: `schema.table.column`
    /// - Quoted: `"CamelCase"."UserTable"`
    /// - Mixed: `public."UserTable".id`
    pub fn parse(s: &str) -> OrmResult<Self> {
        if s.is_empty() {
            return Err(OrmError::validation("Identifier cannot be empty"));
        }
        if s.contains('\0') {
            return Err(OrmError::validation(
                "Identifier cannot contain NUL character",
            ));
        }

        let mut parts = Vec::new();
        let mut chars = s.chars().peekable();

        while chars.peek().is_some() {
            // Consume '.' between parts (but require there is a next part).
            if !parts.is_empty() {
                match chars.next() {
                    Some('.') => {
                        if chars.peek().is_none() {
                            return Err(OrmError::validation("Trailing '.' in identifier"));
                        }
                    }
                    Some(c) => {
                        return Err(OrmError::validation(format!(
                            "Expected '.' between identifier parts, got '{c}'"
                        )));
                    }
                    None => break,
                }
            }

            // Quoted identifier part.
            if chars.peek() == Some(&'"') {
                chars.next(); // opening quote
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('"') => {
                            // Escaped quote: ""
                            if chars.peek() == Some(&'"') {
                                chars.next();
                                name.push('"');
                            } else {
                                break;
                            }
                        }
                        Some(c) => name.push(c),
                        None => return Err(OrmError::validation("Unclosed quoted identifier")),
                    }
                }
                if name.is_empty() {
                    return Err(OrmError::validation("Empty quoted identifier"));
                }
                parts.push(IdentPart::Quoted(name));
                continue;
            }

            // Unquoted identifier part.
            let mut name = String::new();
            while let Some(&c) = chars.peek() {
                if c == '.' {
                    break;
                }
                if name.is_empty() {
                    // First char: letter or underscore.
                    if c == '_' || c.is_ascii_alphabetic() {
                        name.push(c);
                        chars.next();
                    } else {
                        return Err(OrmError::validation(format!(
                            "Invalid identifier start character: '{c}'"
                        )));
                    }
                } else {
                    // Subsequent chars: letter, digit, underscore, or $.
                    if c == '_' || c == '$' || c.is_ascii_alphanumeric() {
                        name.push(c);
                        chars.next();
                    } else {
                        return Err(OrmError::validation(format!(
                            "Invalid character in identifier: '{c}'"
                        )));
                    }
                }
            }
            if name.is_empty() {
                return Err(OrmError::validation("Empty identifier segment"));
            }
            parts.push(IdentPart::Unquoted(name));
        }

        if parts.is_empty() {
            return Err(OrmError::validation("Empty identifier"));
        }

        Ok(Self { parts })
    }

    /// Render the identifier as SQL.
    pub fn to_sql(&self) -> String {
        let mut cap = self.parts.len().saturating_sub(1); // dots
        for part in &self.parts {
            match part {
                IdentPart::Unquoted(s) => cap += s.len(),
                IdentPart::Quoted(s) => cap += s.len() + 2, // surrounding quotes (escapes may add more)
            }
        }
        let mut out = String::with_capacity(cap);
        self.write_sql(&mut out);
        out
    }

    pub(crate) fn write_sql(&self, out: &mut String) {
        for (i, part) in self.parts.iter().enumerate() {
            if i > 0 {
                out.push('.');
            }
            match part {
                IdentPart::Unquoted(s) => out.push_str(s),
                IdentPart::Quoted(s) => {
                    out.push('"');
                    for ch in s.chars() {
                        if ch == '"' {
                            out.push('"');
                            out.push('"');
                        } else {
                            out.push(ch);
                        }
                    }
                    out.push('"');
                }
            }
        }
    }
}

/// Convert an input into an [`Ident`].
///
/// This is mainly for ergonomics in builder APIs.
pub trait IntoIdent {
    fn into_ident(self) -> OrmResult<Ident>;
}

impl IntoIdent for Ident {
    fn into_ident(self) -> OrmResult<Ident> {
        Ok(self)
    }
}

impl IntoIdent for &Ident {
    fn into_ident(self) -> OrmResult<Ident> {
        Ok(self.clone())
    }
}

impl IntoIdent for &str {
    fn into_ident(self) -> OrmResult<Ident> {
        Ident::parse(self)
    }
}

impl IntoIdent for String {
    fn into_ident(self) -> OrmResult<Ident> {
        Ident::parse(&self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ident_simple() {
        let ident = Ident::parse("users").unwrap();
        assert_eq!(ident.to_sql(), "users");
    }

    #[test]
    fn ident_dotted() {
        let ident = Ident::parse("public.users").unwrap();
        assert_eq!(ident.to_sql(), "public.users");
    }

    #[test]
    fn ident_three_parts() {
        let ident = Ident::parse("schema.table.column").unwrap();
        assert_eq!(ident.to_sql(), "schema.table.column");
    }

    #[test]
    fn ident_quoted() {
        let ident = Ident::parse(r#""CamelCase""#).unwrap();
        assert_eq!(ident.to_sql(), r#""CamelCase""#);
    }

    #[test]
    fn ident_quoted_with_escape() {
        let ident = Ident::parse(r#""has""quote""#).unwrap();
        assert_eq!(ident.to_sql(), r#""has""quote""#);
    }

    #[test]
    fn ident_mixed_quoted_unquoted() {
        let ident = Ident::parse(r#"public."UserTable".id"#).unwrap();
        assert_eq!(ident.to_sql(), r#"public."UserTable".id"#);
    }

    #[test]
    fn ident_with_dollar() {
        let ident = Ident::parse("my_var$1").unwrap();
        assert_eq!(ident.to_sql(), "my_var$1");
    }

    #[test]
    fn ident_rejects_empty() {
        assert!(Ident::parse("").is_err());
    }

    #[test]
    fn ident_rejects_start_digit() {
        assert!(Ident::parse("1table").is_err());
    }

    #[test]
    fn ident_rejects_space() {
        assert!(Ident::parse("my table").is_err());
    }

    #[test]
    fn ident_rejects_double_dot() {
        assert!(Ident::parse("schema..table").is_err());
    }

    #[test]
    fn ident_rejects_trailing_dot() {
        assert!(Ident::parse("schema.").is_err());
    }

    #[test]
    fn ident_rejects_unclosed_quote() {
        assert!(Ident::parse(r#""unclosed"#).is_err());
    }
}
