# Changeset 输入与验证（pgorm）

- 状态：MVP 已实现（仍可迭代）
- 目标版本：0.1.x
- 最后更新：2026-01-29

> 核心取舍：保持 `pgorm` 的 **SQL-first / 最小魔法 / 显式可预测**。验证不隐式介入 `insert()` / `update_*()`，由用户显式调用。

## 这个方向靠谱吗？

结论：**靠谱，且非常适合 pgorm** —— 把验证“挂在” `InsertModel` / `UpdateModel` 的派生宏上，生成一个 **全字段可缺省（Option）** 的 Input struct：

- 既能接 `serde` 反序列化的 payload，也能手工构造；
- 所有校验都在 typed struct 上完成，不引入动态 Value 容器；
- 宏生成 `validate()` / `try_into_model()` / `try_into_patch()`，减少样板代码；
- 最关键：**如果字段本身已经是 `Option<...>`，Input 不再额外套一层 Option**（避免 `Option<Option<T>>` 变成 `Option<Option<Option<T>>>`）。

## 模块与 feature

- `pgorm::changeset`（默认可用）：只放错误类型，**不引入隐式行为**。
- `pgorm::validate`（feature：`validate`，默认开启）：内置校验实现（email/regex/url/uuid 等，依赖 `regex`/`url`）。

## 核心类型（运行时）

```rust
use serde::Serialize;

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationCode {
    Required,
    Len,
    Range,
    Email,
    Regex,
    Url,
    Uuid,
    OneOf,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationError {
    pub field: String,
    pub code: ValidationCode,
    pub message: String,
    pub metadata: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ValidationErrors {
    pub items: Vec<ValidationError>,
}
```

设计要点：

- `field` 用字符串：对 JSON API 更友好（直接当 key），也支持未来扩展成路径（如 `items[3].qty`）。
- `code` 用枚举：便于 i18n / 客户端按 code 渲染。
- `metadata` 先保留接口（v1 主要用于自定义校验；内置规则后续可以补 min/max 等）。

## 宏生成的验证模型（推荐用法）

在 `#[derive(InsertModel)]` / `#[derive(UpdateModel)]` 的 struct 上启用：

- `#[orm(input)]`：生成默认命名 `#{StructName}Input`
- `#[orm(input = "TypeName")]`：自定义 input struct 名称
- `#[orm(input_vis = "pub(crate)")]`：自定义可见性

字段级：

- `#[orm(skip_input)]`：不出现在 input（**仅允许 `Option<T>` 字段**）
- 校验规则（可叠加）：
  - `#[orm(required)]`
  - `#[orm(len = "2..=100")]`
  - `#[orm(range = "0..=150")]`
  - `#[orm(email)]`
  - `#[orm(regex = r"^[a-z0-9_]+$")]`
  - `#[orm(url)]`
  - `#[orm(uuid)]`
  - `#[orm(one_of = "a|b|c")]`
  - `#[orm(custom = "path::to::fn")]`
  - `#[orm(input_as = "String")]`：用于把“解析失败”也变成 `ValidationErrors`（目前仅支持 `uuid::Uuid` / `url::Url`）

## 语义（尽量贴近常见验证库，但更扁平）

- **required**：只检查“是否有值”（`Option<T>` 的 `Some`）。
- 除 required 外，其它校验 **只在值存在时运行**（`None` 直接跳过）。
- `Option<Option<T>>`（tri-state）：
  - 校验只在 `Some(Some(v))` 时运行；
  - `required` 在 `None` 或 `Some(None)` 时都视为不满足。
- **错误累积**：同一个字段可产生多条错误；不会 short-circuit。

## 示例：InsertModel 输入验证 + 转换

```rust
use pgorm::InsertModel;

#[derive(InsertModel)]
#[orm(table = "users")]
#[orm(input)]
pub struct NewUser {
    #[orm(len = "2..=100")]
    pub name: String,

    #[orm(email)]
    pub email: String,

    #[orm(range = "0..=150")]
    pub age: Option<i32>,

    // 让 uuid “解析失败”走 ValidationErrors（而不是 serde 直接报错）
    #[orm(uuid, input_as = "String")]
    pub external_id: uuid::Uuid,
}

// 典型流程：
// 1) NewUserInput 反序列化/构造
// 2) input.validate() -> ValidationErrors
// 3) input.try_into_model() -> Result<NewUser, ValidationErrors>
```

## 示例：UpdateModel 输入验证 + 转换

```rust
use pgorm::UpdateModel;

#[derive(UpdateModel)]
#[orm(table = "users", model = "User", returning = "User")]
#[orm(input)]
pub struct UserPatch {
    #[orm(len = "2..=20")]
    pub username: Option<String>,

    #[orm(email)]
    pub email: Option<String>,
}

// UserPatchInput::try_into_patch() -> Result<UserPatch, ValidationErrors>
```

## 与“Changeset 管道/约束映射”的关系

本 MVP 更偏向：**Input struct + validate + try_into\_***，保持简单、可预测、AI 友好。

数据库约束（unique/fk/check）到字段错误的映射属于更高阶需求，可以后续单独加：

- 为 `OrmError::*Violation` 提供结构化 constraint 信息（推荐方向），或
- 在应用层做 best-effort parser + 映射表（可选）。
