# 手写 SQL：`query()`

当你已经有一条完整的 SQL（并且你愿意显式写出 `$1, $2, ...` 占位符）时，用 `pgorm::query()` 最直接：

- SQL 字符串由你完全控制
- 参数用 `.bind()` 绑定，避免拼接注入
- 提供一组一致的 `fetch_*`/`execute` API

> 如果你需要“动态拼接 SQL 片段”，而又不想手动维护 `$n` 编号，请看下一章：[`sql()` 动态构建器](/zh/guide/sql-builder)。

## 1) 最小示例：查询并映射到结构体

```rust
use pgorm::{FromRow, query};

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    username: String,
}

// SQL 里手动写 $1
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .tag("users.by_id") // 可选：可观测性标签
    .bind(1_i64)        // 对应 $1
    .fetch_one_as(&client)
    .await?;
```

## 2) 绑定参数：只用 `bind()`，不要字符串拼接

`query()` 的规则很简单：SQL 里写 `$1, $2, ...`，然后按顺序 `.bind(v1).bind(v2)...`。

```rust
let affected = query("UPDATE users SET username = $1 WHERE id = $2")
    .tag("users.rename")
    .bind("alice_new")
    .bind(42_i64)
    .execute(&client)
    .await?;
```

## 3) 常用执行方法（先记住 3 个）

- `fetch_one*`：取“第一行”，0 行报错（NotFound），多行不报错  
- `fetch_one_strict*`：要求“恰好 1 行”，0 行报错，多行报错（TooManyRows）  
- `fetch_opt*`：0 行返回 `Ok(None)`，否则 `Ok(Some(...))`

对应的 `*_as` 会把行映射成 `T: FromRow`：

```rust
let u1: User = query("SELECT id, username FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;

let u2: Option<User> = query("SELECT id, username FROM users WHERE id = $1")
    .bind(2_i64)
    .fetch_opt_as(&client)
    .await?;
```

更多细节（错误类型、严格语义、典型用法）见：[`Fetch 语义`](/zh/guide/fetch-semantics)。

## 4) 标量查询：`fetch_scalar_*` / `exists()`

当你只关心第一列（例如 COUNT / EXISTS），用标量 API 更直观：

```rust
let count: i64 = query("SELECT COUNT(*) FROM users WHERE status = $1")
    .bind("active")
    .fetch_scalar_one(&client)
    .await?;

let maybe_max_id: Option<i64> = query("SELECT MAX(id) FROM users")
    .fetch_scalar_opt(&client)
    .await?;

let has_any: bool = query("SELECT 1 FROM users WHERE status = $1")
    .bind("active")
    .exists(&client)
    .await?;
```

## 5) 可观测性：`tag()`

`tag()` 只是一个字符串标签，用于监控/统计/日志分组（当你使用 `PgClient` / `InstrumentedClient` 等包装器时更有用）：

```rust
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .tag("users.by_id")
    .bind(1_i64)
    .fetch_one_as(&pg) // 这里的 pg 可以是 PgClient / InstrumentedClient 等
    .await?;
```

## 常见坑（提前避开）

1) **占位符编号不会自动生成**：`query()` 不会帮你把 `$n` 编好；动态 SQL 请用 `sql()`。  
2) **不要把用户输入拼进 SQL**：值永远用 `.bind()`。  
3) **标识符（表名/列名）不能参数化**：如果你要动态 ORDER BY 列名，用 `sql().push_ident(...)` 做校验与转义。  

## 下一步

- 下一章：[`动态 SQL：sql()`](/zh/guide/sql-builder)
