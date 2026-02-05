//! PostgreSQL special types.
//!
//! This module provides Rust types for PostgreSQL-specific types that go beyond
//! standard SQL, including Range types.

mod range;

pub use range::{Bound, Range};
