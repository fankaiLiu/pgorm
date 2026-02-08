# CLI：pgorm-cli

`pgorm-cli` 是一个用于代码生成和 SQL 验证的命令行工具。它可以从 SQL 查询生成 Rust 代码（类似于 sqlc），从数据库 schema 生成模型结构体，以及根据 schema 验证 SQL 文件。

## 安装

```bash
cargo install pgorm-cli
```

## 配置（`pgorm.toml`）

pgorm-cli 通过 `pgorm.toml` 文件进行配置（默认位置：项目根目录）。使用 `pgorm init` 可以生成配置文件。

### 完整配置示例

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

# --- 可选：从 schema 生成 Rust 模型 ---

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

### 配置章节

#### `version` 和 `engine`

- `version` -- 必须为 `"1"`（目前唯一支持的配置版本）
- `engine` -- 可选，如设置则必须为 `"postgres"`

#### `[database]`

| 字段 | 说明 |
|------|------|
| `url` | PostgreSQL 连接 URL（支持 `${ENV_VAR}` 环境变量展开） |
| `schemas` | 要内省的 schema 列表（默认：`["public"]`） |

#### `[schema_cache]`

控制 pgorm-cli 如何缓存数据库 schema 以避免重复内省：

| 字段 | 说明 |
|------|------|
| `dir` | schema 缓存文件目录（默认：`.pgorm`） |
| `file` | schema 缓存文件名（默认：`schema.json`） |
| `mode` | `auto`（缓存有效时使用缓存，否则刷新）、`refresh`（始终重新获取）、`cache_only`（从不连接数据库） |

#### `[[packages]]`

每个 package 定义一组 SQL 查询及其生成的 Rust 代码输出位置。可以定义多个 package。

| 字段 | 说明 |
|------|------|
| `name` | 唯一的 package 名称 |
| `queries` | SQL 查询文件的 glob 模式（例如 `["queries/**/*.sql"]`） |
| `out` | 生成的 Rust 代码输出目录 |

**`[packages.codegen]`** 控制代码生成选项：

| 字段 | 默认值 | 说明 |
|------|--------|------|
| `emit_queries_struct` | `true` | 生成包含每个查询方法的 `Queries` 结构体 |
| `emit_query_constants` | `true` | 生成 `const` SQL 字符串 |
| `emit_tagged_exec` | `true` | 生成带标签的执行方法（用于监控） |
| `row_derives` | `["FromRow", "Debug", "Clone"]` | 生成的行结构体的派生宏 |
| `param_derives` | `[]` | 生成的参数结构体的派生宏 |
| `extra_uses` | `[]` | 添加到生成文件的额外 `use` 语句 |

**`[packages.types]`** 将 PostgreSQL 类型映射到 Rust 类型：

```toml
[packages.types]
"uuid" = "uuid::Uuid"
"timestamptz" = "chrono::DateTime<chrono::Utc>"
"jsonb" = "serde_json::Value"
```

**`[packages.overrides]`** 提供按查询粒度的参数和列类型覆盖：

```toml
[packages.overrides]
param."GetUser".1 = "i64"              # 覆盖 GetUser 查询的参数 $1 类型
column."SearchUsers".created_at = "chrono::DateTime<chrono::Utc>"
```

#### `[models]`

可选章节，用于直接从数据库 schema 生成 Rust 模型结构体：

| 字段 | 默认值 | 说明 |
|------|--------|------|
| `out` | （必填） | 生成的模型文件输出目录 |
| `dialect` | `"pgorm"` | 输出风格（目前仅支持 `"pgorm"`） |
| `include_views` | `false` | 是否包含视图和物化视图 |
| `tables` | `[]` | 指定要生成的表（空 = 所选 schema 中的所有表） |
| `rename` | `{}` | 将 `table` 或 `schema.table` 映射为自定义 Rust 结构体名称 |
| `primary_key` | `{}` | 将 `table` 或 `schema.table` 映射为主键列名（生成 `#[orm(id)]`） |
| `derives` | `["Debug", "Clone", "FromRow", "Model"]` | 生成的结构体的派生宏 |
| `extra_uses` | `[]` | 额外的 `use` 语句 |
| `types` | `{}` | 自定义 Postgres 类型到 Rust 类型的映射 |

## 命令

推荐使用任务导向命令：`init`、`build`、`check`、`schema`、`sql`。

### `pgorm init`

初始化项目：

```bash
pgorm init
pgorm init --config path/to/pgorm.toml
pgorm init --migrations-dir db/migrations
pgorm init --no-migrations
```

默认会写入带注释的 `pgorm.toml` 模板，并初始化迁移目录。

### `pgorm build`

根据配置生成项目产物：
- 从 `[[packages]]` 生成 SQL 查询代码
- 从 `[models]` 生成模型代码（若配置存在）

```bash
pgorm build
pgorm build --config pgorm.toml
pgorm build --database postgres://user:pass@localhost/mydb
pgorm build --dry-run
pgorm build --check
pgorm build --no-models
pgorm build --no-queries
```

选项：

| 标志 | 说明 |
|------|------|
| `--config <FILE>` | 配置文件路径（默认：`pgorm.toml`） |
| `--database <URL>` | 覆盖配置中的 `database.url` |
| `--dry-run` | 打印将要更改的文件但不写入 |
| `--check` | 如果生成的输出有差异则返回非零退出码（CI 模式） |
| `--no-queries` | 跳过 `[[packages]]` 代码生成 |
| `--no-models` | 跳过 `[models]` 代码生成 |

### `pgorm check`

执行无副作用的项目检查：
- `[[packages]]` 的 SQL 与查询代码检查
- `[models]` 生成结果一致性检查（若配置存在）

```bash
pgorm check
pgorm check --deny-warnings
pgorm check --no-models
```

选项：

| 标志 | 说明 |
|------|------|
| `--config <FILE>` | 配置文件路径（默认：`pgorm.toml`） |
| `--database <URL>` | 覆盖配置中的 `database.url` |
| `--deny-warnings` | 将警告视为错误 |
| `--no-queries` | 跳过 package 检查 |
| `--no-models` | 跳过 model 检查 |

### `pgorm schema`

从数据库转储或刷新 schema 缓存：

```bash
pgorm schema
pgorm schema --schemas public,myschema
pgorm schema --database postgres://user:pass@localhost/mydb
```

选项：

| 标志 | 说明 |
|------|------|
| `--config <FILE>` | 配置文件路径（默认：`pgorm.toml`） |
| `--database <URL>` | 覆盖配置中的 `database.url` |
| `--schemas <CSV>` | 逗号分隔的 schema 列表（默认：`public`） |

### `pgorm sql [FILES...]`

验证 SQL 文件的语法错误、lint 问题和 schema 不匹配：

```bash
pgorm sql queries/users.sql queries/orders.sql
pgorm sql queries/**/*.sql
pgorm sql --deny-warnings queries/*.sql
```

支持多语句输入 -- 每条语句单独验证。

选项：

| 标志 | 说明 |
|------|------|
| `--config <FILE>` | 配置文件路径（默认：`pgorm.toml`） |
| `--database <URL>` | 覆盖配置中的 `database.url` |
| `--schemas <CSV>` | 逗号分隔的 schema 列表（默认：取自配置或 `public`） |
| `--deny-warnings` | 将警告视为错误 |

如果未指定文件，则从 stdin 读取。

## 环境变量展开

`pgorm.toml` 中的所有字符串值支持 `${ENV_VAR}` 语法。这对数据库 URL 特别有用：

```toml
[database]
url = "${DATABASE_URL}"
```

pgorm-cli 会在加载时从进程环境中解析 `${DATABASE_URL}`。如果变量未设置，则返回错误。

## CI/CD 集成

在 CI 流水线中使用 `--check` 标志确保生成的代码始终是最新的：

```bash
# 在 CI 脚本中
pgorm check --deny-warnings
pgorm sql --deny-warnings queries/**/*.sql
```

这些命令在有内容过时或存在问题时会以非零状态退出，适合用作 CI 门禁。

GitHub Actions 步骤示例：

```yaml
- name: Check pgorm codegen
  run: |
    cargo install pgorm-cli
    pgorm check --deny-warnings
  env:
    DATABASE_URL: ${{ secrets.DATABASE_URL }}
```
