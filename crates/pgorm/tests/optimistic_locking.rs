//! Compile-time tests for optimistic locking feature.
//!
//! These tests verify that the `#[orm(version)]` attribute works correctly
//! and generates the expected methods.

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use pgorm::{FromRow, Model, UpdateModel};

// Test: Basic version field works
#[derive(Debug, Model, FromRow)]
#[orm(table = "posts")]
struct Post {
    #[orm(id)]
    id: i64,
    title: String,
    content: String,
    version: i32,
    created_at: DateTime<Utc>,
}

#[derive(UpdateModel)]
#[orm(table = "posts", model = "Post", returning = "Post")]
struct PostPatch {
    title: Option<String>,
    content: Option<String>,
    #[orm(version)]
    version: i32,
}

#[test]
fn test_version_field_compiles() {
    // This test passes if it compiles - the version field is correctly parsed
    let _patch = PostPatch {
        title: Some("New Title".into()),
        content: None,
        version: 1,
    };
}

#[test]
fn test_update_by_id_method_exists() {
    // Verify that update_by_id method is generated
    fn assert_update_by_id<T, I>(_patch: T)
    where
        T: UpdateByIdTrait<I>,
    {
    }

    trait UpdateByIdTrait<I> {
        fn update_by_id(
            self,
            conn: &impl pgorm::GenericClient,
            id: I,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<u64>>;
    }

    impl UpdateByIdTrait<i64> for PostPatch {
        fn update_by_id(
            self,
            conn: &impl pgorm::GenericClient,
            id: i64,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<u64>> {
            PostPatch::update_by_id(self, conn, id)
        }
    }

    let patch = PostPatch {
        title: Some("Test".into()),
        content: None,
        version: 1,
    };
    assert_update_by_id::<PostPatch, i64>(patch);
}

#[test]
fn test_update_by_ids_method_exists() {
    fn assert_update_by_ids<T, I>(_patch: T)
    where
        T: UpdateByIdsTrait<I>,
    {
    }

    trait UpdateByIdsTrait<I> {
        fn update_by_ids(
            self,
            conn: &impl pgorm::GenericClient,
            ids: Vec<I>,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<u64>>;
    }

    impl UpdateByIdsTrait<i64> for PostPatch {
        fn update_by_ids(
            self,
            conn: &impl pgorm::GenericClient,
            ids: Vec<i64>,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<u64>> {
            PostPatch::update_by_ids(self, conn, ids)
        }
    }

    let patch = PostPatch {
        title: Some("Test".into()),
        content: None,
        version: 1,
    };
    assert_update_by_ids::<PostPatch, i64>(patch);
}

#[test]
fn test_update_by_id_force_method_exists() {
    // Verify that update_by_id_force method is generated when version field exists
    fn assert_force_method<T, I>(_patch: T)
    where
        T: UpdateByIdForceTrait<I>,
    {
    }

    trait UpdateByIdForceTrait<I> {
        fn update_by_id_force(
            self,
            conn: &impl pgorm::GenericClient,
            id: I,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<u64>>;
    }

    impl UpdateByIdForceTrait<i64> for PostPatch {
        fn update_by_id_force(
            self,
            conn: &impl pgorm::GenericClient,
            id: i64,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<u64>> {
            PostPatch::update_by_id_force(self, conn, id)
        }
    }

    let patch = PostPatch {
        title: Some("Test".into()),
        content: None,
        version: 1,
    };
    assert_force_method::<PostPatch, i64>(patch);
}

#[test]
fn test_update_by_id_returning_method_exists() {
    // Verify that update_by_id_returning method is generated
    fn assert_returning_method<T, I, R>(_patch: T)
    where
        T: UpdateByIdReturningTrait<I, R>,
    {
    }

    trait UpdateByIdReturningTrait<I, R> {
        fn update_by_id_returning(
            self,
            conn: &impl pgorm::GenericClient,
            id: I,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<R>>;
    }

    impl UpdateByIdReturningTrait<i64, Post> for PostPatch {
        fn update_by_id_returning(
            self,
            conn: &impl pgorm::GenericClient,
            id: i64,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<Post>> {
            PostPatch::update_by_id_returning(self, conn, id)
        }
    }

    let patch = PostPatch {
        title: Some("Test".into()),
        content: None,
        version: 1,
    };
    assert_returning_method::<PostPatch, i64, Post>(patch);
}

#[test]
fn test_update_by_ids_returning_method_exists() {
    fn assert_returning_method<T, I, R>(_patch: T)
    where
        T: UpdateByIdsReturningTrait<I, R>,
    {
    }

    trait UpdateByIdsReturningTrait<I, R> {
        fn update_by_ids_returning(
            self,
            conn: &impl pgorm::GenericClient,
            ids: Vec<I>,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<Vec<R>>>;
    }

    impl UpdateByIdsReturningTrait<i64, Post> for PostPatch {
        fn update_by_ids_returning(
            self,
            conn: &impl pgorm::GenericClient,
            ids: Vec<i64>,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<Vec<Post>>> {
            PostPatch::update_by_ids_returning(self, conn, ids)
        }
    }

    let patch = PostPatch {
        title: Some("Test".into()),
        content: None,
        version: 1,
    };
    assert_returning_method::<PostPatch, i64, Post>(patch);
}

#[test]
fn test_update_by_id_force_returning_method_exists() {
    // Verify that update_by_id_force_returning method is generated when version field exists
    fn assert_force_returning_method<T, I, R>(_patch: T)
    where
        T: UpdateByIdForceReturningTrait<I, R>,
    {
    }

    trait UpdateByIdForceReturningTrait<I, R> {
        fn update_by_id_force_returning(
            self,
            conn: &impl pgorm::GenericClient,
            id: I,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<R>>;
    }

    impl UpdateByIdForceReturningTrait<i64, Post> for PostPatch {
        fn update_by_id_force_returning(
            self,
            conn: &impl pgorm::GenericClient,
            id: i64,
        ) -> impl std::future::Future<Output = pgorm::OrmResult<Post>> {
            PostPatch::update_by_id_force_returning(self, conn, id)
        }
    }

    let patch = PostPatch {
        title: Some("Test".into()),
        content: None,
        version: 1,
    };
    assert_force_returning_method::<PostPatch, i64, Post>(patch);
}

// Test: Version with different integer types
#[derive(UpdateModel)]
#[orm(table = "items", id_column = "id")]
struct ItemPatchI16 {
    name: Option<String>,
    #[orm(version)]
    version: i16,
}

#[derive(UpdateModel)]
#[orm(table = "items", id_column = "id")]
struct ItemPatchI32 {
    name: Option<String>,
    #[orm(version)]
    version: i32,
}

#[derive(UpdateModel)]
#[orm(table = "items", id_column = "id")]
struct ItemPatchI64 {
    name: Option<String>,
    #[orm(version)]
    version: i64,
}

#[test]
fn test_version_field_with_different_int_types() {
    let _i16 = ItemPatchI16 {
        name: Some("Test".into()),
        version: 1,
    };
    let _i32 = ItemPatchI32 {
        name: Some("Test".into()),
        version: 1,
    };
    let _i64 = ItemPatchI64 {
        name: Some("Test".into()),
        version: 1,
    };
}

// Test: Version with custom column name
#[derive(UpdateModel)]
#[orm(table = "versioned_items", id_column = "id")]
struct VersionedItemPatch {
    name: Option<String>,
    #[orm(version, column = "row_version")]
    version: i32,
}

#[test]
fn test_version_with_custom_column_name() {
    let _patch = VersionedItemPatch {
        name: Some("Test".into()),
        version: 1,
    };
}

// Test: UpdateModel without version field (should NOT have force methods)
#[derive(UpdateModel)]
#[orm(table = "simple_items", id_column = "id")]
struct SimpleItemPatch {
    name: Option<String>,
}

#[test]
fn test_update_model_without_version() {
    // This just tests that UpdateModel without version field still compiles
    let _patch = SimpleItemPatch {
        name: Some("Test".into()),
    };
}

// Test: StaleRecord error variant exists
#[test]
fn test_stale_record_error_exists() {
    let err = pgorm::OrmError::stale_record("posts", 1_i64, 5);
    assert!(err.is_stale_record());

    // Check error message format
    let msg = err.to_string();
    assert!(msg.contains("posts"));
    assert!(msg.contains("5")); // expected_version
}

#[test]
fn test_stale_record_error_fields() {
    let err = pgorm::OrmError::StaleRecord {
        table: "users",
        id: "123".to_string(),
        expected_version: 42,
    };

    match err {
        pgorm::OrmError::StaleRecord {
            table,
            id,
            expected_version,
        } => {
            assert_eq!(table, "users");
            assert_eq!(id, "123");
            assert_eq!(expected_version, 42);
        }
        _ => panic!("Expected StaleRecord error"),
    }
}
