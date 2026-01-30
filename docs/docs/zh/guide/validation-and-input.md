# 输入校验与 Input：`#[orm(input)]`

当你要从“不可信输入”（JSON 请求体、表单、消息队列）构造写模型时，常见痛点是：

- 需要一个 `Input` 结构体用于反序列化（字段可选、能表达 patch）
- 需要集中做校验（必填、长度、范围、email/url/uuid…）
- 校验失败希望得到“可序列化的错误列表”（便于 API 返回）

pgorm 提供 `#[orm(input)]`：在 `InsertModel` / `UpdateModel` 上开启后，会自动生成对应的 `*Input` 类型，并生成校验与转换方法。

## 0) 前置：需要启用的 feature

- `derive`：因为这是派生宏能力  
- `validate`：如果你用到了 `#[orm(email)]` / `#[orm(url)]` / `#[orm(regex=...)]` 等校验器  

默认 features 已包含它们；如果你关闭了默认特性，需要手动开启：

```toml
[dependencies]
pgorm = { version = "0.1.1", features = ["derive", "validate"] }
```

## 1) Insert：生成 `NewXxxInput` + validate + try_into_model

```rust
use pgorm::InsertModel;

#[derive(Debug, InsertModel)]
#[orm(table = "users", returning = "User")]
#[orm(input)] // 生成 NewUserInput
struct NewUser {
    #[orm(len = "2..=100")]
    name: String,

    #[orm(email)]
    email: String,

    #[orm(range = "0..=150")]
    age: Option<i32>,

    // 让 Input 接收 String，校验并解析成 uuid::Uuid（更友好地返回 ValidationErrors）
    #[orm(uuid, input_as = "String")]
    external_id: uuid::Uuid,

    #[orm(url)]
    homepage: Option<String>,
}
```

生成的 `NewUserInput` 会：

- `derive(serde::Deserialize)`：可直接反序列化 JSON
- `validate()`：返回 `pgorm::changeset::ValidationErrors`
- `try_into_model()`：校验通过则转换成 `NewUser`，否则返回 `ValidationErrors`

用法示例：

```rust
let input: NewUserInput = serde_json::from_str(json_body)?;
let new_user: NewUser = input.try_into_model()?;
let user: User = new_user.insert_returning(&client).await?;
```

> 完整可运行示例见 `crates/pgorm/examples/changeset`。

## 2) Update：生成 `XxxPatchInput` + try_into_patch（三态语义）

对 `UpdateModel` 来说，Input 更重要：它能表达“字段缺失 vs 字段为 null”。

```rust
use pgorm::UpdateModel;

#[derive(Debug, UpdateModel)]
#[orm(table = "users", model = "User", returning = "User")]
#[orm(input)] // 生成 UserPatchInput
struct UserPatch {
    #[orm(len = "2..=100")]
    name: Option<String>, // None = 缺失/跳过；Some(v) = 更新

    #[orm(email)]
    email: Option<String>,

    #[orm(url)]
    homepage: Option<Option<String>>, // None = 缺失/跳过；Some(None)=设 NULL；Some(Some(v))=设值
}
```

用法示例：

```rust
let patch_input: UserPatchInput = serde_json::from_str(r#"{"email":"a@b.com","homepage":null}"#)?;
let patch: UserPatch = patch_input.try_into_patch()?;
let updated: User = patch.update_by_id_returning(&client, user_id).await?;
```

## 3) 支持的验证属性（常用）

| 属性 | 说明 |
|------|------|
| `#[orm(len = "min..=max")]` | 字符串长度（按 `str::len()`） |
| `#[orm(range = "min..=max")]` | 数值范围（支持表达式） |
| `#[orm(email)]` | 邮箱格式 |
| `#[orm(url)]` | URL 格式 |
| `#[orm(uuid)]` | UUID 格式 |
| `#[orm(regex = "pattern")]` | 正则匹配 |
| `#[orm(one_of = "a\|b\|c")]` | 必须是枚举值之一 |
| `#[orm(custom = "path::to::fn")]` | 自定义校验函数 |

### `input_as` 目前的边界

`input_as` 目前只对以下两类字段生效：

- `uuid::Uuid`
- `url::Url`（需要你在应用里显式依赖 `url` crate）

用法：让 Input 用字符串接收，然后在 `try_into_model/try_into_patch` 阶段做解析并返回 `ValidationErrors`（而不是 serde 的反序列化错误）。

同时，`input_as` **不支持** `Option<Option<T>>` 字段（因为它会和三态语义冲突）。

## 4) 自定义校验函数：`custom`

`custom` 会把整个字段值传给你的函数（包括 Option），你可以返回自定义错误信息：

```rust
fn validate_slug(v: &Option<String>) -> Result<(), String> {
    if let Some(s) = v.as_deref() {
        if s.contains(' ') {
            return Err("must not contain spaces".to_string());
        }
    }
    Ok(())
}
```

## 下一步

- 如果你已经用上 `Input`，通常下一步是启用运行时兜底：[`PgClient / CheckedClient`](/zh/guide/runtime-sql-check)
