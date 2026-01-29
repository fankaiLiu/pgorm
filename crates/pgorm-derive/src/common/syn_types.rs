//! Type helper utilities for syn type analysis.

/// Extract the inner type T from Option<T>, or return None if not an Option type.
///
/// Recognizes `Option<T>`, `std::option::Option<T>`, and `core::option::Option<T>`.
pub fn option_inner(ty: &syn::Type) -> Option<&syn::Type> {
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

/// Extract the inner type T from Vec<T>, or return None if not a Vec type.
///
/// Recognizes `Vec<T>` and `std::vec::Vec<T>`.
pub fn vec_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    let seg = type_path.path.segments.last()?;
    if seg.ident != "Vec" {
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

/// Check if a type is a supported chrono timestamp type for auto_now/auto_now_add.
///
/// Returns `Some(AutoTimestampKind)` if the type is:
/// - `Option<DateTime<Utc>>` or `Option<chrono::DateTime<chrono::Utc>>`
/// - `Option<NaiveDateTime>` or `Option<chrono::NaiveDateTime>`
///
/// Returns `None` if the type is not a supported timestamp type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTimestampKind {
    /// `Option<DateTime<Utc>>` - uses `chrono::Utc::now()`
    DateTimeUtc,
    /// `Option<NaiveDateTime>` - uses `chrono::Utc::now().naive_utc()`
    NaiveDateTime,
}

pub fn detect_auto_timestamp_type(ty: &syn::Type) -> Option<AutoTimestampKind> {
    // Must be Option<T>
    let inner = option_inner(ty)?;

    let syn::Type::Path(type_path) = inner else {
        return None;
    };

    let seg = type_path.path.segments.last()?;
    let ident_str = seg.ident.to_string();

    match ident_str.as_str() {
        "DateTime" => {
            // Check if it's DateTime<Utc>
            let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
                return None;
            };
            if args.args.len() != 1 {
                return None;
            }
            let syn::GenericArgument::Type(inner_ty) = args.args.first()? else {
                return None;
            };
            let syn::Type::Path(inner_path) = inner_ty else {
                return None;
            };
            let inner_seg = inner_path.path.segments.last()?;
            if inner_seg.ident == "Utc" {
                Some(AutoTimestampKind::DateTimeUtc)
            } else {
                None
            }
        }
        "NaiveDateTime" => Some(AutoTimestampKind::NaiveDateTime),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn test_option_inner() {
        let ty: syn::Type = parse_quote!(Option<String>);
        assert!(option_inner(&ty).is_some());

        let ty: syn::Type = parse_quote!(std::option::Option<i32>);
        assert!(option_inner(&ty).is_some());

        let ty: syn::Type = parse_quote!(String);
        assert!(option_inner(&ty).is_none());

        let ty: syn::Type = parse_quote!(Vec<String>);
        assert!(option_inner(&ty).is_none());
    }

    #[test]
    fn test_vec_inner() {
        let ty: syn::Type = parse_quote!(Vec<String>);
        assert!(vec_inner(&ty).is_some());

        let ty: syn::Type = parse_quote!(std::vec::Vec<i32>);
        assert!(vec_inner(&ty).is_some());

        let ty: syn::Type = parse_quote!(String);
        assert!(vec_inner(&ty).is_none());

        let ty: syn::Type = parse_quote!(Option<String>);
        assert!(vec_inner(&ty).is_none());
    }

    #[test]
    fn test_detect_auto_timestamp_type() {
        // Option<DateTime<Utc>>
        let ty: syn::Type = parse_quote!(Option<DateTime<Utc>>);
        assert_eq!(
            detect_auto_timestamp_type(&ty),
            Some(AutoTimestampKind::DateTimeUtc)
        );

        // Option<chrono::DateTime<chrono::Utc>>
        let ty: syn::Type = parse_quote!(Option<chrono::DateTime<chrono::Utc>>);
        assert_eq!(
            detect_auto_timestamp_type(&ty),
            Some(AutoTimestampKind::DateTimeUtc)
        );

        // Option<NaiveDateTime>
        let ty: syn::Type = parse_quote!(Option<NaiveDateTime>);
        assert_eq!(
            detect_auto_timestamp_type(&ty),
            Some(AutoTimestampKind::NaiveDateTime)
        );

        // Option<chrono::NaiveDateTime>
        let ty: syn::Type = parse_quote!(Option<chrono::NaiveDateTime>);
        assert_eq!(
            detect_auto_timestamp_type(&ty),
            Some(AutoTimestampKind::NaiveDateTime)
        );

        // Not supported types
        let ty: syn::Type = parse_quote!(Option<String>);
        assert_eq!(detect_auto_timestamp_type(&ty), None);

        let ty: syn::Type = parse_quote!(DateTime<Utc>);
        assert_eq!(detect_auto_timestamp_type(&ty), None);

        let ty: syn::Type = parse_quote!(String);
        assert_eq!(detect_auto_timestamp_type(&ty), None);
    }
}
