# CLI: pgorm-cli

`pgorm-cli` is a command-line tool for code generation and SQL validation. It can generate Rust code from SQL queries (similar to sqlc), generate model structs from your database schema, and validate SQL files against the schema.

## Installation

```bash
cargo install pgorm-cli
```

## Configuration (`pgorm.toml`)

pgorm-cli is configured via a `pgorm.toml` file (default location: project root). Generate one with `pgorm gen init`.

### Full Configuration Example

```toml
version = "1"
engine = "postgres"

[database]
url = "${DATABASE_URL}"
schemas = ["public"]

[schema_cache]
dir = ".pgorm"
file = "schema.json"
mode = "auto" # auto | refresh | cache_only

[[packages]]
name = "db"
queries = ["queries/**/*.sql"]
out = "src/db"

[packages.codegen]
emit_queries_struct = true
emit_query_constants = true
emit_tagged_exec = true

row_derives = ["FromRow", "Debug", "Clone"]
param_derives = ["Debug", "Clone"]
extra_uses = ["pgorm::FromRow"]

[packages.types]
"uuid" = "uuid::Uuid"
"timestamptz" = "chrono::DateTime<chrono::Utc>"
"jsonb" = "serde_json::Value"

[packages.overrides]
# param."GetUser".1 = "i64"
# column."SearchUsers".created_at = "chrono::DateTime<chrono::Utc>"

# --- Optional: generate Rust models from schema ---

# [models]
# out = "src/models"
# dialect = "pgorm"
# include_views = false
# tables = []
#
# [models.types]
# "uuid" = "uuid::Uuid"
# "timestamptz" = "chrono::DateTime<chrono::Utc>"
```

### Configuration Sections

#### `version` and `engine`

- `version` -- must be `"1"` (the only supported config version)
- `engine` -- optional, must be `"postgres"` if set

#### `[database]`

| Field | Description |
|-------|-------------|
| `url` | PostgreSQL connection URL (supports `${ENV_VAR}` expansion) |
| `schemas` | List of schemas to introspect (default: `["public"]`) |

#### `[schema_cache]`

Controls how pgorm-cli caches the database schema to avoid repeated introspection:

| Field | Description |
|-------|-------------|
| `dir` | Directory for the schema cache file (default: `.pgorm`) |
| `file` | Schema cache filename (default: `schema.json`) |
| `mode` | `auto` (use cache if fresh, else refresh), `refresh` (always re-fetch), `cache_only` (never connect to DB) |

#### `[[packages]]`

Each package defines a set of SQL queries and where to output the generated Rust code. You can define multiple packages.

| Field | Description |
|-------|-------------|
| `name` | Unique package name |
| `queries` | Glob patterns for SQL query files (e.g. `["queries/**/*.sql"]`) |
| `out` | Output directory for generated Rust code |

**`[packages.codegen]`** controls code generation options:

| Field | Default | Description |
|-------|---------|-------------|
| `emit_queries_struct` | `true` | Generate a `Queries` struct with methods for each query |
| `emit_query_constants` | `true` | Generate `const` SQL strings |
| `emit_tagged_exec` | `true` | Generate tagged execution methods (for monitoring) |
| `row_derives` | `["FromRow", "Debug", "Clone"]` | Derive macros for generated row structs |
| `param_derives` | `[]` | Derive macros for generated param structs |
| `extra_uses` | `[]` | Additional `use` statements added to generated files |

**`[packages.types]`** maps PostgreSQL types to Rust types:

```toml
[packages.types]
"uuid" = "uuid::Uuid"
"timestamptz" = "chrono::DateTime<chrono::Utc>"
"jsonb" = "serde_json::Value"
```

**`[packages.overrides]`** provides per-query type overrides for parameters and columns:

```toml
[packages.overrides]
param."GetUser".1 = "i64"              # override param $1 type for GetUser query
column."SearchUsers".created_at = "chrono::DateTime<chrono::Utc>"
```

#### `[models]`

Optional section for generating Rust model structs directly from the database schema:

| Field | Default | Description |
|-------|---------|-------------|
| `out` | (required) | Output directory for generated model files |
| `dialect` | `"pgorm"` | Output style (currently only `"pgorm"`) |
| `include_views` | `false` | Include views and materialized views |
| `tables` | `[]` | Specific tables to generate (empty = all tables in selected schemas) |
| `rename` | `{}` | Map `table` or `schema.table` to a custom Rust struct name |
| `primary_key` | `{}` | Map `table` or `schema.table` to the PK column name (emits `#[orm(id)]`) |
| `derives` | `["Debug", "Clone", "FromRow", "Model"]` | Derive macros for generated structs |
| `extra_uses` | `[]` | Additional `use` statements |
| `types` | `{}` | Custom Postgres type to Rust type mappings |

## Commands

### `pgorm gen init`

Create an initial `pgorm.toml` configuration file:

```bash
pgorm gen init
pgorm gen init --config path/to/pgorm.toml
```

This writes a commented template with all available options. It refuses to overwrite an existing file.

### `pgorm gen`

Generate Rust code from SQL query files defined in the `[[packages]]` sections:

```bash
pgorm gen
pgorm gen --config pgorm.toml
pgorm gen --database postgres://user:pass@localhost/mydb
pgorm gen --dry-run    # print what would change without writing
pgorm gen --check      # exit non-zero if output would change (for CI)
```

Options:

| Flag | Description |
|------|-------------|
| `--config <FILE>` | Config file path (default: `pgorm.toml`) |
| `--database <URL>` | Override `database.url` from config |
| `--dry-run` | Print files that would change without writing |
| `--check` | Exit non-zero if generated output differs (CI mode) |

### `pgorm gen schema`

Dump or refresh the schema cache from the database:

```bash
pgorm gen schema
pgorm gen schema --schemas public,myschema
pgorm gen schema --database postgres://user:pass@localhost/mydb
```

Options:

| Flag | Description |
|------|-------------|
| `--config <FILE>` | Config file path (default: `pgorm.toml`) |
| `--database <URL>` | Override `database.url` from config |
| `--schemas <CSV>` | Comma-separated list of schemas (default: `public`) |

### `pgorm gen check`

Verify that generated code is up-to-date. Useful in CI pipelines to ensure developers have run `pgorm gen` after modifying queries:

```bash
pgorm gen check
pgorm gen check --deny-warnings
```

Options:

| Flag | Description |
|------|-------------|
| `--config <FILE>` | Config file path (default: `pgorm.toml`) |
| `--database <URL>` | Override `database.url` from config |
| `--deny-warnings` | Treat warnings as errors |

### `pgorm model`

Generate Rust model structs from the database schema, using the `[models]` section in your config:

```bash
pgorm model
pgorm model --dry-run
pgorm model --check     # CI mode: exit non-zero if output would change
```

Options:

| Flag | Description |
|------|-------------|
| `--config <FILE>` | Config file path (default: `pgorm.toml`) |
| `--database <URL>` | Override `database.url` from config |
| `--dry-run` | Print files that would change without writing |
| `--check` | Exit non-zero if generated output differs (CI mode) |

### `pgorm sql check [FILES...]`

Validate SQL files for syntax errors, lint issues, and schema mismatches:

```bash
pgorm sql check queries/users.sql queries/orders.sql
pgorm sql check queries/**/*.sql
pgorm sql check --deny-warnings queries/*.sql
```

Supports multi-statement input -- each statement is validated separately.

Options:

| Flag | Description |
|------|-------------|
| `--config <FILE>` | Config file path (default: `pgorm.toml`) |
| `--database <URL>` | Override `database.url` from config |
| `--schemas <CSV>` | Comma-separated list of schemas (default: from config or `public`) |
| `--deny-warnings` | Treat warnings as errors |

If no files are given, reads from stdin.

## Environment Variable Expansion

All string values in `pgorm.toml` support `${ENV_VAR}` syntax. This is especially useful for the database URL:

```toml
[database]
url = "${DATABASE_URL}"
```

pgorm-cli will resolve `${DATABASE_URL}` from the process environment at load time. If the variable is not set, it returns an error.

## CI/CD Integration

Use the `--check` flag in your CI pipeline to ensure generated code is always up-to-date:

```bash
# In your CI script
pgorm gen --check
pgorm model --check
pgorm sql check --deny-warnings queries/**/*.sql
```

These commands exit with a non-zero status if anything is out of date or has issues, making them suitable for CI gates.

Example GitHub Actions step:

```yaml
- name: Check pgorm codegen
  run: |
    cargo install pgorm-cli
    pgorm gen --check
    pgorm model --check
  env:
    DATABASE_URL: ${{ secrets.DATABASE_URL }}
```
