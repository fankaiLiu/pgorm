use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct TypeMapper {
    /// User overrides from `[packages.types]` (normalized PG type -> Rust path/type).
    custom: BTreeMap<String, String>,
}

impl TypeMapper {
    pub fn new(custom: BTreeMap<String, String>) -> Self {
        let mut normalized = BTreeMap::new();
        for (k, v) in custom {
            normalized.insert(normalize_pg_type(&k), v);
        }
        Self { custom: normalized }
    }

    pub fn map(&self, pg_type: &str) -> String {
        let normalized = normalize_pg_type(pg_type);

        if let Some(t) = self.custom.get(&normalized) {
            return t.clone();
        }

        // Arrays from `format_type` look like `integer[]`, `uuid[]`, etc.
        if let Some(base) = normalized.strip_suffix("[]") {
            let inner = self.map(base);
            return format!("Vec<{inner}>");
        }

        match normalized.as_str() {
            "bool" | "boolean" => "bool".to_string(),

            "int2" | "smallint" => "i16".to_string(),
            "int4" | "integer" | "serial" => "i32".to_string(),
            "int8" | "bigint" | "bigserial" => "i64".to_string(),

            "float4" | "real" => "f32".to_string(),
            "float8" | "double precision" => "f64".to_string(),

            "text" | "varchar" | "character varying" | "char" | "character" | "name" => {
                "String".to_string()
            }

            "uuid" => "uuid::Uuid".to_string(),
            "json" | "jsonb" => "serde_json::Value".to_string(),

            "timestamptz" | "timestamp with time zone" => {
                "chrono::DateTime<chrono::Utc>".to_string()
            }
            "timestamp" | "timestamp without time zone" => "chrono::NaiveDateTime".to_string(),
            "date" => "chrono::NaiveDate".to_string(),
            "time" | "time without time zone" => "chrono::NaiveTime".to_string(),

            "bytea" => "Vec<u8>".to_string(),

            // Conservative default (compiles; user can cast/override for runtime correctness).
            _ => "String".to_string(),
        }
    }
}

pub fn normalize_pg_type(pg_type: &str) -> String {
    // Lowercase, remove `(…)` typmods, compress spaces.
    let mut s = pg_type.trim().to_lowercase();

    // Remove typmods: `varchar(255)`, `timestamp(3) with time zone`, `numeric(10,2)`, ...
    while let Some(start) = s.find('(') {
        let Some(end) = s[start..].find(')') else {
            break;
        };
        s.replace_range(start..start + end + 1, "");
    }

    let s = s
        .split_whitespace()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    // Normalize common synonyms.
    match s.as_str() {
        "character varying" => "varchar".to_string(),
        "timestamp with time zone" => "timestamptz".to_string(),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_pg_type_strips_typmods() {
        assert_eq!(normalize_pg_type("character varying(255)"), "varchar");
        assert_eq!(
            normalize_pg_type("timestamp(3) with time zone"),
            "timestamptz"
        );
    }

    #[test]
    fn map_builtin_types() {
        let m = TypeMapper::new(BTreeMap::new());
        assert_eq!(m.map("integer"), "i32");
        assert_eq!(m.map("uuid"), "uuid::Uuid");
        assert_eq!(m.map("uuid[]"), "Vec<uuid::Uuid>");
        assert_eq!(m.map("jsonb"), "serde_json::Value");
    }

    #[test]
    fn custom_mapping_overrides_builtin() {
        let mut custom = BTreeMap::new();
        custom.insert("uuid".to_string(), "my::Uuid".to_string());
        let m = TypeMapper::new(custom);
        assert_eq!(m.map("uuid"), "my::Uuid");
        assert_eq!(m.map("uuid[]"), "Vec<my::Uuid>");
    }
}
