//! Parameter storage using Arc for clone-friendly query builders.

use std::sync::Arc;
use tokio_postgres::types::ToSql;

/// A clone-friendly parameter wrapper using Arc.
///
/// This allows query builders to be cloned without copying parameter values,
/// which is essential for derive macros that need to clone queries.
#[derive(Clone)]
pub struct Param(pub(crate) Arc<dyn ToSql + Send + Sync>);

impl Param {
    /// Create a new parameter from any ToSql value.
    pub fn new<T: ToSql + Send + Sync + 'static>(value: T) -> Self {
        Param(Arc::new(value))
    }

    /// Get a reference to the inner value as a ToSql trait object.
    pub fn as_ref(&self) -> &(dyn ToSql + Sync) {
        // Arc<dyn ToSql + Send + Sync> -> &(dyn ToSql + Sync)
        // This is safe because we're just removing Send from the trait bounds
        &*self.0 as &(dyn ToSql + Sync)
    }
}

impl std::fmt::Debug for Param {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Param").field(&"<dyn ToSql>").finish()
    }
}

/// A collection of parameters that can be built into references.
#[derive(Clone, Debug, Default)]
pub struct ParamList {
    params: Vec<Param>,
}

impl ParamList {
    /// Create a new empty parameter list.
    pub fn new() -> Self {
        Self { params: Vec::new() }
    }

    /// Add a parameter and return its 1-based index.
    pub fn push<T: ToSql + Send + Sync + 'static>(&mut self, value: T) -> usize {
        self.params.push(Param::new(value));
        self.params.len()
    }

    /// Add a pre-wrapped Param and return its 1-based index.
    pub fn push_param(&mut self, param: Param) -> usize {
        self.params.push(param);
        self.params.len()
    }

    /// Get the current parameter count.
    pub fn len(&self) -> usize {
        self.params.len()
    }

    /// Check if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    /// Get all parameters as references for tokio-postgres.
    pub fn as_refs(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params.iter().map(|p| p.as_ref()).collect()
    }

    /// Extend this list with another list's parameters.
    pub fn extend(&mut self, other: &ParamList) {
        self.params.extend(other.params.iter().cloned());
    }

    /// Extend this list with parameters from an iterator.
    pub fn extend_params(&mut self, params: impl IntoIterator<Item = Param>) {
        self.params.extend(params);
    }

    /// Clear all parameters.
    pub fn clear(&mut self) {
        self.params.clear();
    }
}
