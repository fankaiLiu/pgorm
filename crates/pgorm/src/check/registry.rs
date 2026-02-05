use std::collections::HashMap;

/// Metadata for a database table.
///
/// This trait is automatically implemented by the `#[derive(Model)]` macro.
/// It provides table name and column information for schema validation.
pub trait TableMeta {
    /// The database table name.
    fn table_name() -> &'static str;

    /// The database schema name (defaults to "public").
    fn schema_name() -> &'static str {
        "public"
    }

    /// List of column names in this table.
    fn columns() -> &'static [&'static str];

    /// The primary key column name, if any.
    fn primary_key() -> Option<&'static str> {
        None
    }
}

/// Column information for schema checking.
#[derive(Debug, Clone)]
pub struct ColumnMeta {
    /// Column name.
    pub name: String,
    /// Whether this column is the primary key.
    pub is_primary_key: bool,
}

/// Table information for schema checking.
#[derive(Debug, Clone)]
pub struct TableSchema {
    /// Schema name (e.g., "public").
    pub schema: String,
    /// Table name.
    pub name: String,
    /// Column metadata.
    pub columns: Vec<ColumnMeta>,
}

impl TableSchema {
    /// Create a new table schema.
    pub fn new(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            name: name.into(),
            columns: Vec::new(),
        }
    }

    /// Add a column to this table schema.
    pub fn add_column(&mut self, name: impl Into<String>, is_primary_key: bool) {
        self.columns.push(ColumnMeta {
            name: name.into(),
            is_primary_key,
        });
    }

    /// Add multiple columns to this table schema.
    pub fn with_columns(mut self, columns: &[&str]) -> Self {
        for col in columns {
            self.columns.push(ColumnMeta {
                name: col.to_string(),
                is_primary_key: false,
            });
        }
        self
    }

    /// Set the primary key column.
    pub fn with_primary_key(mut self, pk: &str) -> Self {
        for col in &mut self.columns {
            col.is_primary_key = col.name == pk;
        }
        // If the primary key column doesn't exist, add it
        if !self.columns.iter().any(|c| c.name == pk) {
            self.columns.push(ColumnMeta {
                name: pk.to_string(),
                is_primary_key: true,
            });
        }
        self
    }

    /// Check if this table has a column with the given name.
    pub fn has_column(&self, name: &str) -> bool {
        self.columns.iter().any(|c| c.name == name)
    }
}

/// Registry for table schemas.
///
/// Use this to register all your model tables and then check SQL against them.
#[derive(Debug, Clone)]
pub struct SchemaRegistry {
    /// Map of schema -> (table -> TableSchema)
    ///
    /// This layout avoids per-lookup allocations in hot paths (`get_table`/`has_table`).
    pub(super) tables: HashMap<String, HashMap<String, TableSchema>>,
    #[cfg(feature = "check")]
    pub(super) parse_cache: std::sync::Arc<pgorm_check::SqlParseCache>,
}

impl SchemaRegistry {
    /// Create a new empty schema registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new schema registry with a custom SQL parse cache capacity.
    #[cfg(feature = "check")]
    pub fn with_parse_cache_capacity(capacity: usize) -> Self {
        Self {
            tables: HashMap::new(),
            parse_cache: std::sync::Arc::new(pgorm_check::SqlParseCache::new(capacity)),
        }
    }

    /// Register a table from a type that implements `TableMeta`.
    pub fn register<T: TableMeta>(&mut self) {
        let schema_name = T::schema_name().to_string();
        let table_name = T::table_name().to_string();
        let columns = T::columns();
        let pk = T::primary_key();

        let mut table = TableSchema::new(&schema_name, &table_name);
        for col in columns {
            let is_pk = pk == Some(*col);
            table.add_column(*col, is_pk);
        }

        self.tables
            .entry(schema_name)
            .or_default()
            .insert(table_name, table);
    }

    /// Register a table schema directly.
    pub fn register_table(&mut self, table: TableSchema) {
        let schema = table.schema.clone();
        let name = table.name.clone();
        self.tables.entry(schema).or_default().insert(name, table);
    }

    /// Get a table by schema and name.
    pub fn get_table(&self, schema: &str, name: &str) -> Option<&TableSchema> {
        self.tables
            .get(schema)
            .and_then(|by_name| by_name.get(name))
    }

    /// Find a table by name, searching all schemas.
    pub fn find_table(&self, name: &str) -> Option<&TableSchema> {
        // First try public schema
        if let Some(t) = self.get_table("public", name) {
            return Some(t);
        }
        // Then try any schema
        self.tables.values().find_map(|by_name| by_name.get(name))
    }

    /// Check if a table exists.
    pub fn has_table(&self, schema: &str, name: &str) -> bool {
        self.tables
            .get(schema)
            .is_some_and(|by_name| by_name.contains_key(name))
    }

    /// Get all registered tables.
    pub fn tables(&self) -> impl Iterator<Item = &TableSchema> {
        self.tables.values().flat_map(|by_name| by_name.values())
    }

    /// Get the number of registered tables.
    pub fn len(&self) -> usize {
        self.tables.values().map(|by_name| by_name.len()).sum()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tables.values().all(|by_name| by_name.is_empty())
    }
}

impl Default for SchemaRegistry {
    fn default() -> Self {
        Self {
            tables: HashMap::new(),
            #[cfg(feature = "check")]
            parse_cache: std::sync::Arc::new(pgorm_check::SqlParseCache::default()),
        }
    }
}

/// Level of a schema issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaIssueLevel {
    /// Informational message.
    Info,
    /// Warning - may be intentional.
    Warning,
    /// Error - likely a bug.
    Error,
}

/// Kind of schema issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaIssueKind {
    /// SQL parse error.
    ParseError,
    /// Referenced table not found.
    MissingTable,
    /// Referenced column not found.
    MissingColumn,
    /// Column reference is ambiguous across visible tables.
    AmbiguousColumn,
    /// SQL feature not supported by the checker.
    Unsupported,
}

/// A schema validation issue.
#[derive(Debug, Clone)]
pub struct SchemaIssue {
    /// Severity level.
    pub level: SchemaIssueLevel,
    /// Type of issue.
    pub kind: SchemaIssueKind,
    /// Human-readable message.
    pub message: String,
}

impl std::fmt::Display for SchemaIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} {:?}: {}", self.level, self.kind, self.message)
    }
}
