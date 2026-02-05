# 批量更新/删除（Bulk Update/Delete）设计与计划

状态：Draft
相关代码：`crates/pgorm/src/sql.rs` / `crates/pgorm/src/builder.rs`
最后更新：2026-02-05

## 背景

当前 `pgorm` 提供了强大的批量插入能力（`insert_many` / `upsert_many`），但批量更新和删除需要用户手写 SQL：

```rust,ignore
// 当前方式：手写 SQL
pgorm::query("UPDATE users SET status = $1 WHERE created_at < $2")
    .bind("inactive")
    .bind(one_year_ago)
    .execute(&client)
    .await?;
```

提供 `update_many()` 和 `delete_many()` 可以：
1. 与 `insert_many` 形成完整的批量操作套件
2. 利用现有的 `WhereExpr` / `Condition` 类型安全地构建条件
3. 配合 `PgClient` 的安全策略（`DangerousDmlPolicy`）防止误删全表

## 目标 / 非目标

### 目标

1. 提供 `Sql::update_many()` 方法，支持条件批量更新。
2. 提供 `Sql::delete_many()` 方法，支持条件批量删除。
3. 支持 `RETURNING` 返回受影响的行。
4. 与 `PgClient` 的安全策略集成（强制要求 WHERE 子句）。
5. 返回 `affected_rows` 统计。

### 非目标

- 跨表批量更新（`UPDATE ... FROM ...`，可作为独立 RFC）。
- 批量更新不同值到不同行（如按 ID 更新不同 status，建议用 `upsert_many`）。
- 软删除的自动处理（由 soft_delete RFC 覆盖）。

## 方案

### 1) update_many()

```rust,ignore
impl Sql {
    /// 批量更新：UPDATE table SET ... WHERE ...
    pub fn update_many(self, sets: impl IntoIterator<Item = SetExpr>) -> UpdateManyBuilder;
}

pub struct SetExpr {
    column: Ident,
    value: Box<dyn ToSql + Send + Sync>,
}

impl SetExpr {
    pub fn set<T: ToSql + Send + Sync + 'static>(column: impl Into<Ident>, value: T) -> Self;
    pub fn increment(column: impl Into<Ident>, amount: i64) -> Self; // col = col + amount
    pub fn raw(expr: &'static str) -> Self; // col = NOW() 等
}
```

### 2) UpdateManyBuilder

```rust,ignore
pub struct UpdateManyBuilder {
    table: Ident,
    sets: Vec<SetExpr>,
    where_clause: Option<WhereExpr>,
}

impl UpdateManyBuilder {
    /// 添加 WHERE 条件（必须调用，否则执行时报错）
    pub fn filter(self, condition: impl Into<WhereExpr>) -> Self;

    /// 执行更新，返回受影响行数
    pub async fn execute<C: GenericClient>(self, client: &C) -> OrmResult<u64>;

    /// 执行更新并返回受影响的行
    pub async fn returning<R: FromRow, C: GenericClient>(self, client: &C) -> OrmResult<Vec<R>>;

    /// 流式返回受影响的行
    pub async fn returning_stream<R: FromRow, C: GenericClient>(self, client: &C) -> OrmResult<RowStream<R>>;
}
```

### 3) delete_many()

```rust,ignore
impl Sql {
    /// 批量删除：DELETE FROM table WHERE ...
    pub fn delete_many(self) -> DeleteManyBuilder;
}

pub struct DeleteManyBuilder {
    table: Ident,
    where_clause: Option<WhereExpr>,
}

impl DeleteManyBuilder {
    /// 添加 WHERE 条件（必须调用，否则执行时报错）
    pub fn filter(self, condition: impl Into<WhereExpr>) -> Self;

    /// 执行删除，返回受影响行数
    pub async fn execute<C: GenericClient>(self, client: &C) -> OrmResult<u64>;

    /// 执行删除并返回被删除的行
    pub async fn returning<R: FromRow, C: GenericClient>(self, client: &C) -> OrmResult<Vec<R>>;
}
```

### 4) 安全机制

与 `PgClient` 的 `DangerousDmlPolicy` 集成：

```rust,ignore
pub enum DangerousDmlPolicy {
    /// 允许无 WHERE 的 UPDATE/DELETE（危险）
    Off,
    /// 无 WHERE 时打印警告
    Warn,
    /// 无 WHERE 时返回错误（推荐）
    Enforce,
}
```

当 `DangerousDmlPolicy::Enforce` 时：

```rust,ignore
// 这会返回错误：OrmError::UnsafeDelete
pgorm::sql("users")
    .delete_many()
    .execute(&pg_client)  // 没有 .filter()
    .await?;
```

### 5) 显式全表操作

如果确实需要更新/删除全表，提供显式方法：

```rust,ignore
impl UpdateManyBuilder {
    /// 显式标记"我知道这会影响全表"
    pub fn all_rows(self) -> Self;
}

impl DeleteManyBuilder {
    /// 显式标记"我知道这会删除全表"
    pub fn all_rows(self) -> Self;
}

// 使用
pgorm::sql("temp_data")
    .delete_many()
    .all_rows()  // 显式声明
    .execute(&client)
    .await?;
```

## 使用示例

### A) 基础批量更新

```rust,ignore
use pgorm::prelude::*;

// UPDATE users SET status = 'inactive' WHERE last_login < $1
let affected = pgorm::sql("users")
    .update_many([
        SetExpr::set("status", "inactive"),
    ])
    .filter(Condition::lt("last_login", one_year_ago)?)
    .execute(&client)
    .await?;

println!("Deactivated {} users", affected);
```

### B) 多字段更新

```rust,ignore
// UPDATE orders SET status = 'shipped', shipped_at = NOW() WHERE id = ANY($1)
let order_ids = vec![1_i64, 2, 3, 4, 5];

let affected = pgorm::sql("orders")
    .update_many([
        SetExpr::set("status", "shipped"),
        SetExpr::raw("shipped_at = NOW()"),
    ])
    .filter(Condition::eq_any("id", order_ids)?)
    .execute(&client)
    .await?;
```

### C) 递增更新

```rust,ignore
// UPDATE products SET view_count = view_count + 1 WHERE id = $1
pgorm::sql("products")
    .update_many([
        SetExpr::increment("view_count", 1),
    ])
    .filter(Condition::eq("id", product_id)?)
    .execute(&client)
    .await?;
```

### D) 带 RETURNING 的更新

```rust,ignore
#[derive(FromRow)]
struct UpdatedUser {
    id: i64,
    email: String,
    status: String,
}

// UPDATE users SET status = 'premium' WHERE subscription_ends_at > NOW()
// RETURNING id, email, status
let upgraded: Vec<UpdatedUser> = pgorm::sql("users")
    .update_many([
        SetExpr::set("status", "premium"),
    ])
    .filter(WhereExpr::raw("subscription_ends_at > NOW()"))
    .returning(&client)
    .await?;

for user in upgraded {
    println!("Upgraded user {} ({})", user.id, user.email);
}
```

### E) 基础批量删除

```rust,ignore
// DELETE FROM sessions WHERE expires_at < NOW()
let deleted = pgorm::sql("sessions")
    .delete_many()
    .filter(WhereExpr::raw("expires_at < NOW()"))
    .execute(&client)
    .await?;

println!("Cleaned up {} expired sessions", deleted);
```

### F) 带条件的批量删除

```rust,ignore
// DELETE FROM audit_logs WHERE created_at < $1 AND level != 'error'
let cutoff = Utc::now() - Duration::days(90);

let deleted = pgorm::sql("audit_logs")
    .delete_many()
    .filter(WhereExpr::and([
        Condition::lt("created_at", cutoff)?.into(),
        Condition::ne("level", "error")?.into(),
    ]))
    .execute(&client)
    .await?;
```

### G) 删除并返回被删除的行

```rust,ignore
#[derive(FromRow)]
struct DeletedOrder {
    id: i64,
    user_id: i64,
    total: Decimal,
}

// DELETE FROM orders WHERE status = 'cancelled' AND created_at < $1
// RETURNING *
let deleted_orders: Vec<DeletedOrder> = pgorm::sql("orders")
    .delete_many()
    .filter(WhereExpr::and([
        Condition::eq("status", "cancelled")?.into(),
        Condition::lt("created_at", one_year_ago)?.into(),
    ]))
    .returning(&client)
    .await?;

// 可以用于审计或发送通知
for order in &deleted_orders {
    audit_log::record_deletion("order", order.id);
}
```

### H) 事务中的批量操作

```rust,ignore
pgorm::transaction!(client, async |tx| {
    // 先更新状态
    pgorm::sql("orders")
        .update_many([SetExpr::set("status", "archived")])
        .filter(Condition::lt("created_at", archive_cutoff)?)
        .execute(tx)
        .await?;

    // 再删除关联数据
    pgorm::sql("order_items")
        .delete_many()
        .filter(WhereExpr::raw("order_id IN (SELECT id FROM orders WHERE status = 'archived')"))
        .execute(tx)
        .await?;

    Ok(())
});
```

## 生成的 SQL

### update_many

```sql
UPDATE users
SET status = $1, updated_at = NOW()
WHERE last_login < $2
```

### delete_many

```sql
DELETE FROM sessions
WHERE expires_at < $1
```

### with RETURNING

```sql
UPDATE users
SET status = $1
WHERE created_at < $2
RETURNING id, email, status
```

## API 一览

### SetExpr 构造方法

| 方法 | 生成 SQL | 示例 |
|------|----------|------|
| `set(col, val)` | `col = $n` | `SET status = $1` |
| `increment(col, n)` | `col = col + n` | `SET count = count + 1` |
| `raw(expr)` | `expr` | `SET updated_at = NOW()` |

### 链式方法

```
pgorm::sql("table")
    .update_many([...])  // 或 .delete_many()
    .filter(...)         // WHERE 条件（推荐必须）
    .all_rows()          // 可选：显式允许全表操作
    .execute()           // 或 .returning()
```

## 关键取舍

| 选项 | 优点 | 缺点 | 决策 |
|------|------|------|------|
| 强制 WHERE vs 可选 | 安全 | 灵活性降低 | **可配置策略** |
| SetExpr vs HashMap | 类型安全，支持表达式 | 稍繁琐 | **SetExpr** |
| 返回 affected_rows vs () | 有用的反馈 | 额外解析 | **返回 affected_rows** |

## 与现有功能的关系

- **PgClient 策略**：`DangerousDmlPolicy` 控制是否允许无 WHERE 的 DML。
- **事务**：`update_many` / `delete_many` 在事务中同样有效。
- **监控**：通过 `QueryMonitor` 可以记录批量操作的执行情况。

## 兼容性与迁移

- 纯新增 API，不影响现有行为。
- 用户可以继续使用 `pgorm::query()` 写原生 UPDATE/DELETE。

## 里程碑 / TODO

### M1（基础实现）

- [ ] `SetExpr` 类型
- [ ] `update_many()` 方法
- [ ] `delete_many()` 方法
- [ ] `execute()` 返回 affected_rows
- [ ] 单元测试

### M2（安全与 RETURNING）

- [ ] 与 `DangerousDmlPolicy` 集成
- [ ] `all_rows()` 显式全表操作
- [ ] `returning()` 方法
- [ ] `returning_stream()` 方法
- [ ] 集成测试

### M3（文档与示例）

- [ ] `examples/bulk_operations`
- [ ] 中英文文档
- [ ] README 更新

## Open Questions

1. `SetExpr::increment` 是否支持负数？（建议支持，`increment("col", -1)` 即减法）
2. 是否提供 `SetExpr::concat` 用于字符串拼接？（建议后续扩展）
3. `update_many` 是否支持 `FROM` 子句做跨表更新？（建议作为 M2 扩展或独立 RFC）
4. 是否提供 `delete_many().limit(n)` 限制删除数量？（PostgreSQL 不原生支持，需要子查询实现）
