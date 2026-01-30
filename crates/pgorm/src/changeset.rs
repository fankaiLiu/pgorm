//! Changeset-style validation error types.
//!
//! This module is intentionally lightweight and framework-agnostic.

use serde::Serialize;

/// A machine-friendly validation code.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationCode {
    Required,
    Len,
    Range,
    Email,
    Regex,
    Url,
    Uuid,
    Ip,
    OneOf,
    Custom(String),
}

impl ValidationCode {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Required => "required",
            Self::Len => "len",
            Self::Range => "range",
            Self::Email => "email",
            Self::Regex => "regex",
            Self::Url => "url",
            Self::Uuid => "uuid",
            Self::Ip => "ip",
            Self::OneOf => "one_of",
            Self::Custom(s) => s.as_str(),
        }
    }
}

impl Serialize for ValidationCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// A single field validation error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationError {
    pub field: String,
    pub code: ValidationCode,
    pub message: String,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty", default)]
    pub metadata: std::collections::BTreeMap<String, serde_json::Value>,
}

impl ValidationError {
    pub fn new(field: impl Into<String>, code: ValidationCode, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            code,
            message: message.into(),
            metadata: std::collections::BTreeMap::new(),
        }
    }

    pub fn with_metadata(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// A collection of validation errors.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ValidationErrors {
    pub items: Vec<ValidationError>,
}

impl ValidationErrors {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn push(&mut self, err: ValidationError) {
        self.items.push(err);
    }

    pub fn extend(&mut self, other: Self) {
        self.items.extend(other.items);
    }

    pub fn iter(&self) -> impl Iterator<Item = &ValidationError> {
        self.items.iter()
    }
}
