# QueryParams（从参数 struct 生成 Query Builder）

`QueryParams` 是一个面向 Web API / 管理后台“搜索条件”场景的过程宏：把一堆 `Option<T>` 入参收敛成一个 struct，然后自动生成 `apply()` / `into_query()`，用同一套条件同时驱动 `search` 和 `count`。

> 本质上它只是 `Model::query()` 链式调用的语法糖，不引入新的 DSL。

## 快速示例：search + count 共用一套条件

```rust
use pgorm::QueryParams;

#[derive(QueryParams)]
#[orm(model = "AuditLog")]
pub struct AuditLogSearchParams<'a> {
    // WHERE
    #[orm(eq(AuditLogQuery::COL_USER_ID))]
    pub user_id: Option<uuid::Uuid>,

    #[orm(ilike(AuditLogQuery::COL_OPERATION_TYPE))]
    pub operation_type_like: Option<&'a str>,

    #[orm(eq(AuditLogQuery::COL_IP_ADDRESS), map(parse_ip))]
    pub ip: Option<&'a str>,

    // ORDER BY / Pagination
    #[orm(order_by_desc)]
    pub order_by_desc: Option<&'a str>,

    #[orm(page(per_page = per_page.unwrap_or(20)))]
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

fn parse_ip(s: &str) -> Option<std::net::IpAddr> {
    s.parse().ok()
}

// 同一个 params -> 同一个 Query Builder
let q = params.into_query()?;
let rows = q.find(&client).await?;
let total = q.count(&client).await?;
```

## 生成的方法

- `params.apply(q)`：把 params 应用到一个已有的 `<Model>Query`
- `params.into_query()`：等价于 `params.apply(Model::query())`

## 属性语法

### struct 级别

- `#[orm(model = "TypePath")]`：指定 model 类型（必须有 `Model::query()`）

### field 级别：WHERE 过滤（与 Query Builder 对齐）

每个 field **只能选择一个 op**（如果需要多个条件，拆成多个字段，或者用 `and/or/raw` 自己组合）。

- 等值 / 比较：
  - `#[orm(eq(COL))]` / `ne/gt/gte/lt/lte`
- 模糊匹配：
  - `#[orm(like(COL))]` / `ilike/not_like/not_ilike`
- NULL 判断（字段类型要求 `bool` 或 `Option<bool>`）：
  - `#[orm(is_null(COL))]` / `is_not_null`
  - `bool`：`true` 才会追加条件
  - `Option<bool>`：`Some(true)` 才会追加条件
- 列表：
  - `#[orm(in_list(COL))]` / `not_in`（字段类型要求 `Vec<T>` 或 `Option<Vec<T>>`）
- 范围：
  - `#[orm(between(COL))]` / `not_between`（字段类型要求 `(T, T)` 或 `Option<(T, T)>`）

### field 级别：排序与分页

- ORDER BY：
  - `#[orm(order_by)]`（字段类型 `OrderBy` / `Option<OrderBy>`）
  - `#[orm(order_by_asc)]` / `#[orm(order_by_desc)]`（字段类型通常是 `&str`/`String` 或 `Option<...>`，会走标识符校验）
  - `#[orm(order_by_raw)]`（escape hatch，注意 SQL 注入）
- Pagination：
  - `#[orm(paginate)]`（字段类型 `Pagination` / `Option<Pagination>`）
  - `#[orm(limit)]` / `#[orm(offset)]`（字段类型 `i64` / `Option<i64>`）
  - `#[orm(page)]`（字段类型 `(i64, i64)` / `Option<(i64, i64)>`）
  - `#[orm(page(per_page = EXPR))]`（字段类型 `i64` / `Option<i64>`，`EXPR` 可以引用 struct 里其它字段变量）

### 预处理：`map(...)`

很多业务入参是 `Option<&str>`，但数据库字段是更强类型（例如 `IpAddr` / `Uuid`）。你可以在同一个字段上加一个 mapper：

```rust
#[orm(eq(AuditLogQuery::COL_IP_ADDRESS), map(parse_ip))]
ip: Option<&str>,
```

mapper 约定：`fn(T) -> Option<U>`，返回 `None` 代表“跳过不加条件”（而不是报错）。

### 逃生舱：`raw / and / or`

当条件组合非常复杂时：

- `#[orm(raw)]`：追加 raw WHERE 片段（字段类型 `String` / `&str` / `Option<...>`）
- `#[orm(and)]` / `#[orm(or)]`：把一个 `WhereExpr` 合并进 query（字段类型 `WhereExpr` / `Option<WhereExpr>`）

> 注意：`raw/order_by_raw` 都可能带来 SQL 注入风险，只建议用于**可信的硬编码**字符串。

## 常见写法建议

- 输入侧优先用 owned 类型（例如 `Option<String>`），或者用 `map(...)` 做解析/转换。
- 时间范围要么写两个字段：`gte + lte`，要么用一个 `Option<(from, to)>` + `between`。
- `page(per_page = ...)` 的 `EXPR` 会在 `apply()` 里展开，所以可以自然引用其它字段（例如 `per_page.unwrap_or(20)`）。

