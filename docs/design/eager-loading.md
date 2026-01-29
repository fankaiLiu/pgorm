# Eager Loading

> 参考来源：Drizzle (TypeScript)、Prisma (TypeScript)、ActiveRecord (Ruby)

## 概述

Eager Loading 允许在一次查询中预加载关联数据，避免 N+1 查询问题。这是 ORM 中最重要的性能优化手段之一。

## 设计原则

- **显式声明**：必须明确指定要加载的关联，无隐藏查询
- **类型安全**：加载的关联在编译时确定，返回类型自动推断
- **灵活策略**：支持 JOIN 和多查询两种加载策略
- **可组合**：与现有查询构建器无缝集成

## N+1 问题示例

```rust
// ❌ N+1 问题：1 + N 次查询
let users = User::select_all(&client).await?;  // 1 次查询

for user in &users {
    let posts = Post::query()
        .eq("user_id", user.id)
        .find(&client).await?;  // N 次查询
    // ...
}

// ✅ Eager Loading：2 次查询
let users = User::query()
    .include::<Posts>()
    .find(&client).await?;  // 1 次查询 users，1 次查询 posts

for user in &users {
    let posts = &user.posts;  // 已加载，无额外查询
    // ...
}
```

## API 设计

### 基本用法

```rust
#[derive(Model, FromRow)]
#[orm(table = "users")]
#[orm(has_many(Post, foreign_key = "user_id", as = "posts"))]
#[orm(has_one(Profile, foreign_key = "user_id", as = "profile"))]
pub struct User {
    #[orm(id)]
    pub id: i64,
    pub name: String,
    pub email: String,
}

#[derive(Model, FromRow)]
#[orm(table = "posts")]
#[orm(belongs_to(User, foreign_key = "user_id", as = "author"))]
pub struct Post {
    #[orm(id)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub content: String,
}

// 加载用户及其帖子
let users: Vec<UserWithPosts> = User::query()
    .include::<Posts>()
    .find(&client).await?;

// 返回类型自动变为 UserWithPosts，包含 posts 字段
for user in users {
    println!("User: {}", user.name);
    for post in &user.posts {
        println!("  Post: {}", post.title);
    }
}
```

### 多关联加载

```rust
// 同时加载多个关联
let users: Vec<UserWithRelations> = User::query()
    .include::<Posts>()
    .include::<Profile>()
    .find(&client).await?;

// 返回类型包含 posts 和 profile
for user in users {
    println!("User: {}", user.name);
    if let Some(ref profile) = user.profile {
        println!("  Bio: {}", profile.bio);
    }
    println!("  Posts: {}", user.posts.len());
}
```

### 嵌套加载

```rust
// 加载帖子及其评论
let users: Vec<UserWithPostsAndComments> = User::query()
    .include::<Posts>()
    .include_nested::<Posts, Comments>()  // 帖子的评论
    .find(&client).await?;

// 或使用链式语法
let users = User::query()
    .include(Posts::include(Comments))
    .find(&client).await?;
```

### 条件加载

```rust
// 只加载活跃的帖子
let users = User::query()
    .include_where::<Posts>(|q| q.eq("status", "published"))
    .find(&client).await?;

// 限制加载数量
let users = User::query()
    .include_limit::<Posts>(5)  // 每个用户最多 5 篇帖子
    .find(&client).await?;

// 组合条件
let users = User::query()
    .include_with::<Posts>(|q| {
        q.eq("status", "published")
         .order_by_desc("created_at")
         .limit(5)
    })
    .find(&client).await?;
```

## 加载策略

### 策略 1：多查询（默认）

执行多次查询，然后在内存中组装。

```rust
// 生成的查询：
// 1. SELECT * FROM users WHERE ...
// 2. SELECT * FROM posts WHERE user_id IN ($1, $2, $3, ...)

let users = User::query()
    .include::<Posts>()
    .strategy(LoadStrategy::MultiQuery)  // 默认
    .find(&client).await?;
```

**优点**：
- 避免笛卡尔积膨胀
- 适合 has_many 关系
- 查询简单，易于优化

**缺点**：
- 多次网络往返
- 需要内存中组装

### 策略 2：JOIN 查询

使用 JOIN 一次性加载。

```rust
// 生成的查询：
// SELECT u.*, p.* FROM users u
// LEFT JOIN posts p ON p.user_id = u.id
// WHERE ...

let users = User::query()
    .include::<Posts>()
    .strategy(LoadStrategy::Join)
    .find(&client).await?;
```

**优点**：
- 单次查询
- 减少网络往返

**缺点**：
- 笛卡尔积可能导致数据膨胀
- 多个 has_many JOIN 会爆炸
- 需要去重处理

### 策略选择

```rust
pub enum LoadStrategy {
    /// 多查询策略（默认）
    /// 适合：has_many 关系、大数据量
    MultiQuery,

    /// JOIN 策略
    /// 适合：has_one/belongs_to 关系、小数据量
    Join,

    /// 自动选择
    /// has_one/belongs_to -> Join
    /// has_many -> MultiQuery
    Auto,
}
```

## 类型系统设计

### 关联类型标记

```rust
/// 关联关系标记 trait
pub trait Relation {
    /// 源模型
    type Source: Model;
    /// 目标模型
    type Target: Model;
    /// 关系类型
    type Kind: RelationKind;
    /// 外键字段
    const FOREIGN_KEY: &'static str;
}

/// 关系类型标记
pub trait RelationKind {}

pub struct HasMany;
impl RelationKind for HasMany {}

pub struct HasOne;
impl RelationKind for HasOne {}

pub struct BelongsTo;
impl RelationKind for BelongsTo {}
```

### 宏生成的关联类型

```rust
// #[orm(has_many(Post, foreign_key = "user_id", as = "posts"))]
// 生成：

pub struct Posts;

impl Relation for Posts {
    type Source = User;
    type Target = Post;
    type Kind = HasMany;
    const FOREIGN_KEY: &'static str = "user_id";
}
```

### 带关联的结果类型

```rust
/// 加载关联后的包装类型
pub struct WithRelation<M, R: Relation> {
    pub base: M,
    pub relation: RelationData<R>,
}

/// 关联数据容器
pub enum RelationData<R: Relation> {
    HasMany(Vec<R::Target>),
    HasOne(Option<R::Target>),
    BelongsTo(Option<R::Target>),
}

// 自动实现 Deref 以便访问原始字段
impl<M, R: Relation> Deref for WithRelation<M, R> {
    type Target = M;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}
```

### 类型安全的加载

```rust
impl<M: Model> ModelQuery<M> {
    /// 添加 include，返回新的查询类型
    pub fn include<R>(self) -> ModelQueryWith<M, (R,)>
    where
        R: Relation<Source = M>,
    {
        ModelQueryWith {
            base: self,
            relations: PhantomData,
        }
    }
}

impl<M: Model, Rs> ModelQueryWith<M, Rs> {
    /// 继续添加 include
    pub fn include<R>(self) -> ModelQueryWith<M, (Rs, R)>
    where
        R: Relation<Source = M>,
    {
        ModelQueryWith {
            base: self.base,
            relations: PhantomData,
        }
    }

    /// 执行查询
    pub async fn find(self, client: &impl GenericClient) -> OrmResult<Vec<LoadedModel<M, Rs>>> {
        // 根据关系类型选择策略并执行
        todo!()
    }
}
```

## 实现方案

### 多查询策略实现

```rust
async fn load_with_multi_query<M, R>(
    base_query: &ModelQuery<M>,
    client: &impl GenericClient,
) -> OrmResult<Vec<WithRelation<M, R>>>
where
    M: Model,
    R: Relation<Source = M>,
{
    // 1. 执行基础查询
    let base_records: Vec<M> = base_query.find(client).await?;

    if base_records.is_empty() {
        return Ok(vec![]);
    }

    // 2. 收集主键
    let ids: Vec<_> = base_records.iter().map(|r| r.pk()).collect();

    // 3. 批量加载关联
    let related: Vec<R::Target> = sql("SELECT * FROM ")
        .push_ident(R::Target::TABLE)
        .push(" WHERE ")
        .push_ident(R::FOREIGN_KEY)
        .push(" = ANY(")
        .push_param(&ids)
        .push(")")
        .fetch_all_as(client)
        .await?;

    // 4. 按外键分组
    let mut grouped: HashMap<_, Vec<_>> = HashMap::new();
    for record in related {
        let fk = record.get_fk::<R>();
        grouped.entry(fk).or_default().push(record);
    }

    // 5. 组装结果
    let results = base_records
        .into_iter()
        .map(|base| {
            let relation_data = grouped
                .remove(&base.pk())
                .unwrap_or_default();
            WithRelation {
                base,
                relation: RelationData::HasMany(relation_data),
            }
        })
        .collect();

    Ok(results)
}
```

### JOIN 策略实现

```rust
async fn load_with_join<M, R>(
    base_query: &ModelQuery<M>,
    client: &impl GenericClient,
) -> OrmResult<Vec<WithRelation<M, R>>>
where
    M: Model,
    R: Relation<Source = M>,
{
    // 构建 JOIN 查询
    let sql = sql("SELECT ")
        .push(M::SELECT_LIST)
        .push(", ")
        .push(R::Target::SELECT_LIST_AS("r"))
        .push(" FROM ")
        .push_ident(M::TABLE)
        .push(" LEFT JOIN ")
        .push_ident(R::Target::TABLE)
        .push(" r ON r.")
        .push_ident(R::FOREIGN_KEY)
        .push(" = ")
        .push_ident(M::TABLE)
        .push(".")
        .push_ident(M::ID);

    // 执行并去重
    let rows = sql.fetch_all(client).await?;

    // 去重并组装（使用 HashMap 按主键聚合）
    let mut result_map: IndexMap<_, WithRelation<M, R>> = IndexMap::new();

    for row in rows {
        let base = M::from_row(&row)?;
        let pk = base.pk();

        let related = R::Target::from_row_prefixed(&row, "r").ok();

        result_map
            .entry(pk)
            .or_insert_with(|| WithRelation {
                base,
                relation: RelationData::HasMany(vec![]),
            })
            .relation
            .push(related);
    }

    Ok(result_map.into_values().collect())
}
```

## 与现有功能的交互

### 与 Model 宏的交互

```rust
// 声明关联
#[derive(Model, FromRow)]
#[orm(table = "users")]
#[orm(has_many(Post, foreign_key = "user_id", as = "posts"))]
#[orm(has_one(Profile, foreign_key = "user_id", as = "profile"))]
pub struct User {
    #[orm(id)]
    pub id: i64,
    pub name: String,
}

// 宏生成：
// - Posts 关联类型
// - Profile 关联类型
// - User::query().include::<Posts>() 支持
```

### 与可组合查询的交互

```rust
// Scopes 与 include 组合
let users = User::query()
    .apply(User::active())
    .apply(User::adult())
    .include::<Posts>()
    .include_with::<Posts>(|q| q.eq("status", "published"))
    .find(&client).await?;
```

### 与 Write Graph 的区别

| 特性 | Eager Loading | Write Graph |
|------|---------------|-------------|
| 方向 | 读取 | 写入 |
| 用途 | 批量加载关联数据 | 原子写入关联数据 |
| 查询次数 | 1-N（取决于策略） | 1 事务 |
| 返回类型 | 带关联的模型 | 写入报告 |

## 高级用法

### 预定义加载配置

```rust
// 定义常用的加载配置
impl User {
    fn with_posts() -> impl Fn(UserQueryWith<()>) -> UserQueryWith<(Posts,)> {
        |q| q.include::<Posts>()
    }

    fn with_profile() -> impl Fn(UserQueryWith<()>) -> UserQueryWith<(Profile,)> {
        |q| q.include::<Profile>()
    }

    fn with_all_relations() -> impl Fn(UserQueryWith<()>) -> UserQueryWith<(Posts, Profile)> {
        |q| q.include::<Posts>().include::<Profile>()
    }
}

// 使用
let users = User::query()
    .apply(User::with_all_relations())
    .find(&client).await?;
```

### 条件嵌套加载

```rust
// 只加载已发布帖子的评论
let users = User::query()
    .include_with::<Posts>(|q| {
        q.eq("status", "published")
         .include::<Comments>()  // 嵌套
    })
    .find(&client).await?;
```

### 计数而非加载

```rust
// 只获取计数，不加载实际数据
let users = User::query()
    .include_count::<Posts>()  // 添加 posts_count 字段
    .find(&client).await?;

// 返回类型包含 posts_count: i64
for user in users {
    println!("{} has {} posts", user.name, user.posts_count);
}

// 生成的 SQL：
// SELECT u.*, (SELECT COUNT(*) FROM posts WHERE user_id = u.id) as posts_count
// FROM users u
```

## 错误处理

| 场景 | 错误类型 | 说明 |
|------|----------|------|
| 未定义的关联 | 编译错误 | `Posts is not a relation of User` |
| 外键类型不匹配 | 编译错误 | 类型系统保证 |
| 数据加载失败 | `OrmError::Query` | 运行时数据库错误 |
| 关联数据解析失败 | `OrmError::Decode` | 行映射错误 |

## 实现检查清单

- [ ] 定义 `Relation` trait
- [ ] 定义 `RelationKind` 标记 traits
- [ ] 修改 Model 宏生成关联类型
- [ ] 实现 `WithRelation` 包装类型
- [ ] 实现 `ModelQueryWith` 查询类型
- [ ] 实现 `include` 方法
- [ ] 实现 `include_with` 条件加载
- [ ] 实现 `include_limit` 限制加载
- [ ] 实现 `include_nested` 嵌套加载
- [ ] 实现 `include_count` 计数
- [ ] 实现多查询策略
- [ ] 实现 JOIN 策略
- [ ] 实现自动策略选择
- [ ] 与可组合查询集成
- [ ] 单元测试
- [ ] 性能基准测试
- [ ] 文档更新
