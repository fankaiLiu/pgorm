use super::delete::DeleteBuilder;
use super::insert::InsertBuilder;
use super::select::QueryBuilder;
use super::update::UpdateBuilder;
use tokio_postgres::types::ToSql;

/// Database table metadata helper.
///
/// This is a small ergonomic wrapper to create builders with consistent
/// select/returning lists.
///
/// # Example
///
/// ```rust
/// use pgorm::builder::{SqlBuilder, Table};
///
/// const USERS: Table = Table::new("users")
///     .with_select_cols(&["id", "username", "email", "created_at"]);
///
/// let mut qb = USERS.select();
/// qb.and_eq("id", 1);
/// let sql = qb.build_sql();
/// assert!(sql.contains("FROM users WHERE id = $1"));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Table {
    pub name: &'static str,
    pub select_cols: &'static [&'static str],
    pub conflict_cols: &'static [&'static str],
    pub returning_cols: &'static [&'static str],
    pub id_col: &'static str,
}

impl Table {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            select_cols: &["*"],
            conflict_cols: &[],
            returning_cols: &[],
            id_col: "id",
        }
    }

    pub const fn with_select_cols(mut self, cols: &'static [&'static str]) -> Self {
        self.select_cols = cols;
        self
    }

    pub const fn with_conflict_cols(mut self, cols: &'static [&'static str]) -> Self {
        self.conflict_cols = cols;
        self
    }

    pub const fn with_returning_cols(mut self, cols: &'static [&'static str]) -> Self {
        self.returning_cols = cols;
        self
    }

    pub const fn with_id_col(mut self, col: &'static str) -> Self {
        self.id_col = col;
        self
    }

    pub fn select(&self) -> QueryBuilder {
        let mut qb = QueryBuilder::new(self.name);
        if !self.select_cols.is_empty() && (self.select_cols.len() > 1 || self.select_cols[0] != "*") {
            qb.select_cols(self.select_cols);
        }
        qb
    }

    pub fn insert(&self) -> InsertBuilder {
        let mut builder = InsertBuilder::new(self.name);
        if !self.returning_cols.is_empty() {
            builder.returning_cols(self.returning_cols);
        }
        builder
    }

    pub fn update(&self) -> UpdateBuilder {
        let mut builder = UpdateBuilder::new(self.name);
        if !self.returning_cols.is_empty() {
            builder.returning_cols(self.returning_cols);
        }
        builder
    }

    pub fn delete(&self) -> DeleteBuilder {
        let mut builder = DeleteBuilder::new(self.name);
        if !self.returning_cols.is_empty() {
            builder.returning_cols(self.returning_cols);
        }
        builder
    }

    pub fn count(&self) -> QueryBuilder {
        QueryBuilder::new(self.name)
    }

    pub fn update_by_id<T>(&self, id: T) -> UpdateBuilder
    where
        T: ToSql + Sync + Send + 'static,
    {
        let mut builder = self.update();
        builder.and_eq(self.id_col, id);
        builder
    }

    pub fn delete_by_id<T>(&self, id: T) -> DeleteBuilder
    where
        T: ToSql + Sync + Send + 'static,
    {
        let mut builder = self.delete();
        builder.and_eq(self.id_col, id);
        builder
    }

    pub fn insert_with_json<T>(&self, column: &str, data: &T) -> serde_json::Result<InsertBuilder>
    where
        T: serde::Serialize + Sync + Send,
    {
        let mut builder = self.insert();
        builder.set_json(column, data)?;
        Ok(builder)
    }
}
