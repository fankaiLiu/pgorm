# Installation & Feature Flags

## Cargo.toml

Add pgorm to your project:

```toml
[dependencies]
pgorm = "0.3.0"
```

Default features (`pool`, `derive`, `check`, `validate`) cover most use cases. For a minimal build with only the SQL builder and row mapping:

```toml
[dependencies]
pgorm = { version = "0.3.0", default-features = false }
```

## Rust Toolchain

- **MSRV:** Rust 1.88+
- **Edition:** 2024

pgorm tracks the latest stable Rust edition. Ensure your toolchain meets the minimum supported version.

## Feature Flags

| Flag | Default | Dependencies | Purpose | Recommended |
|------|---------|-------------|---------|-------------|
| `pool` | Yes | `deadpool-postgres` | Connection pooling (`create_pool`) | Yes for servers |
| `derive` | Yes | `pgorm-derive` (proc-macro) | `FromRow`, `Model`, `InsertModel`, `UpdateModel`, `ViewModel`, `QueryParams` | Yes |
| `check` | Yes | `pgorm-check` + `libpg_query` | SQL schema checking, linting, `PgClient` | Yes for dev/staging |
| `validate` | Yes | `regex`, `url` | Input validation (email/url/regex) | Yes if accepting user input |
| `migrate` | No | `refinery` | SQL migrations | Only for migration runner binary |
| `tracing` | No | `tracing` | Emit SQL via `tracing` (target: `pgorm.sql`) | Yes if using tracing |
| `rust_decimal` | No | `rust_decimal` | `Decimal` type support | As needed |
| `time` | No | `time` | `time` crate date/time support | As needed |
| `cidr` | No | `cidr` | Network type support | As needed |
| `geo_types` | No | `geo-types` | Geometry support | As needed |
| `eui48` | No | `eui48` | MAC address support | As needed |
| `bit_vec` | No | `bit-vec` | Bit vector support | As needed |
| `extra_types` | No | all of the above | Enable all optional type support | Convenience alias |

### Enabling Optional Features

To enable specific features:

```toml
[dependencies]
pgorm = { version = "0.3.0", features = ["tracing", "rust_decimal"] }
```

To enable all extra type support at once:

```toml
[dependencies]
pgorm = { version = "0.3.0", features = ["extra_types"] }
```

## Re-exports

pgorm re-exports key types from `tokio-postgres` so you do not need to add it as a direct dependency for common tasks:

- `tokio_postgres::Row`
- `tokio_postgres::types::ToSql`, `FromSql`, `FromSqlOwned`
- `tokio_postgres::GenericClient`
- `tokio_postgres::NoTls`

When you need the full `tokio-postgres` API (e.g., for raw connections without the pool), add it directly:

```toml
[dependencies]
pgorm = "0.3.0"
tokio-postgres = "0.7"
```

## Minimal Build Example

If you only need the SQL builder (`sql()`, `query()`) and row mapping (`FromRow`) without pooling, derive macros, or runtime SQL checking:

```toml
[dependencies]
pgorm = { version = "0.3.0", default-features = false }
```

This gives you the smallest dependency footprint while retaining the core query API.

---

Next: [Connection & Pooling](/en/guide/connection)
