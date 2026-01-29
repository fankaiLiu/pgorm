# Changeset（变更集）与验证

- 状态：Draft
- 目标版本：0.1.x（可独立于 core，通过 feature 启用）
- 最后更新：2026-01-29

> 参考来源：Ecto (Elixir)，以及 Rust 社区“typed input + 显式校验”的常见做法。

## 背景

`pgorm` 的定位是 **SQL-first、最小魔法、显式可预测**。但在实际业务里，写入（Insert/Update）通常还需要一层“变更/输入验证”：

- 允许在执行 SQL 之前收集 **多个字段错误**（而不是遇到第一个就返回）。
- 错误能够携带结构化信息（field/code/message/metadata），便于 API 层序列化、i18n 或前端定位字段。
- 可选地把 **数据库约束错误**（如 unique / foreign key）映射回字段错误。

Ecto 的 changeset 把“变更 + 验证 + 错误”做成显式管道；本设计尝试把这个思想以更 Rust 的方式落在 `pgorm` 的生态里。

## 目标（Goals）

- **显式**：是否做验证由用户显式调用，不隐式介入 `insert()` / `update_*()`。
- **类型优先**：changeset 以 *typed* 的 InsertModel / UpdateModel 作为输入，不引入运行时反射。
- **错误可组合**：累积错误，且支持跨字段验证。
- **可落地**：不依赖 web 框架；可作为 `pgorm` 的可选模块/feature。

## 不做（Non-goals）

- 不提供 Ecto 风格的“按字符串字段名 cast/cast_typed + 动态 Value 容器”的通用实现（Rust 生态更推荐先 `serde::Deserialize` 到结构体）。
- 不内置 axum/actix 等框架的 response 适配（由应用层完成）。
- 不尝试自动发现/推断数据库约束到字段的映射（用户显式声明映射规则）。

## 模块形态

建议以 **可选模块** 的方式落地，避免给 `pgorm` 的 core 引入额外“隐式行为”：

- 路径：`pgorm::changeset`
- feature：`changeset`（默认关闭）
- 依赖：仅 `std` + `serde_json`（用于 metadata，可选；也可拆为 `changeset` / `changeset-serde` 两个 feature）

## 核心类型

### `ValidationError` / `ValidationErrors`

errors 的数据结构尽量保持 **轻量** 与 **可序列化**（message 必有，metadata 可选，code 可用于 i18n）。

```rust
use std::borrow::Cow;

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationCode {
    Required,
    Format,
    Length,
    Range,
    Inclusion,
    Exclusion,
    Unique,
    ForeignKey,
    Conflict,
    Custom(Cow<'static, str>),
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field: Cow<'static, str>,
    pub code: ValidationCode,
    pub message: Cow<'static, str>,
    // 可选：用于 i18n 或前端（min/max/expected 等）
    pub metadata: std::collections::BTreeMap<Cow<'static, str>, serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct ValidationErrors {
    items: Vec<ValidationError>,
}

impl ValidationErrors {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn push(&mut self, err: ValidationError) {
        self.items.push(err);
    }

    pub fn iter(&self) -> impl Iterator<Item = &ValidationError> {
        self.items.iter()
    }
}
```

说明：

- `field` 使用 `Cow<'static, str>`：既能放静态字段名，也能放动态路径（如 `"items[3].quantity"`）。
- `metadata` 用 `BTreeMap`：稳定输出顺序，便于测试与序列化。

### `Changeset<T>`

changeset 本质上是“值 + 错误”的容器；是否 valid 由 `errors.is_empty()` 推导。

```rust
#[derive(Debug, Clone)]
pub struct Changeset<T> {
    value: T,
    errors: ValidationErrors,
    // 可选：数据库约束映射（用于把 unique/fk 映射回字段）
    constraints: ConstraintMap,
}

#[derive(Debug, Clone, Default)]
pub struct ConstraintMap {
    // constraint_name -> (field, code, message)
    items: std::collections::HashMap<Cow<'static, str>, ConstraintMapping>,
}

#[derive(Debug, Clone)]
pub struct ConstraintMapping {
    pub field: Cow<'static, str>,
    pub code: ValidationCode,
    pub message: Cow<'static, str>,
}

impl<T> Changeset<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            errors: ValidationErrors::default(),
            constraints: ConstraintMap::default(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn errors(&self) -> &ValidationErrors {
        &self.errors
    }

    pub fn validate(mut self, f: impl FnOnce(&T, &mut ValidationErrors)) -> Self {
        f(&self.value, &mut self.errors);
        self
    }

    pub fn map_constraint(
        mut self,
        constraint_name: impl Into<Cow<'static, str>>,
        mapping: ConstraintMapping,
    ) -> Self {
        self.constraints
            .items
            .insert(constraint_name.into(), mapping);
        self
    }

    pub fn into_result(self) -> Result<T, ValidationErrors> {
        if self.errors.is_empty() {
            Ok(self.value)
        } else {
            Err(self.errors)
        }
    }

    /// 校验通过则返回 (value, constraints)，用于后续把数据库约束错误映射回字段错误。
    pub fn into_ok(self) -> Result<(T, ConstraintMap), ValidationErrors> {
        if self.errors.is_empty() {
            Ok((self.value, self.constraints))
        } else {
            Err(self.errors)
        }
    }
}

impl ConstraintMap {
    pub fn to_validation_error(&self, constraint: &str) -> Option<ValidationError> {
        let mapping = self.items.get(constraint)?;
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert(
            "constraint".into(),
            serde_json::Value::String(constraint.to_string()),
        );
        Some(ValidationError {
            field: mapping.field.clone(),
            code: mapping.code.clone(),
            message: mapping.message.clone(),
            metadata,
        })
    }
}
```

## 验证辅助（推荐形态）

为了避免把 “field 名称” 与 “取值方式” 绑定到 `Changeset` 的 API（容易变成字符串反射），推荐把常用校验做成 `ValidationErrors` 的辅助方法/函数：

```rust
use std::ops::RangeInclusive;

impl ValidationErrors {
    pub fn required(&mut self, field: impl Into<Cow<'static, str>>, ok: bool) {
        if ok {
            return;
        }
        self.push(ValidationError {
            field: field.into(),
            code: ValidationCode::Required,
            message: "is required".into(),
            metadata: Default::default(),
        });
    }

    pub fn len(
        &mut self,
        field: impl Into<Cow<'static, str>>,
        s: Option<&str>,
        range: RangeInclusive<usize>,
    ) {
        let Some(s) = s else { return };
        if range.contains(&s.len()) {
            return;
        }
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("min".into(), ((*range.start()) as u64).into());
        metadata.insert("max".into(), ((*range.end()) as u64).into());
        self.push(ValidationError {
            field: field.into(),
            code: ValidationCode::Length,
            message: "has invalid length".into(),
            metadata,
        });
    }
}
```

这种设计的优点：

- `Changeset` 只负责“容器 + 管道”，校验逻辑保持在应用层或 `validation` 子模块；
- 不需要引入 `Value` 动态类型，也不需要 `Params` trait；
- 便于对 InsertModel / UpdateModel 使用同一套函数。

## 数据库约束 → 字段错误

目标：当 `insert()` / `update_*()` 返回 `OrmError::UniqueViolation` / `OrmError::ForeignKeyViolation` 时，能够把它映射成 `ValidationError { field, code, message }`，避免在 API 层输出难以消费的“constraint: message”字符串。

### 推荐：让 `OrmError` 提供结构化 constraint 信息

当前 `pgorm::OrmError` 的约束类错误是 `UniqueViolation(String)` 等，字符串里通常包含 `"constraint_name: message"`。对 changeset 来说，这种信息不稳定、难以可靠解析。

更适合 changeset 的方向是把错误改为结构化字段（示意）：

```rust
pub enum OrmError {
    UniqueViolation { constraint: String, message: String },
    ForeignKeyViolation { constraint: String, message: String },
    CheckViolation { constraint: String, message: String },
    // ...
}
```

### 临时方案：best-effort 解析字符串

在不调整 `OrmError` 形态的前提下，可以在应用层做 best-effort：

```rust
fn parse_constraint_name(s: &str) -> Option<&str> {
    // "users_email_key: duplicate key value violates unique constraint"
    s.split_once(':').map(|(c, _)| c.trim())
}
```

然后用 `map_constraint()` 的映射表把它转为字段错误。

## 使用示例（与 pgorm 写入集成）

### InsertModel：创建用户

```rust
use pgorm::changeset::{Changeset, ConstraintMapping, ValidationCode, ValidationErrors};
use pgorm::{InsertModel, OrmError, OrmResult};

#[derive(InsertModel)]
#[orm(table = "users")]
pub struct NewUser {
    pub name: String,
    pub email: String,
    pub age: Option<i32>,
}

fn new_user_changeset(input: NewUser) -> Changeset<NewUser> {
    Changeset::new(input)
        .validate(|u, e| {
            e.required("name", !u.name.trim().is_empty());
            e.required("email", !u.email.trim().is_empty());
            e.len("name", Some(u.name.as_str()), 2..=100);
        })
        .map_constraint(
            "users_email_key",
            ConstraintMapping {
                field: "email".into(),
                code: ValidationCode::Unique,
                message: "has already been taken".into(),
            },
        )
}

pub async fn create_user(conn: &impl pgorm::GenericClient, input: NewUser) -> OrmResult<u64> {
    let (input, constraints) = new_user_changeset(input).into_ok().map_err(|errs| {
        // pgorm core 目前只有 OrmError::Validation(String)，应用层可自定义更结构化的错误类型
        OrmError::validation(format!("validation failed: {} error(s)", errs.iter().count()))
    })?;

    match input.insert(conn).await {
        Ok(affected) => Ok(affected),
        Err(OrmError::UniqueViolation(s)) => {
            // 应用层：best-effort 解析 constraint 并映射到 field error（示意）
            if let Some(constraint) = parse_constraint_name(&s) {
                if let Some(err) = constraints.to_validation_error(constraint) {
                    let mut errs = ValidationErrors::default();
                    errs.push(err);
                    return Err(OrmError::validation(format!(
                        "validation failed: {} error(s)",
                        errs.iter().count()
                    )));
                }
            }
            Err(OrmError::UniqueViolation(s))
        }
        Err(e) => Err(e),
    }
}
```

### UpdateModel：只校验被更新的字段

```rust
use pgorm::changeset::Changeset;
use pgorm::UpdateModel;

#[derive(UpdateModel)]
#[orm(table = "users")]
pub struct UpdateUser {
    pub name: Option<String>,
    pub email: Option<String>,
    pub age: Option<i32>,
}

fn update_user_changeset(patch: UpdateUser) -> Changeset<UpdateUser> {
    Changeset::new(patch).validate(|p, e| {
        if let Some(name) = p.name.as_deref() {
            e.len("name", Some(name), 2..=100);
        }
        // email/age 同理：只在 Some(...) 时校验
    })
}
```

## 与现有设计的关系

- **和 `UpdateModel` 的 Option patch 互补**：`UpdateModel` 负责“哪些字段写入 SQL”，changeset 负责“写入前校验并收集错误”。
- **和 hook 系统（QueryHook）的关系**：QueryHook 面向 SQL 执行（只看 SQL/QueryContext），适合做 guard/观测；字段级校验仍建议在应用层/仓储层显式调用 changeset，再调用 insert/update。

## 实现检查清单

- [ ] 定义 `ValidationCode` / `ValidationError` / `ValidationErrors`
- [ ] 定义 `Changeset<T>`（只做容器 + validate 管道）
- [ ] 定义 `ConstraintMap` 与 `map_constraint()`（可选）
- [ ] 提供常用校验 helper（required/len/range/format 等）
- [ ] （可选）升级 `OrmError` 的约束类错误为结构化字段，消除字符串解析
- [ ] 单元测试（errors 累积、constraint mapping、序列化稳定性）
