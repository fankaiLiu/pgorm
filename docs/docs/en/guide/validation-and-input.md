# Input Validation: `#[orm(input)]`

When you build write models from untrusted input (JSON bodies, forms, messages), you usually want:

- a dedicated `Input` struct for deserialization (and patch semantics)
- centralized validation (required/len/range/email/url/uuid/…)
- a machine-friendly error list you can return from your API

`#[orm(input)]` does this for `InsertModel` and `UpdateModel`: it generates a `*Input` type plus helpers like `validate()` and `try_into_model()/try_into_patch()`.

## 0) Prerequisites (features)

- `derive` (proc macros)
- `validate` (if you use `#[orm(email)]`, `#[orm(url)]`, `#[orm(regex = ...)]`, …)

Default features include them. If you disabled defaults:

```toml
[dependencies]
pgorm = { version = "0.1.1", features = ["derive", "validate"] }
```

## 1) Insert: generate `NewXxxInput` + validate + try_into_model

```rust
use pgorm::InsertModel;

#[derive(Debug, InsertModel)]
#[orm(table = "users", returning = "User")]
#[orm(input)] // generates NewUserInput
struct NewUser {
    #[orm(len = "2..=100")]
    name: String,

    #[orm(email)]
    email: String,

    #[orm(range = "0..=150")]
    age: Option<i32>,

    // Accept String in Input, validate+parse into uuid::Uuid with ValidationErrors
    #[orm(uuid, input_as = "String")]
    external_id: uuid::Uuid,

    #[orm(url)]
    homepage: Option<String>,
}
```

The generated `NewUserInput`:

- `derive(serde::Deserialize)` so you can parse JSON directly
- `validate()` → `pgorm::changeset::ValidationErrors`
- `try_into_model()` → `Result<NewUser, ValidationErrors>`

Usage:

```rust
let input: NewUserInput = serde_json::from_str(json_body)?;
let new_user: NewUser = input.try_into_model()?;
let user: User = new_user.insert_returning(&client).await?;
```

> Runnable example: `crates/pgorm/examples/changeset`.

## 2) Update: generate `XxxPatchInput` + try_into_patch (tri-state)

For `UpdateModel`, Input helps you distinguish “missing field” vs “explicit null”.

```rust
use pgorm::UpdateModel;

#[derive(Debug, UpdateModel)]
#[orm(table = "users", model = "User", returning = "User")]
#[orm(input)] // generates UserPatchInput
struct UserPatch {
    #[orm(len = "2..=100")]
    name: Option<String>, // None = missing/skip; Some(v) = update

    #[orm(email)]
    email: Option<String>,

    #[orm(url)]
    homepage: Option<Option<String>>, // None = skip; Some(None)=NULL; Some(Some(v))=value
}
```

Usage:

```rust
let patch_input: UserPatchInput = serde_json::from_str(r#"{"email":"a@b.com","homepage":null}"#)?;
let patch: UserPatch = patch_input.try_into_patch()?;
let updated: User = patch.update_by_id_returning(&client, user_id).await?;
```

## 3) Supported validation attributes (common ones)

| Attribute | Meaning |
|---|---|
| `#[orm(len = "min..=max")]` | string length |
| `#[orm(range = "min..=max")]` | numeric range (expressions allowed) |
| `#[orm(email)]` | email format |
| `#[orm(url)]` | URL format |
| `#[orm(uuid)]` | UUID format |
| `#[orm(regex = "pattern")]` | regex match |
| `#[orm(one_of = "a\|b\|c")]` | must be one of the values |
| `#[orm(custom = "path::to::fn")]` | custom validator |

### `input_as` boundary (current)

`input_as` currently only supports:

- `uuid::Uuid`
- `url::Url` (requires you to explicitly depend on the `url` crate)

It is meant to accept a string in the generated Input, then parse into the typed field while returning `ValidationErrors` (instead of serde parse errors).

Also, `input_as` is **not supported** on `Option<Option<T>>` fields (it conflicts with tri-state semantics).

## 4) Custom validators: `custom`

Your function receives the whole field value (including Option). Returning `Err(String)` will be recorded as a validation error:

```rust
fn validate_slug(v: &Option<String>) -> Result<(), String> {
    if let Some(s) = v.as_deref() {
        if s.contains(' ') {
            return Err("must not contain spaces".to_string());
        }
    }
    Ok(())
}
```

## Next

- If you already validate inputs, consider enabling runtime safety nets: [`PgClient / CheckedClient`](/en/guide/runtime-sql-check)
