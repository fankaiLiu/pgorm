# Input Validation

When building write models from untrusted input (JSON API requests, form submissions, message queues), you need:

- A dedicated `Input` struct for deserialization (with proper patch/tri-state semantics)
- Centralized field validation (length, range, email, URL, UUID, etc.)
- A machine-friendly error format you can return directly from your API

The `#[orm(input)]` attribute generates all of this for `InsertModel` and `UpdateModel`.

## Prerequisites

The `derive` and `validate` features must be enabled (both are included in default features):

```toml
[dependencies]
pgorm = "0.2.0"

# If you disabled defaults:
# pgorm = { version = "0.2.0", features = ["derive", "validate"] }
```

## 1. Generate Input Structs with `#[orm(input)]`

### For InsertModel

Adding `#[orm(input)]` to an `InsertModel` generates a `*Input` struct (e.g. `NewUser` generates `NewUserInput`):

```rust
use pgorm::{FromRow, InsertModel, Model};

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
    age: Option<i32>,
    external_id: uuid::Uuid,
    homepage: Option<String>,
}

#[derive(Debug, InsertModel)]
#[orm(table = "users", returning = "User")]
#[orm(input)]  // generates NewUserInput
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
    homepage: Option<String>,
}
```

The generated `NewUserInput` struct:

- Derives `serde::Deserialize` so you can parse JSON directly
- Has a `validate()` method returning `ValidationErrors`
- Has a `try_into_model()` method returning `Result<NewUser, ValidationErrors>`

### For UpdateModel

Adding `#[orm(input)]` to an `UpdateModel` generates a `*Input` struct with tri-state semantics for patch operations:

```rust
use pgorm::UpdateModel;

#[derive(Debug, UpdateModel)]
#[orm(table = "users", model = "User", returning = "User")]
#[orm(input)]  // generates UserPatchInput
struct UserPatch {
    #[orm(len = "2..=100")]
    name: Option<String>,           // None = skip, Some(v) = update

    #[orm(email)]
    email: Option<String>,

    #[orm(url)]
    homepage: Option<Option<String>>, // None = skip, Some(None) = set NULL, Some(Some(v)) = set value
}
```

The generated `UserPatchInput` has a `try_into_patch()` method returning `Result<UserPatch, ValidationErrors>`.

## 2. Validation Attributes

| Attribute | Description |
|-----------|-------------|
| `#[orm(len = "min..=max")]` | String length validation |
| `#[orm(range = "min..=max")]` | Numeric range validation |
| `#[orm(email)]` | Email format validation |
| `#[orm(url)]` | URL format validation |
| `#[orm(uuid)]` | UUID format validation |
| `#[orm(ip)]` | IP address format validation |
| `#[orm(regex = "pattern")]` | Custom regex pattern matching |
| `#[orm(one_of = "a\|b\|c")]` | Value must be one of the listed options |
| `#[orm(custom = "path::to::fn")]` | Custom validator function |
| `#[orm(input_as = "Type")]` | Accept a different type in the Input struct |

Attributes can be combined on a single field:

```rust
#[orm(uuid, input_as = "String")]
external_id: uuid::Uuid,
```

## 3. The `input_as` Boundary

`input_as` tells pgorm to accept a different type in the generated Input struct, validate it, and then convert it to the model's field type. This is useful when the wire format is a string but the model uses a parsed type.

Currently supported conversions:

- `String` to `uuid::Uuid`
- `String` to `std::net::IpAddr`
- `String` to `url::Url` (requires the `url` crate as a dependency)

```rust
// In the model: field is uuid::Uuid
// In the Input: field is String
// Validation: checks UUID format
// Conversion: parses String -> uuid::Uuid, returns ValidationErrors on failure
#[orm(uuid, input_as = "String")]
external_id: uuid::Uuid,
```

Note: `input_as` is **not supported** on `Option<Option<T>>` fields (it conflicts with tri-state semantics).

## 4. Workflow: Deserialize, Validate, Convert

The typical flow for handling input is:

### Insert Flow

```rust
use pgorm::changeset::ValidationErrors;

// 1. Deserialize from untrusted input
let input: NewUserInput = serde_json::from_str(json_body)?;

// 2. Validate all fields at once
let errors = input.validate();
if !errors.is_empty() {
    // Return errors to the client
    return Err(serde_json::to_string(&errors)?);
}

// 3. Convert to model (also validates + converts input_as types)
let new_user: NewUser = input.try_into_model()?;

// 4. Insert into the database
let user: User = new_user.insert_returning(&client).await?;
```

You can skip the explicit `validate()` step -- `try_into_model()` validates internally and returns `ValidationErrors` on failure:

```rust
let input: NewUserInput = serde_json::from_str(json_body)?;
let new_user: NewUser = match input.try_into_model() {
    Ok(v) => v,
    Err(errs) => {
        // errs is ValidationErrors
        return Err(serde_json::to_string(&errs)?);
    }
};
```

### Update (Patch) Flow

```rust
// JSON with partial fields: {"email": "bob@example.com", "homepage": null}
let patch_input: UserPatchInput = serde_json::from_str(patch_json)?;
let patch: UserPatch = patch_input.try_into_patch()?;
let updated: User = patch.update_by_id_returning(&client, user_id).await?;
```

In the JSON above:
- `email` is present with a value -- it will be updated
- `homepage` is explicitly `null` -- it will be set to NULL in the database
- `name` is missing -- it will be skipped (no change)

## 5. Custom Validators

Use `#[orm(custom = "path::to::fn")]` for validation logic that cannot be expressed with built-in attributes. The function receives the field value (including Option wrapper) and returns `Result<(), String>`:

```rust
fn validate_slug(v: &Option<String>) -> Result<(), String> {
    if let Some(s) = v.as_deref() {
        if s.contains(' ') {
            return Err("must not contain spaces".to_string());
        }
    }
    Ok(())
}

#[derive(Debug, InsertModel)]
#[orm(table = "posts", returning = "Post")]
#[orm(input)]
struct NewPost {
    #[orm(custom = "validate_slug")]
    slug: Option<String>,

    title: String,
}
```

## 6. Error Response Format

`ValidationErrors` implements `serde::Serialize`, so you can return it directly as a JSON response from your API. The format is a map of field names to error messages:

```rust
use pgorm::changeset::ValidationErrors;

let input: NewUserInput = serde_json::from_str(r#"
    {
        "name": "A",
        "email": "not-an-email",
        "age": 200,
        "external_id": "not-a-uuid",
        "homepage": "not-a-url"
    }
"#)?;

let errors = input.validate();
if !errors.is_empty() {
    // Serialize to JSON for the API response
    let json = serde_json::to_string_pretty(&errors)?;
    println!("{json}");
}
```

Example output:

```json
{
  "name": ["length must be between 2 and 100"],
  "email": ["invalid email format"],
  "age": ["must be between 0 and 150"],
  "external_id": ["invalid UUID format"],
  "homepage": ["invalid URL format"]
}
```

## Runnable Example

See `crates/pgorm/examples/changeset/main.rs` for a complete example with both insert and update validation flows.

## Next

- Next: [Migrations](/en/guide/migrations)
