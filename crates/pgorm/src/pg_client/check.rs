use crate::GenericClient;
use crate::check::{DbSchema, TableMeta};
use crate::error::OrmResult;

/// Result of checking a model against the database schema.
#[derive(Debug, Clone)]
pub struct ModelCheckResult {
    /// Model name
    pub model: &'static str,
    /// Table name the model maps to
    pub table: &'static str,
    /// Columns defined in the model
    pub model_columns: Vec<&'static str>,
    /// Columns found in the database (None if table not found)
    pub db_columns: Option<Vec<String>>,
    /// Missing columns (in model but not in DB)
    pub missing_in_db: Vec<&'static str>,
    /// Extra columns (in DB but not in model) - informational only
    pub extra_in_db: Vec<String>,
    /// Whether the table was found
    pub table_found: bool,
}

impl ModelCheckResult {
    /// Returns true if the model matches the database schema.
    pub fn is_valid(&self) -> bool {
        self.table_found && self.missing_in_db.is_empty()
    }

    /// Print a summary of the check result.
    pub fn print(&self) {
        if self.is_valid() {
            println!("  ✓ {} (table: {})", self.model, self.table);
        } else if !self.table_found {
            println!(
                "  ✗ {} - table '{}' not found in database",
                self.model, self.table
            );
        } else {
            println!(
                "  ✗ {} - missing columns: {:?}",
                self.model, self.missing_in_db
            );
        }
    }

    /// Check a model against a database schema.
    pub fn check<T: TableMeta>(db_schema: &DbSchema) -> Self {
        let table_name = T::table_name();
        let schema_name = T::schema_name();
        let model_columns: Vec<&'static str> = T::columns().to_vec();

        let db_table = db_schema.find_table(schema_name, table_name);

        match db_table {
            Some(table) => {
                let db_columns: Vec<String> =
                    table.columns.iter().map(|c| c.name.clone()).collect();

                let missing_in_db: Vec<&'static str> = model_columns
                    .iter()
                    .filter(|col| !db_columns.iter().any(|dc| dc == *col))
                    .copied()
                    .collect();

                let extra_in_db: Vec<String> = db_columns
                    .iter()
                    .filter(|col| !model_columns.contains(&col.as_str()))
                    .cloned()
                    .collect();

                ModelCheckResult {
                    model: std::any::type_name::<T>()
                        .rsplit("::")
                        .next()
                        .unwrap_or("Unknown"),
                    table: table_name,
                    model_columns,
                    db_columns: Some(db_columns),
                    missing_in_db,
                    extra_in_db,
                    table_found: true,
                }
            }
            None => ModelCheckResult {
                model: std::any::type_name::<T>()
                    .rsplit("::")
                    .next()
                    .unwrap_or("Unknown"),
                table: table_name,
                model_columns,
                db_columns: None,
                missing_in_db: vec![],
                extra_in_db: vec![],
                table_found: false,
            },
        }
    }
}

impl<C: GenericClient> super::PgClient<C> {
    /// Load the database schema from PostgreSQL.
    ///
    /// This queries the database catalog to get actual table and column information.
    /// By default, only the "public" schema is loaded.
    pub async fn load_db_schema(&self) -> OrmResult<DbSchema> {
        self.load_db_schema_for(&["public".to_string()]).await
    }

    /// Load the database schema for specific schemas.
    pub async fn load_db_schema_for(&self, schemas: &[String]) -> OrmResult<DbSchema> {
        // Query to get all tables and columns
        let rows = self
            .client
            .query(
                r#"
SELECT
  n.nspname AS schema_name,
  c.relname AS table_name,
  c.relkind AS relkind,
  a.attname AS column_name,
  a.attnum::integer AS ordinal,
  pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
  a.attnotnull AS not_null,
  pg_get_expr(ad.adbin, ad.adrelid) AS default_expr
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid
LEFT JOIN pg_catalog.pg_attrdef ad ON ad.adrelid = c.oid AND ad.adnum = a.attnum
WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f')
  AND a.attnum > 0
  AND NOT a.attisdropped
  AND n.nspname = ANY($1::text[])
ORDER BY n.nspname, c.relname, a.attnum
"#,
                &[&schemas],
            )
            .await?;

        use crate::check::{ColumnInfo, RelationKind, TableInfo};
        use std::collections::BTreeMap;

        let mut tables: BTreeMap<(String, String), TableInfo> = BTreeMap::new();

        for row in rows {
            let schema_name: String = row.get("schema_name");
            let table_name: String = row.get("table_name");
            let relkind: i8 = row.get("relkind");

            let column_name: String = row.get("column_name");
            let ordinal: i32 = row.get("ordinal");
            let data_type: String = row.get("data_type");
            let not_null: bool = row.get("not_null");
            let default_expr: Option<String> = row.get("default_expr");

            let kind = match relkind as u8 as char {
                'r' => RelationKind::Table,
                'p' => RelationKind::PartitionedTable,
                'v' => RelationKind::View,
                'm' => RelationKind::MaterializedView,
                'f' => RelationKind::ForeignTable,
                _ => RelationKind::Other,
            };

            let key = (schema_name.clone(), table_name.clone());

            let table = tables.entry(key).or_insert_with(|| TableInfo {
                schema: schema_name,
                name: table_name,
                kind,
                columns: Vec::new(),
            });

            table.columns.push(ColumnInfo {
                name: column_name,
                data_type,
                not_null,
                default_expr,
                ordinal,
            });
        }

        let tables = tables.into_values().collect::<Vec<_>>();

        Ok(DbSchema {
            schemas: schemas.to_vec(),
            tables,
        })
    }

    /// Check a single model against the database schema.
    ///
    /// Compares the model's columns with the actual database table.
    pub async fn check_model<T: TableMeta>(&self) -> OrmResult<ModelCheckResult> {
        let db_schema = self.load_db_schema().await?;
        Ok(ModelCheckResult::check::<T>(&db_schema))
    }
}
