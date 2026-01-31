# QueryParams (derive query builder apply)

`QueryParams` is a derive macro for the common “search params” use case in APIs/admin dashboards: collect many `Option<T>` inputs into a single struct and generate `apply()` / `into_query()` so the same filter set can be reused for both `search` and `count`.

> It is just ergonomic sugar on top of `Model::query()` (no new DSL).

## Quick example: reuse filters for search + count

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

let q = params.into_query()?;
let rows = q.find(&client).await?;
let total = q.count(&client).await?;
```

## Generated methods

- `params.apply(q)`: apply params onto an existing `<Model>Query`
- `params.into_query()`: equivalent to `params.apply(Model::query())`

## Attribute syntax

### Struct-level

- `#[orm(model = "TypePath")]`: the model type (must provide `Model::query()`)

### Field-level: WHERE filters (mirrors Query Builder)

One field can have **at most one op**. If you need multiple conditions, split into multiple fields, or use `and/or/raw`.

- Equality / comparisons:
  - `#[orm(eq(COL))]` / `ne/gt/gte/lt/lte`
- Pattern match:
  - `#[orm(like(COL))]` / `ilike/not_like/not_ilike`
- NULL checks (field must be `bool` or `Option<bool>`):
  - `#[orm(is_null(COL))]` / `is_not_null`
  - `bool`: only applies when `true`
  - `Option<bool>`: only applies when `Some(true)`
- Lists:
  - `#[orm(in_list(COL))]` / `not_in` (field must be `Vec<T>` or `Option<Vec<T>>`)
- Ranges:
  - `#[orm(between(COL))]` / `not_between` (field must be `(T, T)` or `Option<(T, T)>`)

### Field-level: ordering & pagination

- ORDER BY:
  - `#[orm(order_by)]` (`OrderBy` / `Option<OrderBy>`)
  - `#[orm(order_by_asc)]` / `#[orm(order_by_desc)]` (usually `&str`/`String` or `Option<...>`, identifiers are validated)
  - `#[orm(order_by_raw)]` (escape hatch; beware SQL injection)
- Pagination:
  - `#[orm(paginate)]` (`Pagination` / `Option<Pagination>`)
  - `#[orm(limit)]` / `#[orm(offset)]` (`i64` / `Option<i64>`)
  - `#[orm(page)]` (`(i64, i64)` / `Option<(i64, i64)>`)
  - `#[orm(page(per_page = EXPR))]` (`i64` / `Option<i64>`, `EXPR` can reference other fields)

### Preprocessing: `map(...)`

When your API input is `Option<&str>` but the DB field is strongly typed (e.g. `IpAddr` / `Uuid`), add a mapper:

```rust
#[orm(eq(AuditLogQuery::COL_IP_ADDRESS), map(parse_ip))]
ip: Option<&str>,
```

The mapper must return `Option<T>`; `None` means “skip the filter”.

### Escape hatches: `raw / and / or`

For very custom logic:

- `#[orm(raw)]`: append a raw WHERE fragment (`String` / `&str` / `Option<...>`)
- `#[orm(and)]` / `#[orm(or)]`: merge a `WhereExpr` into the query (`WhereExpr` / `Option<WhereExpr>`)

> `raw/order_by_raw` can introduce SQL injection risks. Use only with trusted, hardcoded SQL.

