# 动态 SQL：`sql()`

当 SQL 需要“按条件拼接”（可选 WHERE、可选 JOIN、可选排序/分页），`pgorm::sql()` 比 `query()` 更合适：

- 你写的是 SQL 片段，`pgorm` 自动生成 `$1, $2, ...` 占位符
- 值仍然通过参数绑定，避免注入
- 最终仍然可以用 `fetch_*`/`execute` 执行

> `sql()` 解决的是“动态拼接 + 自动占位符编号”；更结构化的 WHERE/ORDER/Pagination，请继续看 [`动态条件与分页`](/zh/guide/conditions)。

## 1) 最小示例：可选过滤条件

```rust
use pgorm::sql;

let mut q = sql("SELECT id, username FROM users WHERE 1=1");

if let Some(status) = status {
    q.push(" AND status = ").push_bind(status);
}

if let Some(keyword) = keyword {
    q.push(" AND username ILIKE ").push_bind(format!("%{keyword}%"));
}

q.push(" ORDER BY id DESC");

let users: Vec<User> = q.fetch_all_as(&client).await?;
```

你不需要自己维护 `$n`：每次 `push_bind(...)` 都会追加一个新的占位符。

## 2) `push()` vs `push_bind()`（牢记这个边界）

- `push("...")`：追加原始 SQL 字符串（无参数）
- `push_bind(v)`：追加一个参数占位符，并绑定值 `v`

```rust
let mut q = sql("SELECT * FROM users WHERE id = ");
q.push_bind(1_i64);
```

## 3) `push_bind_list()`：构建 `IN (...)`

```rust
let mut q = sql("SELECT * FROM users WHERE id IN (");
q.push_bind_list([1_i64, 2, 3]);
q.push(")");
```

如果列表为空，`push_bind_list([])` 会写入 `NULL`（得到 `IN (NULL)`，语法合法且不会误匹配）。

## 4) 安全地拼接标识符：`push_ident()`

PostgreSQL **不允许**用参数绑定表名/列名（这是数据库能力限制），所以遇到“用户选择排序字段”这种需求：

- **不要**把列名直接拼进 SQL
- 用 `push_ident(...)` 解析与校验标识符（不合法会报错）

```rust
use pgorm::sql;

let mut q = sql("SELECT * FROM users ORDER BY ");
q.push_ident("created_at")?; // 动态列名：会校验并正确转义
q.push(" DESC");
```

更推荐的做法是用 `OrderBy`（它同样会校验标识符）：见 [`动态条件与分页`](/zh/guide/conditions)。

## 5) 调试：查看最终 SQL 与参数数量

```rust
let sql_text = q.to_sql();
let param_count = q.params_ref().len();
```

你可以在日志里输出 `to_sql()` 的结果用于排查“拼接出来的 SQL 到底长什么样”。

## 下一步

- 下一章：[`动态条件与分页`](/zh/guide/conditions)
