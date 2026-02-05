# SQL 查询：query() 与 sql()

pgorm 提供两种编写 SQL 的方式：`query()` 用于手写带编号占位符的 SQL，`sql()` 用于动态组装自动编号参数的 SQL。

## 1. `query()` -- 手写 SQL 与编号占位符

当你有一条完整的 SQL 字符串，并使用显式的 `$1, $2, ...` 占位符时，使用 `pgorm::query()`。值通过 `.bind()` 绑定 -- 永远不要使用字符串拼接。

### `.bind()` 绑定参数

按顺序为每个占位符调用一次 `.bind()`：

```rust
use pgorm::query;

let affected = query("UPDATE users SET username = $1 WHERE id = $2")
    .bind("alice_new")
    .bind(42_i64)
    .execute(&client)
    .await?;
```

### `.tag()` 可观测性标签

附加一个标签用于监控和日志。该标签会被 `PgClient` 和 `InstrumentedClient` 使用：

```rust
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .tag("users.by_id")
    .bind(1_i64)
    .fetch_one_as(&pg)
    .await?;
```

### Fetch 方法

| 方法 | 行为 |
|------|------|
| `fetch_all(&client)` | 返回所有行，类型为 `Vec<Row>` |
| `fetch_all_as::<T>(&client)` | 返回所有行，映射为 `Vec<T>` |
| `fetch_one(&client)` | 第一行；0 行返回 `NotFound` 错误 |
| `fetch_one_as::<T>(&client)` | 第一行映射为 `T` |
| `fetch_one_strict(&client)` | 恰好 1 行；0 行 = `NotFound`，2+ 行 = `TooManyRows` |
| `fetch_opt(&client)` | 0 行 = `None`，1+ 行 = `Some(第一行)` |
| `fetch_opt_as::<T>(&client)` | 可选行映射为 `T` |
| `fetch_scalar_one(&client)` | 第一行的第一列 |
| `fetch_scalar_opt(&client)` | 第一列，可选 |
| `fetch_scalar_all(&client)` | 所有行的第一列 |
| `exists(&client)` | 如果有匹配的行则返回 `true` |
| `execute(&client)` | 执行但不返回行（返回受影响的行数） |

### `*_as::<T>` 结构体映射

任何实现了 `FromRow`（通常通过 `#[derive(FromRow)]`）的类型都可以使用：

```rust
use pgorm::{FromRow, query};

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    username: String,
}

let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;

let users: Vec<User> = query("SELECT id, username FROM users ORDER BY id")
    .fetch_all_as(&client)
    .await?;
```

### 标量辅助方法：`fetch_scalar_one`、`fetch_scalar_opt`、`exists()`

当你只关心第一列时：

```rust
let count: i64 = query("SELECT COUNT(*) FROM users WHERE status = $1")
    .bind("active")
    .fetch_scalar_one(&client)
    .await?;

let maybe_max: Option<i64> = query("SELECT MAX(id) FROM users")
    .fetch_scalar_opt(&client)
    .await?;

let has_any: bool = query("SELECT 1 FROM users WHERE status = $1")
    .bind("active")
    .exists(&client)
    .await?;
```

### 常见陷阱

1. `query()` **不会**自动编号占位符 -- 你必须自己写 `$1`、`$2` 等。需要自动编号请使用 `sql()`。
2. 永远不要将不可信的输入拼接到 SQL 中。始终使用 `.bind()`。
3. 标识符（表名/列名）在 PostgreSQL 中不能参数化 -- 对于动态标识符，请使用 `sql().push_ident(...)` 或 `OrderBy`。

## 2. `sql()` -- 动态 SQL 组装

当你的 SQL 需要在运行时动态组装（可选的 WHERE 子句、动态 JOIN 等）时，使用 `pgorm::sql()`。它会自动处理参数编号（`$1, $2, ...`）。

### `push()` 拼接 SQL 片段

```rust
use pgorm::sql;

let mut q = sql("SELECT id, username FROM users WHERE 1=1");

if let Some(status) = status {
    q.push(" AND status = ");
    q.push_bind(status);
}

q.push(" ORDER BY id DESC");
let users: Vec<User> = q.fetch_all_as(&client).await?;
```

### `push_bind()` 绑定参数（自动编号）

每次调用 `push_bind()` 都会追加一个 `$N` 占位符并绑定值：

```rust
let mut q = sql("SELECT * FROM users WHERE id = ");
q.push_bind(1_i64);
// 生成：SELECT * FROM users WHERE id = $1
```

也可以与 `push()` 链式调用：

```rust
q.push(" AND status = ").push_bind("active");
```

### `push_bind_list()` 用于 IN 子句

追加逗号分隔的占位符列表：

```rust
let mut q = sql("SELECT * FROM users WHERE id IN (");
q.push_bind_list([1_i64, 2, 3]);
q.push(")");
// 生成：SELECT * FROM users WHERE id IN ($1, $2, $3)
```

如果列表为空，`push_bind_list([])` 会追加 `NULL`（因此 `IN (NULL)` 是合法的 SQL，不会匹配任何行）。

### `push_ident()` 安全的动态标识符

PostgreSQL 不允许参数化标识符（表名/列名）。使用 `push_ident()` 可以验证标识符以防止注入：

```rust
let mut q = sql("SELECT * FROM users ORDER BY ");
q.push_ident("created_at")?;
q.push(" DESC");
```

只接受 `[a-zA-Z0-9_]` 以及类似 `schema.table.column` 的限定名称。非法标识符会返回 `OrmError::Validation`。

### 拥有所有权的 `.bind()` 变体

用于单行构造（常用于 CTE 子查询），`.bind()` 会获取并返回所有权：

```rust
let sub = sql("SELECT id FROM users WHERE status = ").bind("active");
```

### 调试：`.to_sql()` 和 `.params_ref()`

查看生成的 SQL 和参数数量：

```rust
let q = build_list_users_sql(&filters)?;
println!("SQL: {}", q.to_sql());
println!("params: {}", q.params_ref().len());
```

## 3. 如何选择

| 场景 | 使用 |
|------|------|
| 已知占位符的静态 SQL | `query("SELECT ... WHERE id = $1").bind(id)` |
| 动态 SQL（可选过滤、条件 JOIN） | `sql("SELECT ...").push(...).push_bind(...)` |
| 模型的简单 CRUD | 模型方法（`select_all`、`insert_returning` 等） |
| 类型安全的 WHERE / ORDER BY / 分页 | `Condition`、`WhereExpr`、`OrderBy`、`Pagination` 配合 `sql()` |

## 下一步

- 下一章：[动态过滤与分页](/zh/guide/conditions)
