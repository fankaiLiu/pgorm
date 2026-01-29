//! Graph declaration data structures for multi-table update writes.

// ─────────────────────────────────────────────────────────────────────────────
// Graph Declarations for UpdateModel (child table strategies)
// ─────────────────────────────────────────────────────────────────────────────

/// Strategy for updating has_many children.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum UpdateStrategy {
    /// Delete all old children, insert new ones.
    #[default]
    Replace,
    /// Only insert new children (don't delete old ones).
    Append,
    /// Upsert children (ON CONFLICT DO UPDATE).
    Upsert,
    /// Upsert + delete children not in the new list (sync to exact list).
    Diff,
}

/// has_many_update declaration.
#[derive(Clone)]
pub(super) struct HasManyUpdate {
    /// The child InsertModel type.
    pub(super) child_type: syn::Path,
    /// The Rust field name on this struct.
    pub(super) field: String,
    /// The SQL column name for the foreign key.
    pub(super) fk_column: String,
    /// The child's foreign key field name.
    pub(super) fk_field: String,
    /// Update strategy.
    pub(super) strategy: UpdateStrategy,
    /// For diff strategy: the key columns (SQL column names).
    pub(super) key_columns: Option<Vec<String>>,
}

/// has_one_update declaration.
#[derive(Clone)]
pub(super) struct HasOneUpdate {
    /// The child InsertModel type.
    pub(super) child_type: syn::Path,
    /// The Rust field name on this struct.
    pub(super) field: String,
    /// The SQL column name for the foreign key.
    pub(super) fk_column: String,
    /// The child's foreign key field name.
    pub(super) fk_field: String,
    /// Strategy: replace or upsert.
    pub(super) strategy: UpdateStrategy,
}

/// All graph declarations for an UpdateModel.
#[derive(Clone, Default)]
pub(super) struct UpdateGraphDeclarations {
    /// has_many_update relations.
    pub(super) has_many: Vec<HasManyUpdate>,
    /// has_one_update relations.
    pub(super) has_one: Vec<HasOneUpdate>,
}

impl UpdateGraphDeclarations {
    pub(super) fn has_any(&self) -> bool {
        !self.has_many.is_empty() || !self.has_one.is_empty()
    }

    /// Get all field names that are used by graph declarations.
    pub(super) fn graph_field_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for rel in &self.has_many {
            names.push(rel.field.clone());
        }
        for rel in &self.has_one {
            names.push(rel.field.clone());
        }
        names
    }
}
