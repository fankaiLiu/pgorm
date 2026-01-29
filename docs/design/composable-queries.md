# 可组合查询 (Composable Queries)

> 参考来源：Ecto (Elixir)、Diesel (Rust)、SQLAlchemy (Python)

## 概述

可组合查询允许将查询逻辑封装为可复用的函数，通过链式调用组合成复杂查询。这是函数式编程在数据库查询中的应用。

## 设计原则

- **函数式组合**：查询是不可变的，每次调用返回新查询
- **类型安全**：组合后的查询保持类型信息
- **延迟执行**：查询构建与执行分离
- **零成本抽象**：编译器可以优化掉中间状态

## 现有基础

pgorm 已有 `ModelQuery` 结构，支持基本的链式调用：

```rust
// 当前支持
User::query()
    .eq("status", "active")
    .gte("age", 18)
    .order_by_desc("created_at")
    .limit(10)
    .find(&client).await?;
```

## 增强目标

### 1. 作用域方法 (Scopes)

```rust
// 在 Model 上定义可复用的查询片段
impl User {
    /// 活跃用户
    fn active() -> impl Fn(UserQuery) -> UserQuery {
        |q| q.eq("status", "active")
    }

    /// 成年用户
    fn adult() -> impl Fn(UserQuery) -> UserQuery {
        |q| q.gte("age", 18)
    }

    /// 最近注册（N 天内）
    fn recent(days: i32) -> impl Fn(UserQuery) -> UserQuery {
        move |q| q.gte("created_at", Utc::now() - Duration::days(days as i64))
    }

    /// VIP 用户
    fn vip() -> impl Fn(UserQuery) -> UserQuery {
        |q| q.eq("tier", "vip")
    }
}

// 组合使用
let users = User::query()
    .apply(User::active())
    .apply(User::adult())
    .apply(User::recent(30))
    .order_by_desc("created_at")
    .find(&client).await?;
```

### 2. 条件组合

```rust
// 根据条件添加过滤器
let mut query = User::query();

if let Some(status) = filter.status {
    query = query.eq("status", status);
}

if let Some(min_age) = filter.min_age {
    query = query.gte("age", min_age);
}

if filter.vip_only {
    query = query.apply(User::vip());
}

let users = query.find(&client).await?;
```

### 3. 管道风格

```rust
// 使用 pipe 方法实现函数式管道
let users = User::query()
    .pipe(|q| if active_only { q.apply(User::active()) } else { q })
    .pipe(|q| if let Some(role) = role { q.eq("role", role) } else { q })
    .find(&client).await?;
```

## API 设计

### Query Trait

```rust
/// 可组合查询的核心 trait
pub trait Composable: Sized {
    /// 应用一个查询修改函数
    fn apply<F>(self, f: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        f(self)
    }

    /// 条件应用
    fn apply_if<F>(self, condition: bool, f: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        if condition {
            f(self)
        } else {
            self
        }
    }

    /// 可选值应用
    fn apply_some<T, F>(self, opt: Option<T>, f: F) -> Self
    where
        F: FnOnce(Self, T) -> Self,
    {
        match opt {
            Some(v) => f(self, v),
            None => self,
        }
    }

    /// 管道操作
    fn pipe<F, R>(self, f: F) -> R
    where
        F: FnOnce(Self) -> R,
    {
        f(self)
    }
}

// 为 ModelQuery 实现
impl<M: Model> Composable for ModelQuery<M> {}
```

### Scope 宏

```rust
/// 定义查询作用域的便捷宏
#[macro_export]
macro_rules! define_scopes {
    ($model:ty {
        $(
            $(#[$meta:meta])*
            $name:ident $(($($param:ident : $ty:ty),*))? => $body:expr
        ),* $(,)?
    }) => {
        impl $model {
            $(
                $(#[$meta])*
                pub fn $name($($($param : $ty),*)?) -> impl Fn(<$model as Model>::Query) -> <$model as Model>::Query {
                    move |q| $body
                }
            )*
        }
    };
}

// 使用
define_scopes!(User {
    /// 活跃用户
    active => q.eq("status", "active"),

    /// 成年用户
    adult => q.gte("age", 18),

    /// 最近 N 天注册
    recent(days: i32) => q.gte("created_at", Utc::now() - Duration::days(days as i64)),

    /// 按角色过滤
    with_role(role: &str) => q.eq("role", role),
});
```

### 复杂组合示例

```rust
// 业务逻辑封装
impl User {
    /// 可发送营销邮件的用户
    fn marketable() -> impl Fn(UserQuery) -> UserQuery {
        |q| q
            .apply(Self::active())
            .eq("email_verified", true)
            .eq("marketing_opt_in", true)
    }

    /// 高价值用户
    fn high_value() -> impl Fn(UserQuery) -> UserQuery {
        |q| q
            .apply(Self::active())
            .gte("total_spent", Decimal::new(10000, 2))  // > $100
            .or(|q| q.apply(Self::vip()))
    }

    /// 搜索用户
    fn search(term: &str) -> impl Fn(UserQuery) -> UserQuery + '_ {
        move |q| q
            .or(|q| q.ilike("name", format!("%{}%", term)))
            .or(|q| q.ilike("email", format!("%{}%", term)))
    }
}

// 组合使用
let users = User::query()
    .apply(User::marketable())
    .apply(User::high_value())
    .apply_some(search_term, |q, term| q.apply(User::search(&term)))
    .order_by_desc("total_spent")
    .limit(100)
    .find(&client).await?;
```

### 查询模块化

```rust
// queries/users.rs
pub mod user_queries {
    use super::*;

    /// 仪表板统计查询
    pub fn dashboard_stats() -> impl Fn(UserQuery) -> UserQuery {
        |q| q
            .apply(User::active())
            .apply(User::recent(30))
    }

    /// 管理员列表查询
    pub fn admin_list() -> impl Fn(UserQuery) -> UserQuery {
        |q| q
            .eq("role", "admin")
            .order_by_asc("name")
    }

    /// 需要关注的用户（异常检测）
    pub fn needs_attention() -> impl Fn(UserQuery) -> UserQuery {
        |q| q
            .apply(User::active())
            .lt("last_login_at", Utc::now() - Duration::days(30))
            .gt("total_spent", Decimal::new(5000, 2))
    }
}

// 使用
use queries::user_queries;

let stats_users = User::query()
    .apply(user_queries::dashboard_stats())
    .find(&client).await?;
```

## Or / And 组合

### 当前问题

当前的 `WhereExpr` 支持 AND/OR，但 API 不够直观：

```rust
// 当前方式（略显繁琐）
let expr = WhereExpr::or(vec![
    WhereExpr::atom(Condition::new("status", Op::Eq("active"))),
    WhereExpr::atom(Condition::new("role", Op::Eq("admin"))),
]);
```

### 改进后的 API

```rust
// 链式 OR
let users = User::query()
    .where_(|w| w
        .eq("status", "active")
        .or()
        .eq("role", "admin")
    )
    .find(&client).await?;

// 嵌套组合
let users = User::query()
    .where_(|w| w
        .group(|g| g.eq("status", "active").and().gte("age", 18))
        .or()
        .eq("role", "admin")
    )
    .find(&client).await?;

// 生成的 SQL:
// WHERE (status = 'active' AND age >= 18) OR role = 'admin'
```

### WhereBuilder 设计

```rust
pub struct WhereBuilder {
    expr: WhereExpr,
    pending_op: Option<LogicalOp>,
}

enum LogicalOp {
    And,
    Or,
}

impl WhereBuilder {
    pub fn new() -> Self {
        Self {
            expr: WhereExpr::Empty,
            pending_op: None,
        }
    }

    pub fn eq<V: ToSql>(mut self, column: &str, value: V) -> Self {
        self.add_condition(Condition::new(column, Op::Eq(value)));
        self
    }

    pub fn and(mut self) -> Self {
        self.pending_op = Some(LogicalOp::And);
        self
    }

    pub fn or(mut self) -> Self {
        self.pending_op = Some(LogicalOp::Or);
        self
    }

    pub fn group<F>(mut self, f: F) -> Self
    where
        F: FnOnce(WhereBuilder) -> WhereBuilder,
    {
        let inner = f(WhereBuilder::new());
        self.add_expr(inner.build());
        self
    }

    fn add_condition(&mut self, cond: Condition) {
        let atom = WhereExpr::Atom(cond);
        self.expr = match self.pending_op.take() {
            Some(LogicalOp::And) => self.expr.and_with(atom),
            Some(LogicalOp::Or) => self.expr.or_with(atom),
            None => atom,
        };
    }

    pub fn build(self) -> WhereExpr {
        self.expr
    }
}
```

## 与 Ecto 的对比

### Ecto 风格

```elixir
# Ecto (Elixir)
User
|> where([u], u.status == "active")
|> where([u], u.age >= 18)
|> order_by([u], desc: u.created_at)
|> limit(10)
|> Repo.all()
```

### pgorm 风格

```rust
// pgorm (Rust) - 更符合 Rust 习惯
User::query()
    .apply(User::active())
    .apply(User::adult())
    .order_by_desc("created_at")
    .limit(10)
    .find(&client).await?
```

### 关键区别

| 特性 | Ecto | pgorm |
|------|------|-------|
| 组合方式 | 管道操作符 `\|>` | 方法链 + `apply()` |
| 绑定变量 | `[u]` 语法 | 闭包捕获 |
| 执行 | `Repo.all()` | `find(&client)` |
| 类型 | 运行时 | 编译时 |

## 实现细节

### ModelQuery 增强

```rust
pub struct ModelQuery<M: Model> {
    table: &'static str,
    select_list: &'static str,
    where_expr: WhereExpr,
    order_by: Vec<OrderItem>,
    limit: Option<i64>,
    offset: Option<i64>,
    _marker: PhantomData<M>,
}

impl<M: Model> ModelQuery<M> {
    // 现有方法...

    /// 应用查询修改函数
    pub fn apply<F>(self, f: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        f(self)
    }

    /// 条件应用
    pub fn apply_if<F>(self, condition: bool, f: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        if condition { f(self) } else { self }
    }

    /// 可选值应用
    pub fn apply_some<T, F>(self, opt: Option<T>, f: F) -> Self
    where
        F: FnOnce(Self, T) -> Self,
    {
        match opt {
            Some(v) => f(self, v),
            None => self,
        }
    }

    /// 结果值应用
    pub fn apply_ok<T, E, F>(self, result: Result<T, E>, f: F) -> Self
    where
        F: FnOnce(Self, T) -> Self,
    {
        match result {
            Ok(v) => f(self, v),
            Err(_) => self,
        }
    }

    /// 使用 WhereBuilder 构建复杂条件
    pub fn where_<F>(mut self, f: F) -> Self
    where
        F: FnOnce(WhereBuilder) -> WhereBuilder,
    {
        let builder = f(WhereBuilder::new());
        self.where_expr = self.where_expr.and_with(builder.build());
        self
    }

    /// OR 条件分支
    pub fn or<F>(mut self, f: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        // 创建一个临时查询来收集条件
        let branch = f(Self::empty());
        self.where_expr = self.where_expr.or_with(branch.where_expr);
        self
    }
}
```

### 零成本验证

确保组合操作不产生运行时开销：

```rust
#[test]
fn test_zero_cost() {
    // 这两个应该生成相同的 SQL
    let q1 = User::query()
        .eq("status", "active")
        .gte("age", 18);

    let q2 = User::query()
        .apply(|q| q.eq("status", "active"))
        .apply(|q| q.gte("age", 18));

    assert_eq!(q1.to_sql(), q2.to_sql());
}
```

## 使用示例

### API 过滤参数

```rust
#[derive(Deserialize)]
pub struct UserFilter {
    pub status: Option<String>,
    pub role: Option<String>,
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub search: Option<String>,
    pub sort_by: Option<String>,
    pub sort_dir: Option<String>,
    pub page: Option<i32>,
    pub per_page: Option<i32>,
}

impl UserFilter {
    pub fn apply(self, query: UserQuery) -> UserQuery {
        query
            .apply_some(self.status, |q, s| q.eq("status", s))
            .apply_some(self.role, |q, r| q.eq("role", r))
            .apply_some(self.min_age, |q, a| q.gte("age", a))
            .apply_some(self.max_age, |q, a| q.lte("age", a))
            .apply_some(self.search, |q, s| q.apply(User::search(&s)))
            .apply_some(self.sort_by, |q, col| {
                match self.sort_dir.as_deref() {
                    Some("desc") => q.order_by_desc(&col),
                    _ => q.order_by_asc(&col),
                }
            })
            .pipe(|q| {
                let page = self.page.unwrap_or(1);
                let per_page = self.per_page.unwrap_or(20);
                q.page(page, per_page)
            })
    }
}

// 在 handler 中使用
async fn list_users(
    Query(filter): Query<UserFilter>,
    State(client): State<PgClient>,
) -> Result<Json<Vec<User>>, AppError> {
    let users = filter
        .apply(User::query())
        .find(&client)
        .await?;

    Ok(Json(users))
}
```

### 报表查询

```rust
// 复杂的报表查询组合
pub fn monthly_active_users(month: u32, year: i32) -> impl Fn(UserQuery) -> UserQuery {
    move |q| {
        let start = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
        let end = start + Months::new(1);

        q.gte("last_login_at", start)
         .lt("last_login_at", end)
         .apply(User::active())
    }
}

pub fn high_engagement() -> impl Fn(UserQuery) -> UserQuery {
    |q| q
        .gte("login_count", 10)
        .gte("total_actions", 100)
}

// 使用
let report_users = User::query()
    .apply(monthly_active_users(12, 2024))
    .apply(high_engagement())
    .count(&client)
    .await?;
```

## 实现检查清单

- [ ] 实现 `Composable` trait
- [ ] 为 `ModelQuery` 实现 `apply` 方法
- [ ] 实现 `apply_if` 方法
- [ ] 实现 `apply_some` 方法
- [ ] 实现 `apply_ok` 方法
- [ ] 实现 `pipe` 方法
- [ ] 增强 `WhereBuilder` 支持链式 AND/OR
- [ ] 实现 `where_` 闭包 API
- [ ] 实现 `or` 分支方法
- [ ] 创建 `define_scopes!` 宏
- [ ] 单元测试
- [ ] 性能基准测试
- [ ] 文档更新
