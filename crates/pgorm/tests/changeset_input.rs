#![allow(dead_code)]

use pgorm::{FromRow, InsertModel, Model, UpdateModel};

#[derive(InsertModel)]
#[orm(table = "users")]
#[orm(input)]
struct NewUser {
    #[orm(len = "2..=100")]
    name: String,

    #[orm(email)]
    email: String,

    #[orm(range = "0..=150")]
    age: Option<i32>,

    #[orm(uuid, input_as = "String")]
    external_id: uuid::Uuid,

    #[orm(url)]
    homepage: String,

    #[orm(one_of = "free|pro|enterprise")]
    plan: String,

    #[orm(regex = r"^[a-z0-9_]+$")]
    username: String,
}

#[test]
fn validate_and_convert() {
    let bad = NewUserInput {
        name: Some("A".into()),
        email: Some("not-an-email".into()),
        age: Some(200),
        external_id: Some("not-a-uuid".into()),
        homepage: Some("not-a-url".into()),
        plan: Some("gold".into()),
        username: Some("bad space".into()),
    };

    let errs = bad.validate();
    assert!(!errs.is_empty());
    assert!(
        errs.iter()
            .any(|e| e.field == "name" && e.code.as_str() == "len")
    );
    assert!(
        errs.iter()
            .any(|e| e.field == "email" && e.code.as_str() == "email")
    );
    assert!(
        errs.iter()
            .any(|e| e.field == "age" && e.code.as_str() == "range")
    );
    assert!(
        errs.iter()
            .any(|e| e.field == "external_id" && e.code.as_str() == "uuid")
    );
    assert!(
        errs.iter()
            .any(|e| e.field == "homepage" && e.code.as_str() == "url")
    );
    assert!(
        errs.iter()
            .any(|e| e.field == "plan" && e.code.as_str() == "one_of")
    );
    assert!(
        errs.iter()
            .any(|e| e.field == "username" && e.code.as_str() == "regex")
    );

    let ok = NewUserInput {
        name: Some("Alice".into()),
        email: Some("alice@example.com".into()),
        age: Some(42),
        external_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
        homepage: Some("https://example.com".into()),
        plan: Some("pro".into()),
        username: Some("alice_42".into()),
    };

    let user = ok.try_into_model().unwrap();
    assert_eq!(user.name, "Alice");
    assert_eq!(user.email, "alice@example.com");
    assert_eq!(user.age, Some(42));
    assert_eq!(
        user.external_id.to_string(),
        "550e8400-e29b-41d4-a716-446655440000"
    );
    assert_eq!(user.homepage, "https://example.com");
    assert_eq!(user.plan, "pro");
    assert_eq!(user.username, "alice_42");
}

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    username: String,
    email: String,
}

#[derive(UpdateModel)]
#[orm(table = "users", model = "User", returning = "User")]
#[orm(input)]
struct UserPatch {
    #[orm(len = "2..=20")]
    username: Option<String>,

    #[orm(email)]
    email: Option<String>,
}

#[test]
fn validate_update_patch() {
    let bad = UserPatchInput {
        username: Some("A".into()),
        email: Some("bad".into()),
    };

    let errs = bad.validate();
    assert!(!errs.is_empty());
    assert!(
        errs.iter()
            .any(|e| e.field == "username" && e.code.as_str() == "len")
    );
    assert!(
        errs.iter()
            .any(|e| e.field == "email" && e.code.as_str() == "email")
    );

    let ok = UserPatchInput {
        username: Some("alice".into()),
        email: Some("alice@example.com".into()),
    };

    let patch = ok.try_into_patch().unwrap();
    assert_eq!(patch.username.as_deref(), Some("alice"));
    assert_eq!(patch.email.as_deref(), Some("alice@example.com"));
}
