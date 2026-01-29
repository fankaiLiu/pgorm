# 可组合查询 (Composable Queries)

> 参考来源：Ecto (Elixir)、Diesel (Rust)、SQLAlchemy (Python)

## 背景

在 `pgorm` 的 SQL-first 设计里，查询往往会遇到：

- 多处复用同一段过滤逻辑（权限/租户隔离、`active`、时间窗口等）
- 由 `Option<T>` / `bool` 驱动的“按需过滤”
- 复杂 `AND/OR/NOT/grouping` 的组合

“可组合查询”的目标是：把这些逻辑拆成可复用片段，用 Rust 的函数/trait 组合起来，同时保持 SQL 显式、参数安全。

本设计文档给出两条推荐路径：

1. **SQL-first**：直接在 `Sql` builder 上组合片段（最贴近 `pgorm` 的核心能力）
2. **Model query builder**：基于 `#[derive(Model)]` 生成的 `<Model>Query` 进行组合

## 设计原则（pgorm 风格）

- **SQL 显式**：组合不会隐藏最终 SQL；可随时打印/监控。
- **参数安全**：值必须使用 bind；动态 identifier 必须走 `IntoIdent` / `push_ident` 校验。
- **Rust 习惯**：可失败操作返回 `OrmResult`，通过 `?` 传播；少用宏，多用普通函数/trait。
- **可测试**：scope/fragment 是普通 Rust 函数，便于单测（例如断言生成的 SQL/参数）。

## 1) SQL-first：在 `Sql` 上组合片段

### 1.1 片段 = `FnOnce(&mut Sql) -> OrmResult<()>`

把查询的某个“片段”封装成闭包：闭包只负责向 `Sql` 追加 SQL 和参数。

```rust,ignore
use pgorm::{sql, OrmResult, Sql};

fn where_status<'a>(status: &'a str) -> impl FnOnce(&mut Sql) -> OrmResult<()> + 'a {
    move |q| {
        q.push(" AND ");
        q.push_ident("status")?;
        q.push(" = ");
        q.push_bind(status);
        Ok(())
    }
}

fn where_role<'a>(role: &'a str) -> impl FnOnce(&mut Sql) -> OrmResult<()> + 'a {
    move |q| {
        q.push(" AND ");
        q.push_ident("role")?;
        q.push(" = ");
        q.push_bind(role);
        Ok(())
    }
}

let mut q = sql("SELECT id, username FROM users WHERE 1=1");

where_status("active")(&mut q)?;
if let Some(role) = role.as_deref() {
    where_role(role)(&mut q)?;
}

q.push(" ORDER BY created_at DESC");
q.limit(100);

let users: Vec<User> = q.fetch_all_as(&client).await?;
```

要点：

- `push_ident(...)` 会对 identifier 做校验（动态列名/表名不要直接 `push` 字符串）。
- 值始终用 `push_bind(...)`，避免 SQL 注入和 `$n` 计数错误。

### 1.2 用 `Condition` / `WhereExpr` 生成 WHERE（避免手写 AND 拼接）

当 WHERE 需要 `OR/grouping/NOT` 时，推荐用结构化表达式，最后再 `append_to_sql`：

```rust,ignore
use pgorm::{sql, Condition, OrmResult, WhereExpr};

fn build_user_where(filter: &UserFilter) -> OrmResult<WhereExpr> {
    let mut expr = WhereExpr::and(Vec::new()); // identity TRUE

    if let Some(status) = filter.status.as_deref() {
        expr = expr.and_with(Condition::eq("status", status)?.into());
    }
    if let Some(min_age) = filter.min_age {
        expr = expr.and_with(Condition::gte("age", min_age)?.into());
    }

    Ok(expr)
}

let expr = build_user_where(&filter)?;

let mut q = sql("SELECT id, username FROM users");
if !expr.is_trivially_true() {
    q.push(" WHERE ");
    expr.append_to_sql(&mut q);
}

let users: Vec<User> = q.fetch_all_as(&client).await?;
```

### 1.3 ORDER BY / Pagination 同样可以结构化组合

```rust,ignore
use pgorm::{sql, OrderBy, Pagination};

let order = OrderBy::new().desc("created_at")?;
let pag = Pagination::page(page, per_page)?;

let mut q = sql("SELECT id, username FROM users WHERE 1=1");
// ... append WHERE fragments ...
order.append_to_sql(&mut q);
pag.append_to_sql(&mut q);
```

## 2) Model query builder：组合 derive 生成的 `<Model>Query`

`#[derive(Model)]` 会生成 `<Model>Query`（例如 `UserQuery`），其内部基于 `WhereExpr/OrderBy/Pagination` 构建 `Sql` 并执行。

### 2.1 基础链式调用（注意 `?`）

`eq/gte/order_by_asc/order_by_desc/page` 等方法会做 identifier 校验，因此返回 `OrmResult`：

```rust,ignore
let q = User::query()
    .eq(UserQuery::status, "active")?
    .gte(UserQuery::age, 18)?
    .order_by_desc(UserQuery::created_at)?
    .page(1, 20)?;

let users: Vec<User> = q.find(&client).await?;
```

> 建议优先使用 `<Model>Query` 生成的列常量（例如 `UserQuery::status`），减少手写字符串带来的拼写错误。

### 2.2 Scopes：普通函数即可（可组合 / 可测试）

推荐把“可复用查询片段”写成普通函数：输入 query，输出 query（可失败就返回 `OrmResult`）。

```rust,ignore
use pgorm::OrmResult;

type Q = UserQuery;

fn active(q: Q) -> OrmResult<Q> {
    q.eq(Q::status, "active")
}

fn adult(q: Q) -> OrmResult<Q> {
    q.gte(Q::age, 18)
}

fn recent(days: i64) -> impl FnOnce(Q) -> OrmResult<Q> {
    move |q| q.gte(Q::created_at, chrono::Utc::now() - chrono::Duration::days(days))
}

let q = User::query();
let q = active(q)?;
let q = adult(q)?;
let q = recent(30)(q)?;

let users: Vec<User> = q.find(&client).await?;
```

### 2.3 条件组合（`Option<T>` / `bool` 驱动）

最朴素、也最 Rust 的写法是 `if let` / `if`：

```rust,ignore
let mut q = User::query();

if let Some(status) = filter.status.as_deref() {
    q = q.eq(UserQuery::status, status)?;
}

if filter.vip_only {
    q = q.eq(UserQuery::tier, "vip")?;
}

let users: Vec<User> = q.find(&client).await?;
```

如果你特别在意“链式风格”，可以在业务侧自定义一个小的扩展 trait（纯语法糖，不要求改 `pgorm`）：

```rust,ignore
use pgorm::OrmResult;

trait Apply: Sized {
    fn apply(self, f: impl FnOnce(Self) -> OrmResult<Self>) -> OrmResult<Self> {
        f(self)
    }

    fn apply_if(self, cond: bool, f: impl FnOnce(Self) -> OrmResult<Self>) -> OrmResult<Self> {
        if cond { f(self) } else { Ok(self) }
    }

    fn apply_some<T>(
        self,
        opt: Option<T>,
        f: impl FnOnce(Self, T) -> OrmResult<Self>,
    ) -> OrmResult<Self> {
        match opt {
            Some(v) => f(self, v),
            None => Ok(self),
        }
    }
}

impl<T> Apply for T {}
```

## 3) AND/OR/grouping：推荐直接用 `WhereExpr`

对复杂布尔逻辑，直接构建表达式树通常最清晰：

```rust,ignore
use pgorm::{Condition, WhereExpr};

// WHERE status = 'active' AND (role = 'admin' OR role = 'owner')
let expr = WhereExpr::and(vec![
    Condition::eq(UserQuery::status, "active")?.into(),
    WhereExpr::or(vec![
        Condition::eq(UserQuery::role, "admin")?.into(),
        Condition::eq(UserQuery::role, "owner")?.into(),
    ]),
]);

let q = User::query().and(expr);
let users: Vec<User> = q.find(&client).await?;
```

同一套 `WhereExpr` 也可以直接 `append_to_sql(&mut Sql)`，在 SQL-first 路径里复用。

## 4) 组织建议：把 scopes 放进独立模块

推荐把查询片段集中放进 `queries/` 目录（或对应业务 module），避免散落在 handler/service 里：

```rust,ignore
// queries/users.rs
use pgorm::OrmResult;

pub type Q = UserQuery;

pub fn dashboard_base(q: Q) -> OrmResult<Q> {
    q.eq(Q::status, "active")?
        .order_by_desc(Q::created_at)
}
```

## 现状与 TODO

- 已有：`Sql`、`Condition`/`WhereExpr`、`OrderBy`、`Pagination`、derive 生成的 `<Model>Query`。
- 可增强（可选）：在 `pgorm` 内部提供通用的 `Apply` 扩展 trait（`apply_if` / `apply_some`），减少业务层样板代码。
