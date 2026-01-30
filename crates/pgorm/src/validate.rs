//! Validation helpers used by derive-generated Input structs.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Best-effort email validation.
///
/// This is intentionally not fully RFC-compliant. If you need stricter rules,
/// use `#[orm(custom = "...")]`.
pub fn is_email(s: &str) -> bool {
    static EMAIL_RE: OnceLock<regex::Regex> = OnceLock::new();
    EMAIL_RE
        .get_or_init(|| {
            regex::Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$").expect("invalid built-in email regex")
        })
        .is_match(s)
}

/// Returns `true` if `value` matches the provided regex `pattern`.
///
/// # Panics
/// Panics if `pattern` is not a valid regex. This is considered a developer
/// configuration error.
pub fn regex_is_match(pattern: &'static str, value: &str) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, regex::Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    let regex = {
        let mut cache = cache.lock().expect("regex cache poisoned");
        if let Some(re) = cache.get(pattern) {
            re.clone()
        } else {
            let re = regex::Regex::new(pattern)
                .unwrap_or_else(|e| panic!("invalid regex pattern: {pattern:?}: {e}"));
            cache.insert(pattern, re.clone());
            re
        }
    };

    regex.is_match(value)
}

pub fn is_url(s: &str) -> bool {
    url::Url::parse(s).is_ok()
}

pub fn parse_url(s: &str) -> Result<url::Url, url::ParseError> {
    url::Url::parse(s)
}

pub fn is_uuid(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok()
}

pub fn parse_uuid(s: &str) -> Result<uuid::Uuid, uuid::Error> {
    uuid::Uuid::parse_str(s)
}
