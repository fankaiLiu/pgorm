# 自动时间戳 (Auto Timestamps)

> 参考来源：Django ORM、ActiveRecord；以及常见的 `created_at` / `updated_at` 约定

## 概述

`pgorm` 的 `InsertModel` / `UpdateModel` 宏可以为标注了属性的字段自动填充时间戳，减少样板代码，同时保持行为显式、可预测（SQL 仍然是明确生成的）。

## 设计原则

- **显式**：只对显式标注 `#[orm(auto_now_add)]` / `#[orm(auto_now)]` 的字段生效
- **SQL-first**：仅影响生成的绑定值，不引入隐藏 trigger / 全局回调
- **可覆盖**：字段为 `Some(v)` 时使用 `v`（便于测试、回填、数据修复）
- **一致性**：一次 insert/update 调用只取一次 `now`，同一行内多个字段保持一致
- **编译期校验**：与 `skip_*` / `default` / 不支持的字段类型冲突在宏展开时报错

## 字段属性与类型

| 属性 | 适用宏 | 行为 |
|------|--------|------|
| `#[orm(auto_now_add)]` | `InsertModel` | INSERT 时：`None -> Some(now)`，`Some(v) -> Some(v)` |
| `#[orm(auto_now)]` | `UpdateModel` | UPDATE 时：该列总会出现在 `SET` 中；`None -> now`，`Some(v) -> v` |

支持的 Rust 类型（与 `tokio-postgres` 的 chrono 支持保持一致）：

- `Option<chrono::DateTime<chrono::Utc>>`（推荐，对应 Postgres `timestamptz`）
- `Option<chrono::NaiveDateTime>`（对应 Postgres `timestamp`）

> 为了避免“先填一个 Default 再被覆盖”的反直觉用法，并保持 UpdateModel 的 patch 语义，`auto_now*` 只允许用在 `Option<T>` 字段上。

## 示例

### InsertModel：`created_at` / `updated_at`

```rust
use chrono::{DateTime, Utc};
use pgorm::InsertModel;

#[derive(Debug, InsertModel)]
#[orm(table = "posts", returning = "Post")]
pub struct NewPost {
    pub title: String,
    pub content: String,

    #[orm(auto_now_add)]
    pub created_at: Option<DateTime<Utc>>,

    #[orm(auto_now_add)]
    pub updated_at: Option<DateTime<Utc>>,
}

// created_at/updated_at 自动填充（同一次 insert 使用同一个 now）
let post = NewPost {
    title: "Hello".into(),
    content: "World".into(),
    created_at: None,
    updated_at: None,
}
.insert_returning(&client)
.await?;
```

如果需要固定时间（测试/回填），直接传 `Some(...)`：

```rust
let fixed_time: DateTime<Utc> = /* ... */;

let _affected = NewPost {
    title: "Backfill".into(),
    content: "…".into(),
    created_at: Some(fixed_time),
    updated_at: Some(fixed_time),
}
.insert(&client)
.await?;
```

### UpdateModel：自动更新时间戳（也支持 touch）

```rust
use chrono::{DateTime, Utc};
use pgorm::UpdateModel;

#[derive(Debug, UpdateModel)]
#[orm(table = "posts")]
pub struct PostPatch {
    pub title: Option<String>,

    #[orm(auto_now)]
    pub updated_at: Option<DateTime<Utc>>,
}

// 常规更新：updated_at 自动设置为 now
let affected = PostPatch {
    title: Some("New title".into()),
    updated_at: None,
}
.update_by_id(&client, 1_i64)
.await?;

// touch：只更新时间戳（不会触发 “no fields to update”）
let affected = PostPatch {
    title: None,
    updated_at: None,
}
.update_by_id(&client, 1_i64)
.await?;
```

## 与现有属性/功能的交互

- 与 `#[orm(skip_insert)]` / `#[orm(skip_update)]` 冲突：编译错误（一个字段不能既“自动写入”又“永不写入”）
- 与 `#[orm(default)]` 冲突：编译错误（选择 Rust 侧填充，或选择数据库 DEFAULT）
- 与 `#[orm(column = "...")]`、graph 写入、`insert_many` / `update_by_ids`：按正常列处理；每次调用只取一次 `now`

## 数据库侧时间（替代方案）

如果你希望以数据库时间为准（避免应用机时钟漂移），推荐直接使用 Postgres 的显式机制：

- INSERT：列上设置 `DEFAULT NOW()`，并在 InsertModel 上用 `#[orm(default)]` 省略该列
- UPDATE：使用 trigger 维护 `updated_at`（或在手写 SQL 中显式 `updated_at = NOW()`）

pgorm 倾向保持 SQL 显式，因此不额外引入 `auto_now_db` 之类的变体。

## 实现摘要（宏层）

- `crates/pgorm-derive/src/insert_model.rs`：在绑定前生成 `let __pgorm_now = chrono::Utc::now();`，对 `auto_now_add` 字段做 `field.get_or_insert(__pgorm_now)`（或等价逻辑）
- `crates/pgorm-derive/src/update_model.rs`：为 `auto_now` 字段生成无条件的 `SET col = $n`，并在 `None` 时绑定 `__pgorm_now`

## 迁移指南

- 把 `created_at: Utc::now()` / `updated_at: Utc::now()` 改为 `None`，并加上 `#[orm(auto_now_add)]` / `#[orm(auto_now)]`
- 测试或回填场景：显式传 `Some(fixed_time)` 覆盖自动行为

## 错误处理

| 场景 | 错误类型 | 说明 |
|------|----------|------|
| 字段不是支持的 `Option<T>` 时间类型 | 编译错误 | `auto_now* requires Option<DateTime<Utc>> or Option<NaiveDateTime>` |
| 与 `skip_insert` / `skip_update` 冲突 | 编译错误 | 明确提示冲突 |
| 与 `default` 冲突 | 编译错误 | 明确提示冲突 |

## 实现检查清单

- [ ] 解析 `#[orm(auto_now_add)]`（InsertModel 字段属性）
- [ ] 解析 `#[orm(auto_now)]`（UpdateModel 字段属性）
- [ ] 编译期校验字段类型为 `Option<DateTime<Utc>>` / `Option<NaiveDateTime>`
- [ ] 编译期冲突检测（`skip_*` / `default`）
- [ ] insert：单次调用取一次 `now`
- [ ] insert_many / insert_many_returning：批量同一次调用复用 `now`
- [ ] update_by_id / update_by_ids：单次调用取一次 `now`，并保证 `auto_now` 字段总参与 SET
- [ ] write graph：按每个步骤的 insert/update 规则处理
- [ ] 文档/示例更新
