//! Example demonstrating `input_as` on `Option<Option<T>>` fields.
//!
//! Run with:
//!   cargo run --example input_as_nested_option -p pgorm

#![allow(dead_code)]

use pgorm::{InsertModel, UpdateModel};

#[derive(Debug, InsertModel)]
#[orm(table = "devices")]
#[orm(input)]
struct NewDevice {
    label: String,
    #[orm(uuid, input_as = "String")]
    external_id: Option<Option<uuid::Uuid>>,
}

#[derive(Debug, UpdateModel)]
#[orm(table = "devices", id_column = "id")]
#[orm(input)]
struct DevicePatch {
    #[orm(uuid, input_as = "String")]
    external_id: Option<Option<uuid::Uuid>>,
}

fn main() {
    let create_input: NewDeviceInput = serde_json::from_str(
        r#"{
          "label": "edge-router",
          "external_id": "550e8400-e29b-41d4-a716-446655440000"
        }"#,
    )
    .unwrap();
    let create_model = create_input.try_into_model().unwrap();
    println!("create parsed: {:?}", create_model.external_id);

    // Note: for tri-state in Rust value form, use `Some(None)` explicitly.
    let clear_input = DevicePatchInput {
        external_id: Some(None),
    };
    let clear_patch = clear_input.try_into_patch().unwrap();
    println!("patch parsed (set NULL): {:?}", clear_patch.external_id);

    let bad_input: NewDeviceInput = serde_json::from_str(
        r#"{
          "label": "broken",
          "external_id": "not-a-uuid"
        }"#,
    )
    .unwrap();
    match bad_input.try_into_model() {
        Ok(_) => println!("unexpected success"),
        Err(errs) => {
            println!("invalid uuid -> validation errors:");
            println!("{}", serde_json::to_string_pretty(&errs).unwrap());
        }
    }
}
