use pgorm::{GenericClient, OrmError, OrmResult, RowExt};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Table,
    PartitionedTable,
    View,
    MaterializedView,
    ForeignTable,
    Other,
}

impl RelationKind {
    fn from_relkind(relkind: i8) -> Self {
        // Postgres stores `relkind` as a "char" internally. tokio-postgres exposes it as i8.
        match relkind as u8 as char {
            'r' => Self::Table,
            'p' => Self::PartitionedTable,
            'v' => Self::View,
            'm' => Self::MaterializedView,
            'f' => Self::ForeignTable,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub not_null: bool,
    pub default_expr: Option<String>,
    pub ordinal: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub kind: RelationKind,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbSchema {
    pub schemas: Vec<String>,
    pub tables: Vec<TableInfo>,
}

impl DbSchema {
    pub fn find_table(&self, schema: &str, table: &str) -> Option<&TableInfo> {
        self.tables
            .iter()
            .find(|t| t.schema == schema && t.name == table)
    }
}

pub async fn schema_fingerprint<C: GenericClient>(
    client: &C,
    schemas: &[String],
) -> OrmResult<String> {
    let row = client
        .query_one(
            r#"
SELECT
  md5(
    COALESCE(
      string_agg(
        concat_ws(
          '|',
          n.nspname,
          c.relname,
          c.relkind::text,
          a.attnum::text,
          a.attname,
          pg_catalog.format_type(a.atttypid, a.atttypmod),
          a.attnotnull::text,
          COALESCE(a.attidentity::text, ''),
          COALESCE(a.attgenerated::text, ''),
          COALESCE(pg_get_expr(ad.adbin, ad.adrelid), '')
        ),
        E'\n' ORDER BY n.nspname, c.relname, a.attnum
      ),
      ''
    )
  ) AS fingerprint
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid
LEFT JOIN pg_catalog.pg_attrdef ad ON ad.adrelid = c.oid AND ad.adnum = a.attnum
WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f')
  AND a.attnum > 0
  AND NOT a.attisdropped
  AND n.nspname = ANY($1::text[])
"#,
            &[&schemas],
        )
        .await?;

    row.try_get_column::<String>("fingerprint")
}

pub async fn load_schema_from_db<C: GenericClient>(
    client: &C,
    schemas: &[String],
) -> OrmResult<(DbSchema, String)> {
    let fingerprint = schema_fingerprint(client, schemas).await?;

    let rows = client
        .query(
            r#"
SELECT
  n.nspname AS schema_name,
  c.relname AS table_name,
  c.relkind AS relkind,
  a.attname AS column_name,
  a.attnum AS ordinal,
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

    use std::collections::BTreeMap;
    let mut tables: BTreeMap<(String, String), TableInfo> = BTreeMap::new();

    for row in rows {
        let schema_name: String = row.try_get_column("schema_name")?;
        let table_name: String = row.try_get_column("table_name")?;
        let relkind: i8 = row.try_get_column("relkind")?;

        let column_name: String = row.try_get_column("column_name")?;
        let ordinal: i32 = row.try_get_column("ordinal")?;
        let data_type: String = row.try_get_column("data_type")?;
        let not_null: bool = row.try_get_column("not_null")?;
        let default_expr: Option<String> = row
            .try_get::<_, Option<String>>("default_expr")
            .map_err(|e| OrmError::decode("default_expr", e.to_string()))?;

        let key = (schema_name.clone(), table_name.clone());

        let table = tables.entry(key).or_insert_with(|| TableInfo {
            schema: schema_name,
            name: table_name,
            kind: RelationKind::from_relkind(relkind),
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

    if tables.is_empty() {
        return Err(OrmError::Validation(
            "No tables found in the selected schemas".to_string(),
        ));
    }

    Ok((
        DbSchema {
            schemas: schemas.to_vec(),
            tables,
        },
        fingerprint,
    ))
}
