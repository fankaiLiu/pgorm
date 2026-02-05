# 输入验证

当从不可信输入（JSON API 请求、表单提交、消息队列）构建写入模型时，你需要：

- 专用的 `Input` 结构体用于反序列化（具备正确的 patch/三态语义）
- 集中的字段验证（长度、范围、邮箱、URL、UUID 等）
- 可以直接从 API 返回的机器友好的错误格式

`#[orm(input)]` 属性为 `InsertModel` 和 `UpdateModel` 自动生成以上所有内容。

## 前置条件

必须启用 `derive` 和 `validate` feature（两者都包含在默认 feature 中）：

```toml
[dependencies]
pgorm = "0.2.0"

# 如果你禁用了默认 feature：
# pgorm = { version = "0.2.0", features = ["derive", "validate"] }
```

## 1. 使用 `#[orm(input)]` 生成 Input 结构体

### 用于 InsertModel

在 `InsertModel` 上添加 `#[orm(input)]` 会生成一个 `*Input` 结构体（例如 `NewUser` 生成 `NewUserInput`）：

```rust
use pgorm::{FromRow, InsertModel, Model};

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
    age: Option<i32>,
    external_id: uuid::Uuid,
    homepage: Option<String>,
}

#[derive(Debug, InsertModel)]
#[orm(table = "users", returning = "User")]
#[orm(input)]  // 生成 NewUserInput
struct NewUser {
    #[orm(len = "2..=100")]
    name: String,

    #[orm(email)]
    email: String,

    #[orm(range = "0..=150")]
    age: Option<i32>,

    #[orm(uuid, input_as = "String")]
    external_id: uuid::Uuid,

    #[orm(url)]
    homepage: Option<String>,
}
```

生成的 `NewUserInput` 结构体：

- 派生了 `serde::Deserialize`，可以直接解析 JSON
- 拥有 `validate()` 方法，返回 `ValidationErrors`
- 拥有 `try_into_model()` 方法，返回 `Result<NewUser, ValidationErrors>`

### 用于 UpdateModel

在 `UpdateModel` 上添加 `#[orm(input)]` 会生成一个具有三态语义的 `*Input` 结构体，用于 patch 操作：

```rust
use pgorm::UpdateModel;

#[derive(Debug, UpdateModel)]
#[orm(table = "users", model = "User", returning = "User")]
#[orm(input)]  // 生成 UserPatchInput
struct UserPatch {
    #[orm(len = "2..=100")]
    name: Option<String>,           // None = 跳过, Some(v) = 更新

    #[orm(email)]
    email: Option<String>,

    #[orm(url)]
    homepage: Option<Option<String>>, // None = 跳过, Some(None) = 设为 NULL, Some(Some(v)) = 设为值
}
```

生成的 `UserPatchInput` 拥有 `try_into_patch()` 方法，返回 `Result<UserPatch, ValidationErrors>`。

## 2. 验证属性

| 属性 | 说明 |
|------|------|
| `#[orm(len = "min..=max")]` | 字符串长度验证 |
| `#[orm(range = "min..=max")]` | 数值范围验证 |
| `#[orm(email)]` | 邮箱格式验证 |
| `#[orm(url)]` | URL 格式验证 |
| `#[orm(uuid)]` | UUID 格式验证 |
| `#[orm(ip)]` | IP 地址格式验证 |
| `#[orm(regex = "pattern")]` | 自定义正则表达式匹配 |
| `#[orm(one_of = "a\|b\|c")]` | 值必须是列出的选项之一 |
| `#[orm(custom = "path::to::fn")]` | 自定义验证函数 |
| `#[orm(input_as = "Type")]` | 在 Input 结构体中接受不同的类型 |

属性可以组合使用在同一个字段上：

```rust
#[orm(uuid, input_as = "String")]
external_id: uuid::Uuid,
```

## 3. `input_as` 类型边界

`input_as` 告诉 pgorm 在生成的 Input 结构体中接受不同的类型，验证后再将其转换为模型的字段类型。当传输格式是字符串但模型使用的是解析后的类型时，这非常有用。

目前支持的转换：

- `String` 到 `uuid::Uuid`
- `String` 到 `std::net::IpAddr`
- `String` 到 `url::Url`（需要将 `url` crate 作为依赖）

```rust
// 模型中：字段类型是 uuid::Uuid
// Input 中：字段类型是 String
// 验证：检查 UUID 格式
// 转换：解析 String -> uuid::Uuid，失败时返回 ValidationErrors
#[orm(uuid, input_as = "String")]
external_id: uuid::Uuid,
```

注意：`input_as` **不支持** `Option<Option<T>>` 类型的字段（与三态语义冲突）。

## 4. 工作流：反序列化、验证、转换

处理输入的典型流程：

### 插入流程

```rust
use pgorm::changeset::ValidationErrors;

// 1. 从不可信输入反序列化
let input: NewUserInput = serde_json::from_str(json_body)?;

// 2. 一次性验证所有字段
let errors = input.validate();
if !errors.is_empty() {
    // 将错误返回给客户端
    return Err(serde_json::to_string(&errors)?);
}

// 3. 转换为模型（同时验证并转换 input_as 类型）
let new_user: NewUser = input.try_into_model()?;

// 4. 插入数据库
let user: User = new_user.insert_returning(&client).await?;
```

你可以跳过显式的 `validate()` 步骤 -- `try_into_model()` 内部会进行验证，失败时返回 `ValidationErrors`：

```rust
let input: NewUserInput = serde_json::from_str(json_body)?;
let new_user: NewUser = match input.try_into_model() {
    Ok(v) => v,
    Err(errs) => {
        // errs 是 ValidationErrors
        return Err(serde_json::to_string(&errs)?);
    }
};
```

### 更新（Patch）流程

```rust
// JSON 包含部分字段：{"email": "bob@example.com", "homepage": null}
let patch_input: UserPatchInput = serde_json::from_str(patch_json)?;
let patch: UserPatch = patch_input.try_into_patch()?;
let updated: User = patch.update_by_id_returning(&client, user_id).await?;
```

上面的 JSON 中：
- `email` 有值 -- 将被更新
- `homepage` 显式为 `null` -- 将在数据库中设为 NULL
- `name` 缺失 -- 将被跳过（不变）

## 5. 自定义验证器

使用 `#[orm(custom = "path::to::fn")]` 实现内置属性无法表达的验证逻辑。函数接收字段值（包括 Option 包装），返回 `Result<(), String>`：

```rust
fn validate_slug(v: &Option<String>) -> Result<(), String> {
    if let Some(s) = v.as_deref() {
        if s.contains(' ') {
            return Err("must not contain spaces".to_string());
        }
    }
    Ok(())
}

#[derive(Debug, InsertModel)]
#[orm(table = "posts", returning = "Post")]
#[orm(input)]
struct NewPost {
    #[orm(custom = "validate_slug")]
    slug: Option<String>,

    title: String,
}
```

## 6. 错误响应格式

`ValidationErrors` 实现了 `serde::Serialize`，因此你可以直接将其作为 JSON 响应从 API 返回。格式为字段名到错误消息的映射：

```rust
use pgorm::changeset::ValidationErrors;

let input: NewUserInput = serde_json::from_str(r#"
    {
        "name": "A",
        "email": "not-an-email",
        "age": 200,
        "external_id": "not-a-uuid",
        "homepage": "not-a-url"
    }
"#)?;

let errors = input.validate();
if !errors.is_empty() {
    // 序列化为 JSON 用于 API 响应
    let json = serde_json::to_string_pretty(&errors)?;
    println!("{json}");
}
```

示例输出：

```json
{
  "name": ["length must be between 2 and 100"],
  "email": ["invalid email format"],
  "age": ["must be between 0 and 150"],
  "external_id": ["invalid UUID format"],
  "homepage": ["invalid URL format"]
}
```

## 可运行示例

参见 `crates/pgorm/examples/changeset/main.rs`，包含插入和更新验证流程的完整示例。

## 下一步

- 下一章：[数据库迁移](/zh/guide/migrations)
