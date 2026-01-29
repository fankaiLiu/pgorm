# 自动时间戳 (Auto Timestamps)

> 参考来源：GORM (Go)、Django ORM、ActiveRecord (Ruby)

## 概述

自动时间戳功能在插入和更新时自动管理 `created_at` 和 `updated_at` 字段，无需手动设置。

## 目标

- 零样板代码：不需要每次插入/更新时手动设置时间
- 编译时确定：行为在宏展开时确定，无运行时开销
- 显式声明：通过属性明确标注，行为可预测
- 与现有 `InsertModel` / `UpdateModel` 无缝集成

## API 设计

### 基本用法

```rust
#[derive(InsertModel)]
#[orm(table = "posts")]
pub struct NewPost {
    pub title: String,
    pub content: String,

    #[orm(auto_now_add)]  // INSERT 时自动设置为当前时间
    pub created_at: DateTime<Utc>,

    #[orm(auto_now_add)]  // INSERT 时也设置
    pub updated_at: DateTime<Utc>,
}

#[derive(UpdateModel)]
#[orm(table = "posts")]
pub struct UpdatePost {
    pub title: Option<String>,
    pub content: Option<String>,

    #[orm(auto_now)]  // UPDATE 时自动设置为当前时间
    pub updated_at: DateTime<Utc>,
}
```

### 使用示例

```rust
// 插入 - created_at 和 updated_at 自动填充
let post = NewPost {
    title: "Hello".to_string(),
    content: "World".to_string(),
    created_at: Default::default(),  // 会被覆盖
    updated_at: Default::default(),  // 会被覆盖
};
NewPost::insert(&post, &client).await?;

// 更新 - updated_at 自动填充
let patch = UpdatePost {
    title: Some("New Title".to_string()),
    content: None,
    updated_at: Default::default(),  // 会被覆盖
};
UpdatePost::update_by_id(1, &patch, &client).await?;
```

### 使用 Option 类型（推荐）

```rust
#[derive(InsertModel)]
#[orm(table = "posts")]
pub struct NewPost {
    pub title: String,

    #[orm(auto_now_add)]
    pub created_at: Option<DateTime<Utc>>,  // Option 类型，用户无需初始化

    #[orm(auto_now_add)]
    pub updated_at: Option<DateTime<Utc>>,
}

// 更简洁的使用
let post = NewPost {
    title: "Hello".to_string(),
    created_at: None,  // 自动填充
    updated_at: None,  // 自动填充
};
```

## 实现方案

### 方案 A：Rust 侧填充（推荐）

在调用 `insert()` / `update()` 之前，宏生成的代码自动设置时间戳。

**优点**：
- 时间戳在应用层生成，调试时可见
- 不依赖数据库函数
- 支持 mock 测试（可以控制时间）

**缺点**：
- 应用服务器时钟可能不同步
- 批量操作时每行时间戳略有不同

**生成的代码**：

```rust
impl NewPost {
    pub async fn insert(
        input: &NewPost,
        client: &impl GenericClient,
    ) -> OrmResult<()> {
        // 自动时间戳处理
        let now = chrono::Utc::now();
        let created_at = now;
        let updated_at = now;

        let sql = "INSERT INTO posts (title, content, created_at, updated_at) \
                   VALUES ($1, $2, $3, $4)";
        client.execute(sql, &[&input.title, &input.content, &created_at, &updated_at]).await?;
        Ok(())
    }
}
```

### 方案 B：数据库侧填充

使用 `DEFAULT now()` 或在 SQL 中直接使用 `NOW()`。

**优点**：
- 时间戳一致（来自数据库服务器）
- 批量操作时所有行时间戳相同

**缺点**：
- 需要表定义配合（DEFAULT 约束）
- 返回值需要 RETURNING 才能获取实际时间

**生成的代码**：

```rust
impl NewPost {
    pub async fn insert(
        input: &NewPost,
        client: &impl GenericClient,
    ) -> OrmResult<()> {
        // created_at 和 updated_at 使用 NOW()
        let sql = "INSERT INTO posts (title, content, created_at, updated_at) \
                   VALUES ($1, $2, NOW(), NOW())";
        client.execute(sql, &[&input.title, &input.content]).await?;
        Ok(())
    }
}
```

### 方案选择

**推荐方案 A（Rust 侧填充）**，原因：
1. 与 pgorm 的显式哲学一致
2. 不依赖数据库 schema 配置
3. 便于测试和调试
4. 可以提供 `#[orm(auto_now_db)]` 作为方案 B 的可选变体

## 属性定义

| 属性 | 适用宏 | 说明 |
|------|--------|------|
| `#[orm(auto_now_add)]` | InsertModel | INSERT 时自动设置为当前时间 |
| `#[orm(auto_now)]` | UpdateModel | UPDATE 时自动设置为当前时间 |
| `#[orm(auto_now_db)]` | 两者 | 使用数据库 `NOW()` 而非 Rust 时间 |

## 支持的类型

```rust
// 推荐
DateTime<Utc>
Option<DateTime<Utc>>

// 也支持
DateTime<Local>
Option<DateTime<Local>>
NaiveDateTime
Option<NaiveDateTime>

// 时间提供者 trait（用于测试）
pub trait TimeProvider: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemTime;
impl TimeProvider for SystemTime {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
```

## 与其他功能的交互

### 与 `skip_insert` / `skip_update` 的关系

```rust
#[derive(InsertModel)]
pub struct NewPost {
    #[orm(auto_now_add)]
    pub created_at: DateTime<Utc>,  // 自动填充，不能跳过

    #[orm(skip_insert)]  // 错误！与 auto_now_add 冲突
    pub updated_at: DateTime<Utc>,
}
```

宏应在编译时检测并报错：`auto_now_add and skip_insert cannot be used together`

### 与 `default` 的关系

```rust
#[derive(InsertModel)]
pub struct NewPost {
    #[orm(auto_now_add)]  // 优先级高于 default
    #[orm(default)]       // 忽略
    pub created_at: DateTime<Utc>,
}
```

`auto_now_add` 优先级高于 `default`，当两者同时存在时，使用 `auto_now_add`。

### 与 Write Graph 的交互

```rust
#[derive(InsertModel)]
#[orm(has_many(Comment, field = "comments", fk_field = "post_id"))]
pub struct NewPost {
    pub title: String,

    #[orm(auto_now_add)]
    pub created_at: DateTime<Utc>,
}

#[derive(InsertModel)]
pub struct NewComment {
    pub content: String,
    pub post_id: Option<i64>,

    #[orm(auto_now_add)]
    pub created_at: DateTime<Utc>,  // 子记录也自动填充
}
```

Graph 写入时，每个模型的 `auto_now_add` 字段独立处理。

## 宏实现细节

### InsertModel 宏修改

```rust
// 在 insert_model.rs 中添加

struct AutoTimestampField {
    field_name: Ident,
    is_option: bool,
    use_db_now: bool,
}

fn extract_auto_timestamps(fields: &[Field]) -> Vec<AutoTimestampField> {
    fields.iter()
        .filter_map(|f| {
            let attrs = parse_field_attrs(f);
            if attrs.auto_now_add {
                Some(AutoTimestampField {
                    field_name: f.ident.clone().unwrap(),
                    is_option: is_option_type(&f.ty),
                    use_db_now: attrs.auto_now_db,
                })
            } else {
                None
            }
        })
        .collect()
}

fn generate_insert_method(/* ... */) -> TokenStream {
    let auto_ts_fields = extract_auto_timestamps(&fields);

    // 生成时间戳设置代码
    let ts_setup = if auto_ts_fields.is_empty() {
        quote! {}
    } else {
        let assignments: Vec<_> = auto_ts_fields.iter()
            .filter(|f| !f.use_db_now)
            .map(|f| {
                let name = &f.field_name;
                quote! { let #name = ::chrono::Utc::now(); }
            })
            .collect();
        quote! {
            let __pgorm_now = ::chrono::Utc::now();
            #(#assignments)*
        }
    };

    // 生成 SQL 和参数绑定...
}
```

### 测试支持

```rust
#[cfg(test)]
mod tests {
    use pgorm::testing::MockTimeProvider;

    #[tokio::test]
    async fn test_auto_timestamps() {
        let fixed_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let time_provider = MockTimeProvider::new(fixed_time);

        // 使用 mock 时间提供者...
    }
}
```

## 迁移指南

### 从手动时间戳迁移

**之前**：
```rust
let post = NewPost {
    title: "Hello".to_string(),
    created_at: Utc::now(),
    updated_at: Utc::now(),
};
```

**之后**：
```rust
// 添加 #[orm(auto_now_add)] 属性后
let post = NewPost {
    title: "Hello".to_string(),
    created_at: None,  // 或 Default::default()
    updated_at: None,
};
```

## 错误处理

| 场景 | 错误类型 | 说明 |
|------|----------|------|
| 不支持的字段类型 | 编译错误 | `auto_now_add requires DateTime type` |
| 与 skip_insert 冲突 | 编译错误 | `auto_now_add and skip_insert cannot be used together` |
| 数据库时钟问题 | 运行时 | 不影响 ORM，由用户自行处理 |

## 实现检查清单

- [ ] 解析 `#[orm(auto_now_add)]` 属性
- [ ] 解析 `#[orm(auto_now)]` 属性
- [ ] 解析 `#[orm(auto_now_db)]` 属性
- [ ] 支持 `DateTime<Utc>` 类型
- [ ] 支持 `Option<DateTime<Utc>>` 类型
- [ ] 生成 Rust 侧时间戳代码
- [ ] 生成数据库侧时间戳代码（可选）
- [ ] 编译时冲突检测
- [ ] 与 `insert_many` 集成
- [ ] 与 Write Graph 集成
- [ ] 单元测试
- [ ] 文档更新
