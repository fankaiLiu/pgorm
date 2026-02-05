# 字段修饰符（只读字段、生命周期回调）设计与计划

状态：Draft
相关代码：`crates/pgorm-derive/` / `crates/pgorm/src/model.rs`
最后更新：2026-02-05

## 背景

在实际业务中，某些字段有特殊的写入规则：

1. **只读字段**：`created_at` 只在插入时设置，更新时不可修改
2. **自动更新字段**：`updated_at` 每次更新时自动设置为当前时间
3. **计算字段**：`full_name` 由 `first_name + last_name` 计算得出
4. **生命周期回调**：在 insert/update 前后执行自定义逻辑

当前 `pgorm` 已支持 `#[orm(skip_insert)]`、`#[orm(skip_update)]`、`#[orm(auto_now)]`、`#[orm(auto_now_add)]`，但：
- 缺少显式的"只读"语义
- 回调机制局限于 `write_graph` 的 `before_insert`/`after_insert`

## 目标 / 非目标

### 目标

1. **只读字段**：`#[orm(read_only)]` 标记插入后不可更新的字段。
2. **不可变字段**：`#[orm(immutable)]` 标记从不写入数据库的字段（派生值）。
3. **生命周期回调**：统一的 `before_save`/`after_save` 回调机制。
4. **字段级验证回调**：在写入前对单个字段进行转换或验证。

### 非目标

- 数据库触发器的替代（复杂逻辑应在数据库层）。
- 异步回调（保持简单的同步模型）。
- 跨表的回调编排（使用 `write_graph`）。

---

## 一、只读字段（Read-Only Fields）

### 1.1 语义

`#[orm(read_only)]` 标记的字段：
- **InsertModel**：正常写入
- **UpdateModel**：自动跳过（不出现在 SET 子句中）
- **如果用户尝试更新**：编译时警告或运行时忽略

### 1.2 使用示例

```rust,ignore
#[derive(Model, InsertModel, UpdateModel, FromRow)]
#[orm(table = "users")]
pub struct User {
    pub id: i64,

    pub email: String,

    pub name: String,

    #[orm(read_only)]  // 插入后不可修改
    pub created_at: DateTime<Utc>,

    #[orm(auto_now)]  // 每次更新自动设置
    pub updated_at: DateTime<Utc>,

    #[orm(read_only)]  // 用户类型创建后不可改变
    pub user_type: String,
}

// InsertModel 正常包含 created_at
let new_user = NewUser {
    email: "alice@example.com".into(),
    name: "Alice".into(),
    created_at: Utc::now(),
    user_type: "premium".into(),
};
new_user.insert(&client).await?;

// UpdateModel 自动排除 created_at 和 user_type
let patch = UserPatch {
    id: 1,
    email: Some("alice.new@example.com".into()),
    name: Some("Alice Smith".into()),
    // created_at 和 user_type 不在 patch 中
};
patch.update_by_id(&client).await?;
// 生成 SQL: UPDATE users SET email = $1, name = $2, updated_at = NOW() WHERE id = $3
```

### 1.3 与现有属性的关系

| 属性 | InsertModel | UpdateModel | 说明 |
|------|-------------|-------------|------|
| `skip_insert` | 跳过 | 正常 | 如数据库默认值 |
| `skip_update` | 正常 | 跳过 | 如只读计数器 |
| `read_only` | 正常 | 跳过 | **新增**：语义更清晰 |
| `auto_now` | 跳过 | 自动设置 NOW() | |
| `auto_now_add` | 自动设置 NOW() | 跳过 | |

> 注：`read_only` 等同于 `skip_update`，但语义更明确。

---

## 二、不可变字段（Immutable / Derived Fields）

### 2.1 语义

`#[orm(immutable)]` 标记的字段：
- **InsertModel**：跳过
- **UpdateModel**：跳过
- **FromRow**：正常映射（从数据库读取）

适用于：数据库计算列、生成列（generated columns）、视图派生值。

### 2.2 使用示例

```rust,ignore
#[derive(Model, InsertModel, UpdateModel, FromRow)]
#[orm(table = "users")]
pub struct User {
    pub id: i64,
    pub first_name: String,
    pub last_name: String,

    #[orm(immutable)]  // 数据库 GENERATED ALWAYS AS 列
    pub full_name: String,

    #[orm(immutable)]  // 视图派生字段
    pub order_count: i64,
}

// 数据库定义
// CREATE TABLE users (
//     id BIGSERIAL PRIMARY KEY,
//     first_name TEXT NOT NULL,
//     last_name TEXT NOT NULL,
//     full_name TEXT GENERATED ALWAYS AS (first_name || ' ' || last_name) STORED
// );

// InsertModel 不包含 full_name 和 order_count
let new_user = NewUser {
    first_name: "Alice".into(),
    last_name: "Smith".into(),
};
new_user.insert(&client).await?;

// 读取时包含 full_name
let user: User = pgorm::query("SELECT * FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one(&client)
    .await?;

assert_eq!(user.full_name, "Alice Smith");
```

---

## 三、生命周期回调

### 3.1 回调 Trait

```rust,ignore
/// 模型生命周期回调
pub trait ModelCallbacks: Sized {
    /// 插入前调用
    fn before_insert(&mut self) -> OrmResult<()> { Ok(()) }

    /// 插入后调用（带返回的记录）
    fn after_insert(&mut self, _inserted: &Self) -> OrmResult<()> { Ok(()) }

    /// 更新前调用
    fn before_update(&mut self) -> OrmResult<()> { Ok(()) }

    /// 更新后调用
    fn after_update(&mut self) -> OrmResult<()> { Ok(()) }

    /// 删除前调用
    fn before_delete(&self) -> OrmResult<()> { Ok(()) }

    /// 删除后调用
    fn after_delete(&self) -> OrmResult<()> { Ok(()) }

    /// 保存前调用（insert 或 update）
    fn before_save(&mut self) -> OrmResult<()> { Ok(()) }

    /// 保存后调用
    fn after_save(&mut self) -> OrmResult<()> { Ok(()) }
}
```

### 3.2 使用示例

```rust,ignore
#[derive(Model, InsertModel, UpdateModel, FromRow)]
#[orm(table = "posts")]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub slug: String,
    pub word_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ModelCallbacks for NewPost {
    fn before_insert(&mut self) -> OrmResult<()> {
        // 自动生成 slug
        self.slug = slugify(&self.title);
        // 计算字数
        self.word_count = self.content.split_whitespace().count() as i32;
        Ok(())
    }
}

impl ModelCallbacks for PostPatch {
    fn before_update(&mut self) -> OrmResult<()> {
        // 如果 title 变化，重新生成 slug
        if let Some(ref title) = self.title {
            self.slug = Some(slugify(title));
        }
        // 如果 content 变化，重新计算字数
        if let Some(ref content) = self.content {
            self.word_count = Some(content.split_whitespace().count() as i32);
        }
        Ok(())
    }
}

// 使用
let mut new_post = NewPost {
    title: "Hello World".into(),
    content: "This is my first post...".into(),
    slug: String::new(),  // 会被 before_insert 填充
    word_count: 0,        // 会被 before_insert 计算
};

// insert() 会自动调用 before_insert
new_post.insert(&client).await?;
```

### 3.3 回调调用时机

| 方法 | 回调顺序 |
|------|----------|
| `insert()` | `before_save` -> `before_insert` -> SQL -> `after_insert` -> `after_save` |
| `update_by_id()` | `before_save` -> `before_update` -> SQL -> `after_update` -> `after_save` |
| `delete_by_id()` | `before_delete` -> SQL -> `after_delete` |

### 3.4 回调中止执行

```rust,ignore
impl ModelCallbacks for NewPost {
    fn before_insert(&mut self) -> OrmResult<()> {
        if self.title.is_empty() {
            return Err(OrmError::ValidationFailed("title cannot be empty".into()));
        }
        Ok(())
    }
}

// 如果 before_insert 返回 Err，insert 不会执行
let result = empty_post.insert(&client).await;
assert!(result.is_err());
```

---

## 四、字段级回调（Field Transformers）

### 4.1 属性语法

```rust,ignore
#[derive(InsertModel)]
#[orm(table = "users")]
pub struct NewUser {
    #[orm(transform = "trim")]
    pub name: String,

    #[orm(transform = "lowercase")]
    pub email: String,

    #[orm(transform = "hash_password")]
    pub password: String,
}
```

### 4.2 内置转换器

| 名称 | 作用 |
|------|------|
| `trim` | 去除首尾空白 |
| `lowercase` | 转小写 |
| `uppercase` | 转大写 |
| `normalize_unicode` | Unicode 规范化 |

### 4.3 自定义转换器

```rust,ignore
// 注册自定义转换器
pgorm::register_transformer!("hash_password", |s: String| {
    bcrypt::hash(s, bcrypt::DEFAULT_COST).map_err(|e| OrmError::TransformFailed(e.to_string()))
});

// 或通过函数
#[derive(InsertModel)]
pub struct NewUser {
    #[orm(transform = hash_password)]  // 函数引用
    pub password: String,
}

fn hash_password(s: String) -> OrmResult<String> {
    bcrypt::hash(s, bcrypt::DEFAULT_COST)
        .map_err(|e| OrmError::TransformFailed(e.to_string()))
}
```

---

## API 汇总

### 字段属性

| 属性 | InsertModel | UpdateModel | 说明 |
|------|-------------|-------------|------|
| `read_only` | 包含 | 跳过 | 插入后只读 |
| `immutable` | 跳过 | 跳过 | 完全只读（数据库计算） |
| `transform = "..."` | 写入前转换 | 写入前转换 | 字段转换器 |

### Trait

```rust,ignore
pub trait ModelCallbacks {
    fn before_insert(&mut self) -> OrmResult<()>;
    fn after_insert(&mut self, inserted: &Self) -> OrmResult<()>;
    fn before_update(&mut self) -> OrmResult<()>;
    fn after_update(&mut self) -> OrmResult<()>;
    fn before_delete(&self) -> OrmResult<()>;
    fn after_delete(&self) -> OrmResult<()>;
    fn before_save(&mut self) -> OrmResult<()>;
    fn after_save(&mut self) -> OrmResult<()>;
}
```

---

## 关键取舍

| 选项 | 优点 | 缺点 | 决策 |
|------|------|------|------|
| Trait vs 属性回调 | 类型安全 | 需要实现 Trait | **Trait** |
| 同步 vs 异步回调 | 简单 | 不能做 I/O | **同步，复杂逻辑用事务** |
| 编译时 vs 运行时检查 | 更早发现错误 | 实现复杂 | **运行时，编译时警告** |

---

## 与现有功能的关系

- **write_graph**：已有 `before_insert`/`after_insert` 属性，保持兼容。
- **validate**：回调在验证之后、写入之前执行。
- **事务**：回调在事务内执行，错误会触发回滚。

---

## 兼容性与迁移

- `read_only` 是 `skip_update` 的别名，现有代码无需修改。
- `immutable` 是新增属性。
- `ModelCallbacks` Trait 是可选实现，不影响现有 Model。

---

## 里程碑 / TODO

### M1（只读字段）

- [ ] `#[orm(read_only)]` 属性解析
- [ ] `#[orm(immutable)]` 属性解析
- [ ] UpdateModel 代码生成更新
- [ ] 单元测试

### M2（生命周期回调）

- [ ] `ModelCallbacks` Trait 定义
- [ ] InsertModel/UpdateModel 集成回调
- [ ] 回调调用顺序实现
- [ ] 集成测试

### M3（字段转换器）

- [ ] `#[orm(transform = "...")]` 属性解析
- [ ] 内置转换器（trim, lowercase, uppercase）
- [ ] 自定义转换器注册机制
- [ ] 单元测试

### M4（文档与示例）

- [ ] `examples/field_modifiers`
- [ ] `examples/model_callbacks`
- [ ] 中英文文档

---

## Open Questions

1. `immutable` 字段在 `insert_returning` 时是否包含在 RETURNING 中？（建议包含）
2. 回调是否支持 `async`？（建议 M1 不支持，后续考虑）
3. `transform` 失败是返回错误还是 panic？（建议返回 `OrmResult<T>`）
4. 是否提供 `#[orm(readonly_after = "some_condition")]` 条件只读？（建议不提供，过于复杂）
