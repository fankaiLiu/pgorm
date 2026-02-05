# 聚合查询（Aggregate Queries）设计与计划

状态：Draft
相关代码：`crates/pgorm/src/sql.rs` / `crates/pgorm/src/builder.rs`
最后更新：2026-02-05

## 背景

在报表、统计、仪表盘等场景中，聚合查询（COUNT、SUM、AVG 等）是高频需求。当前 `pgorm` 作为 SQL-first ORM，用户需要手写完整的聚合 SQL：

```rust,ignore
let count: i64 = pgorm::query("SELECT COUNT(*) FROM users WHERE status = $1")
    .bind("active")
    .fetch_one_scalar(&client)
    .await?;
```

这种方式虽然灵活，但对于简单聚合显得冗余。提供便捷方法可以提升开发体验，同时保持 SQL-first 的透明性。

## 目标 / 非目标

### 目标

1. 提供 `count()`、`sum()`、`avg()`、`min()`、`max()` 便捷方法。
2. 支持 `group_by()` 构建器，生成分组聚合查询。
3. 支持 `having()` 子句过滤分组结果。
4. 方法可链式调用，与现有 `Sql` builder 风格一致。
5. 返回类型明确，支持泛型（如 `sum::<i64>()`）。

### 非目标

- 窗口函数（Window Functions）的完整支持（可作为独立 RFC）。
- 复杂嵌套聚合或子查询聚合。
- CUBE、ROLLUP 等高级分组（可后续扩展）。

## 方案

### 1) 基础聚合方法

在 `Sql` builder 上新增聚合方法：

```rust,ignore
impl Sql {
    /// SELECT COUNT(*) FROM table WHERE ...
    pub fn count(self) -> AggregateQuery<i64>;

    /// SELECT COUNT(column) FROM table WHERE ...
    pub fn count_column(self, column: impl Into<Ident>) -> AggregateQuery<i64>;

    /// SELECT COUNT(DISTINCT column) FROM table WHERE ...
    pub fn count_distinct(self, column: impl Into<Ident>) -> AggregateQuery<i64>;

    /// SELECT SUM(column) FROM table WHERE ...
    pub fn sum<T>(self, column: impl Into<Ident>) -> AggregateQuery<T>;

    /// SELECT AVG(column) FROM table WHERE ...
    pub fn avg<T>(self, column: impl Into<Ident>) -> AggregateQuery<T>;

    /// SELECT MIN(column) FROM table WHERE ...
    pub fn min<T>(self, column: impl Into<Ident>) -> AggregateQuery<T>;

    /// SELECT MAX(column) FROM table WHERE ...
    pub fn max<T>(self, column: impl Into<Ident>) -> AggregateQuery<T>;
}
```

### 2) AggregateQuery 类型

```rust,ignore
pub struct AggregateQuery<T> {
    inner: Sql,
    _marker: PhantomData<T>,
}

impl<T: FromSql<'_> + Send + Sync> AggregateQuery<T> {
    /// 执行聚合查询，返回单一结果
    pub async fn fetch_one<C: GenericClient>(self, client: &C) -> OrmResult<T>;

    /// 执行聚合查询，结果可能为 NULL
    pub async fn fetch_opt<C: GenericClient>(self, client: &C) -> OrmResult<Option<T>>;
}
```

### 3) GROUP BY 构建器

```rust,ignore
impl Sql {
    /// 添加 GROUP BY 子句
    pub fn group_by(self, columns: impl IntoIterator<Item = impl Into<Ident>>) -> GroupedSql;
}

pub struct GroupedSql {
    inner: Sql,
    group_columns: Vec<Ident>,
}

impl GroupedSql {
    /// 添加 HAVING 子句
    pub fn having(self, condition: WhereExpr) -> Self;

    /// SELECT 指定列 + 聚合
    pub fn select_agg<R: FromRow>(self) -> GroupedQuery<R>;
}
```

### 4) 分组聚合查询

```rust,ignore
pub struct GroupedQuery<R> {
    inner: GroupedSql,
    select_clause: String,
    _marker: PhantomData<R>,
}

impl<R: FromRow + Send + Sync> GroupedQuery<R> {
    /// 执行分组查询，返回多行结果
    pub async fn fetch_all<C: GenericClient>(self, client: &C) -> OrmResult<Vec<R>>;

    /// 流式返回
    pub async fn stream<C: GenericClient>(self, client: &C) -> OrmResult<RowStream<R>>;
}
```

## 使用示例

### A) 简单计数

```rust,ignore
use pgorm::prelude::*;

// SELECT COUNT(*) FROM users WHERE status = 'active'
let count: i64 = pgorm::sql("users")
    .filter(Condition::eq("status", "active")?)
    .count()
    .fetch_one(&client)
    .await?;

println!("Active users: {}", count);
```

### B) 带条件的聚合

```rust,ignore
// SELECT SUM(amount) FROM orders WHERE created_at > $1
let total: Option<Decimal> = pgorm::sql("orders")
    .filter(Condition::gt("created_at", last_month)?)
    .sum::<Decimal>("amount")
    .fetch_opt(&client)
    .await?;
```

### C) COUNT DISTINCT

```rust,ignore
// SELECT COUNT(DISTINCT user_id) FROM orders
let unique_users: i64 = pgorm::sql("orders")
    .count_distinct("user_id")
    .fetch_one(&client)
    .await?;
```

### D) 分组聚合

```rust,ignore
#[derive(FromRow)]
struct StatusCount {
    status: String,
    count: i64,
}

// SELECT status, COUNT(*) as count FROM users GROUP BY status
let stats: Vec<StatusCount> = pgorm::sql("users")
    .group_by(["status"])
    .select("status, COUNT(*) as count")
    .fetch_all(&client)
    .await?;

for stat in stats {
    println!("{}: {}", stat.status, stat.count);
}
```

### E) GROUP BY + HAVING

```rust,ignore
#[derive(FromRow)]
struct CategorySales {
    category: String,
    total_sales: Decimal,
}

// SELECT category, SUM(amount) as total_sales
// FROM orders
// WHERE created_at > $1
// GROUP BY category
// HAVING SUM(amount) > 1000
let top_categories: Vec<CategorySales> = pgorm::sql("orders")
    .filter(Condition::gt("created_at", last_month)?)
    .group_by(["category"])
    .having(WhereExpr::raw("SUM(amount) > 1000"))
    .select("category, SUM(amount) as total_sales")
    .fetch_all(&client)
    .await?;
```

### F) 多字段分组

```rust,ignore
#[derive(FromRow)]
struct DailyStats {
    date: NaiveDate,
    status: String,
    count: i64,
    avg_amount: Decimal,
}

// SELECT DATE(created_at) as date, status, COUNT(*) as count, AVG(amount) as avg_amount
// FROM orders
// GROUP BY DATE(created_at), status
// ORDER BY date DESC
let daily_stats: Vec<DailyStats> = pgorm::sql("orders")
    .group_by(["DATE(created_at)", "status"])
    .select("DATE(created_at) as date, status, COUNT(*) as count, AVG(amount) as avg_amount")
    .order_by(OrderBy::desc("date"))
    .fetch_all(&client)
    .await?;
```

## API 设计细节

### 返回类型约定

| 函数 | 返回类型 | 说明 |
|------|----------|------|
| `count()` | `i64` | COUNT 永远返回整数 |
| `count_column()` | `i64` | COUNT(col) 不计 NULL |
| `sum::<T>()` | `Option<T>` | 空表返回 NULL |
| `avg::<T>()` | `Option<T>` | 空表返回 NULL |
| `min::<T>()` | `Option<T>` | 空表返回 NULL |
| `max::<T>()` | `Option<T>` | 空表返回 NULL |

### 类型推断

```rust,ignore
// 显式指定类型
let sum: Decimal = sql.sum::<Decimal>("amount").fetch_one(&client).await?;

// 或通过变量类型推断
let sum: Decimal = sql.sum("amount").fetch_one(&client).await?;
```

## 生成的 SQL

### count()

```sql
SELECT COUNT(*) FROM users WHERE status = $1
```

### sum("amount")

```sql
SELECT SUM(amount) FROM orders WHERE created_at > $1
```

### group_by + select

```sql
SELECT status, COUNT(*) as count
FROM users
GROUP BY status
HAVING COUNT(*) > 10
ORDER BY count DESC
```

## 关键取舍

| 选项 | 优点 | 缺点 | 决策 |
|------|------|------|------|
| 链式 API vs 独立函数 | 与现有 Sql builder 一致 | 实现稍复杂 | **链式 API** |
| 泛型返回 vs 固定类型 | 灵活，支持 Decimal 等 | 需要类型注解 | **泛型返回** |
| HAVING 用 WhereExpr vs raw | 类型安全 | HAVING 常需聚合表达式 | **支持两者** |

## 兼容性与迁移

- 纯新增 API，不影响现有 `Sql` 的行为。
- 用户可以继续使用 `pgorm::query()` 写原生聚合 SQL。

## 里程碑 / TODO

### M1（基础聚合）

- [ ] `count()` / `count_column()` / `count_distinct()`
- [ ] `sum()` / `avg()` / `min()` / `max()`
- [ ] `AggregateQuery<T>` 类型
- [ ] 单元测试

### M2（分组聚合）

- [ ] `group_by()` 方法
- [ ] `having()` 方法
- [ ] `GroupedSql` / `GroupedQuery<R>` 类型
- [ ] 集成测试

### M3（文档与示例）

- [ ] `examples/aggregate_queries`
- [ ] 中英文文档
- [ ] README 更新

## Open Questions

1. `having()` 是否需要支持 `Condition` 类型？（建议支持，但需要先有聚合表达式的 Condition 变体）
2. 是否支持多聚合函数组合？如 `SELECT COUNT(*), SUM(amount), AVG(amount)`（建议通过 `select()` 手写）
3. 是否提供 `array_agg()` / `string_agg()` 等 PostgreSQL 特有聚合？（建议 M2 后扩展）
