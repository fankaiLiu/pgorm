//! Common utilities for examples
//!
//! This module contains shared code used across multiple examples:
//! - Output formatting (styled terminal output)
//! - Database schema setup helpers

mod output;
mod schema;

#[allow(unused_imports)]
pub use output::*;
#[allow(unused_imports)]
pub use schema::*;
