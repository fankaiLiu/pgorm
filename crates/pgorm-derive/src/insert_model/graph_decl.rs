//! Graph declaration data structures for multi-table writes.

// ─────────────────────────────────────────────────────────────────────────────
// Graph Declarations (for multi-table writes)
// ─────────────────────────────────────────────────────────────────────────────

/// has_one / has_many declaration.
#[derive(Clone)]
pub(super) struct HasRelation {
    /// The child InsertModel type.
    pub(super) child_type: syn::Path,
    /// The Rust field name on this struct.
    pub(super) field: String,
    /// The child's foreign key field name.
    pub(super) fk_field: String,
    /// Is this has_one (single) or has_many (vec)?
    pub(super) is_many: bool,
    /// Mode: "insert" or "upsert" (default: insert).
    pub(super) mode: HasRelationMode,
}

/// Mode for has_one/has_many operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum HasRelationMode {
    #[default]
    Insert,
    Upsert,
}

/// belongs_to declaration (pre-insert dependency).
#[derive(Clone)]
pub(super) struct BelongsTo {
    /// The parent InsertModel type.
    pub(super) parent_type: syn::Path,
    /// The Rust field name on this struct.
    pub(super) field: String,
    /// The field to set with parent's id.
    pub(super) set_fk_field: String,
    /// Mode: "insert_returning" or "upsert_returning".
    pub(super) mode: BelongsToMode,
    /// Whether this relation is required.
    pub(super) required: bool,
}

/// before_insert / after_insert step.
#[derive(Clone)]
pub(super) struct InsertStep {
    /// The InsertModel type to insert.
    pub(super) step_type: syn::Path,
    /// The Rust field name on this struct.
    pub(super) field: String,
    /// Mode: "insert" or "upsert".
    pub(super) mode: StepMode,
    /// Is this before or after the root insert?
    pub(super) is_before: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum BelongsToMode {
    #[default]
    InsertReturning,
    UpsertReturning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum StepMode {
    #[default]
    Insert,
    Upsert,
}

/// All graph declarations for an InsertModel.
#[derive(Clone, Default)]
pub(super) struct GraphDeclarations {
    /// The root ID field name (from input, for UUID/snowflake scenarios).
    /// When set, root_id is taken from self.<field> instead of from returning.
    /// Per doc §5: If both returning and graph_root_id_field are set, graph_root_id_field wins.
    pub(super) graph_root_id_field: Option<String>,
    /// has_one / has_many relations.
    pub(super) has_relations: Vec<HasRelation>,
    /// belongs_to relations (pre-insert).
    pub(super) belongs_to: Vec<BelongsTo>,
    /// before_insert / after_insert steps.
    pub(super) insert_steps: Vec<InsertStep>,
}

impl GraphDeclarations {
    pub(super) fn has_any(&self) -> bool {
        !self.has_relations.is_empty()
            || !self.belongs_to.is_empty()
            || !self.insert_steps.is_empty()
    }

    /// Get all field names that are used by graph declarations (should not be inserted into main table).
    pub(super) fn graph_field_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for rel in &self.has_relations {
            names.push(rel.field.clone());
        }
        for bt in &self.belongs_to {
            names.push(bt.field.clone());
        }
        for step in &self.insert_steps {
            names.push(step.field.clone());
        }
        names
    }
}
