# Changeset Input 宏（基于 InsertModel / UpdateModel）实现计划

- 状态：MVP 已实现（仍可迭代）
- 目标版本：0.1.x（先实现 flat input，再扩展到 graph）
- 最后更新：2026-01-29

## 目标

在 `#[derive(InsertModel)]` / `#[derive(UpdateModel)]` 的同一份 struct 定义上，通过额外的 `#[orm(...)]` 配置，自动生成一个“用于接收外部输入”的 **全字段可缺省（Option）** struct（下称 *Input struct*），从而：

- 创建（insert）场景：可以接收“字段不齐全”的 payload，先做验证（required / format / range ...），再转换成真正的 InsertModel。
- 更新（patch）场景：UpdateModel 本身往往已经是 Option patch 语义；生成 Input struct 主要用于隔离 API DTO 与 ORM patch type，并提供统一的验证入口。
- 关键约束：**如果字段本身已经是 `Option<...>`，生成 Input 时不再额外套一层 `Option`**（避免 `Option<Option<T>>` 或更深层嵌套仅为了“可缺省”）。
- 验证风格：**扁平、少嵌套、默认值可预测**（更适合 AI 生成/修改）。

## 非目标（Non-goals）

- 不做动态“按字符串字段名 cast + Value 容器”的通用输入层（Rust 更推荐先 `Deserialize` 到 typed struct）。
- 不强行把验证塞进 `insert()` / `update_*()`；验证仍然是显式调用。
- 不在 v1 做“任意深度嵌套输入/关联写入”一把梭（graph 支持分阶段）。

## 放在哪里（新 crate vs 现有 crate）

结论：**放在现有 `pgorm` crate 里**，不新开 crate。

原因：

- 派生宏（`pgorm-derive`）生成的代码需要一个稳定的运行时入口（`pgorm::changeset` / `pgorm::validate`），放在同一个 crate 路径最省心，也最 AI 友好（不用解释“再加一个依赖/feature”）。
- `pgorm` 的定位是库而不是框架：验证功能可以通过 feature 变成“可选能力”，不影响纯 SQL-first 用户。

建议模块与 feature：

- `pgorm::changeset`：错误类型与收集容器（`ValidationError(s)`）
- `pgorm::validate`：校验实现（len/range/email/regex/url/uuid/one_of 等）
- Cargo feature：`validate`（默认开启；关闭后只能使用不依赖外部 crate 的校验项）
  - 依赖：`regex`（regex），`url`（url），`uuid`（已存在），email 可用 `regex` 或额外 `email_address`

## 用户侧 API（草案）

### InsertModel

```rust
use pgorm::InsertModel;

#[derive(InsertModel)]
#[orm(table = "users")]
#[orm(input = "NewUserInput")] // 开启生成（也可省略，走默认命名）
pub struct NewUser {
    pub name: String,
    pub email: String,
    pub age: Option<i32>,
}
```

也可以直接在源字段上声明验证（扁平语法，AI 友好）：

```rust
#[derive(InsertModel)]
#[orm(table = "users")]
#[orm(input = "NewUserInput")]
pub struct NewUser {
    #[orm(len = "2..=100")]
    pub name: String,

    #[orm(email)]
    pub email: String,

    #[orm(range = "0..=150")]
    pub age: Option<i32>,
}
```

生成（示意）：

```rust
#[derive(Debug, Clone, Default, ::pgorm::serde::Deserialize)]
#[serde(crate = "::pgorm::serde")]
pub struct NewUserInput {
    pub name: Option<String>,      // String -> Option<String>
    pub email: Option<String>,     // String -> Option<String>
    pub age: Option<i32>,          // Option<i32> 保持不变（不再套 Option）
}
```

并生成一个转换入口（示意）：

```rust
impl NewUserInput {
    pub fn try_into_model(self) -> Result<NewUser, pgorm::changeset::ValidationErrors> { /* ... */ }
}
```

默认规则：**原 InsertModel 中非 Option 字段视为 required**，缺失则产生 `Required` 错误；Option 字段不要求提供。

### UpdateModel

```rust
use pgorm::UpdateModel;

#[derive(UpdateModel)]
#[orm(table = "users", id_column = "id")]
#[orm(input)] // 默认生成 UserPatchInput
pub struct UserPatch {
    pub name: Option<String>,
    pub bio: Option<Option<String>>, // tri-state：skip / set NULL / set value
}
```

生成（示意）：

```rust
#[derive(Debug, Clone, Default, ::pgorm::serde::Deserialize)]
#[serde(crate = "::pgorm::serde")]
pub struct UserPatchInput {
    pub name: Option<String>,           // 已经是 Option，不再套
    pub bio: Option<Option<String>>,    // 已经是 Option<...>，不再套（避免 Option<Option<Option<_>>>）
}
```

`UpdateModel` 的“是否更新/置空”语义由字段类型本身决定：

- `Option<T>`：`None` => skip；`Some(v)` => set value
- `Option<Option<T>>`：`None` => skip；`Some(None)` => set NULL；`Some(Some(v))` => set value

Input struct 不改变这层语义，只提供 DTO 隔离与验证入口。

## 类型映射规则（必须满足“Option 不再套”）

对源 struct 的每个字段 `F: Ty`，生成 input 字段类型 `TyIn`：

1. 若 `Ty` 的最外层是 `Option<Inner>`：`TyIn = Ty`（保持不变）
2. 否则：`TyIn = Option<Ty>`

例子：

- `String` -> `Option<String>`
- `Option<i32>` -> `Option<i32>`（不变）
- `Option<Option<String>>` -> `Option<Option<String>>`（不变）

> 说明：该规则只检查“最外层是否 Option”，不会对内部泛型做递归包裹。

## 验证 DSL（AI 友好版）

本设计建议生成一个固定签名的 `Input::validate()` 方法（不需要额外 trait/import，更适合 AI 生成/修改）：

```rust
impl NewUserInput {
    pub fn validate(&self) -> pgorm::changeset::ValidationErrors {
        /* generated */
    }
}
```

### 字段级验证 attribute

推荐语法：把校验规则直接声明为 `#[orm(...)]` 的 item（避免 `validate(...)` 再包一层）：

- `#[orm(required)]`
- `#[orm(len = "2..=100")]`
- `#[orm(range = "0..=150")]`
- `#[orm(email)]`
- `#[orm(regex = r"^[a-z0-9_]+$")]`
- `#[orm(url)]`
- `#[orm(uuid)]`
- `#[orm(one_of = "a|b|c")]`
- `#[orm(custom = "crate::path::to::fn")]`
- `#[orm(input_as = "String")]`（仅对 `uuid::Uuid` / `url::Url` 生效；用于让“解析失败”走 `ValidationErrors`）

允许同一个字段写多个校验项：

```rust
#[orm(required, len = "2..=100")]
pub name: String,
```

### Option 语义（非常关键）

Input struct 的字段往往是 `Option<T>`；验证行为建议与常见库保持一致：

- 除 `required` 外，其它校验 **只在值存在时运行**（`None` 直接跳过）
- `required` 仅检查 “是否为 Some”，不负责检查字符串空白等（那是 `len = "..."` 或 custom 的职责）
- `Option<Option<T>>`：当外层 `Some(v)` 时继续校验 `v`（`Some(None)` 通常代表“显式置空”，此时：
  - 对 `required`：应视为不满足（因为值为 NULL）
  - 对其它校验：默认跳过或报错取决于规则；v1 建议跳过，交给业务自定义）

### v1 内置校验集合（建议先小后大）

- `required`：默认会对“源字段非 Option，但在 Input 中被 Option 化”的字段自动注入（用户通常不需要手写）
- `len = "min..=max"`：字符串/集合长度
- `range = "min..=max"`：数值范围（支持 `i*`/`u*`/`f*` 先按可实现性收敛）
- `email`：邮箱格式（v1 可先用简化规则；更严格可选依赖 `email_address`）
- `regex`：正则匹配（建议使用 raw string：`regex = r"..."`，避免转义噩梦）
- `url`：URL 解析
- `uuid`：UUID 解析
- `one_of = "a|b|c"`：枚举值/白名单（建议用 `|` 分隔，避免空格干扰；后续也可以支持 `,`）
- `custom = "path::to::fn"`：自定义校验（参数建议为 `&T` 或 `&Option<T>`，返回 `Result<(), &'static str>` / `Result<(), ValidationError>`）

### 类型要求与 emstr 集成

- `len/email/regex/url/uuid/one_of`：建议作用在“可视作字符串”的类型上（`String`/`&str`/`Cow<str>` 等）。实现上应尽量用 `AsRef<str>` 作为约束，这样如果你的 `emstr` 类型实现了 `AsRef<str>`，就能直接用这些校验项。
- `range`：作用在数值类型上（至少覆盖 `i64/i32/u64/u32/f64/f32`）。

> 注意：如果字段类型本身是 `uuid::Uuid` 或 `url::Url`，无效值通常会在 `serde` 反序列化阶段就报错（而不是进入 `validate()` 产出字段错误）。如果你希望“无效 uuid/url”也走 `ValidationErrors`，需要引入一个“输入类型覆盖”机制（例如 `#[orm(input_as = "String", uuid)]`）；该能力可作为 v1.1 追加，不阻塞 v1 先落地。
>
> 目前 MVP 已支持 `#[orm(input_as = "String")]`（仅 `uuid::Uuid` / `url::Url`），用于把解析失败转成字段错误（而不是 serde 直接失败）。

## 字段选择规则（v1 建议）

默认生成 input 时使用“源 struct 的字段集合”，但需要提供可控的排除能力，否则 UpdateModel 里出现 “T: always bind” 的内部字段会让 input 变得不合理。

已实现字段级 attribute：

- `#[orm(skip_input)]`：不出现在 input struct 里（限制：**只能用于 `Option<T>` 字段**）

并在文档中给出推荐：

- 对于 DB 自动生成/内部填充的字段（`id`、`updated_at`、服务端计算字段等），要么源 struct 用 `Option<T>` 表达“可缺省”，要么标 `#[orm(skip_input)]`。

## 转换策略（InsertModel / UpdateModel）

### InsertModel：`Input -> InsertModel`

生成 `try_into_model()`（建议顺序：先 validate，再转换）：

- 1) `self.validate()`：根据字段上的校验项（含“默认 required 推断”）产出 `ValidationErrors`
- 2) 构造 InsertModel：对每个源字段：
  - 源字段是 `Option<_>`：直接赋值（input 同型）
  - 源字段是 `T`：从 `Option<T>` 取值（理论上在 validate 后应为 `Some`；实现上仍建议做一次防御性检查，避免误用时 panic）
- 返回 `Result<InsertModel, ValidationErrors>`

> v1 目标是做到：required/len/range/email/custom 这类“字段级校验”可由宏生成；跨字段/跨对象校验仍由用户显式代码完成（后续可加 hook 点/扩展语法）。

### UpdateModel：`Input -> UpdateModel`

两种可选实现路径（择一）：

1. **同型生成（推荐 v1）**：当源 UpdateModel 所有字段最外层都是 `Option<_>` 或标注 `#[orm(default)]`/`#[orm(skip_update)]` 等可缺省形态时，input 与 patch 可以同型，`try_into_patch()` 仅做字段直传（或直接 `type UserPatchInput = UserPatch` 不生成新 struct）。
2. **异型生成**：如果源 UpdateModel 存在非 Option 的 “always bind” 字段，则 input 字段会被包成 `Option<T>`，需要：
   - 要么要求用户标 `#[orm(skip_input)]` 排除这些字段；
   - 要么在 `try_into_patch()` 中对缺失字段报 `Required`（更偏“强制更新字段”场景）。

## 与验证模块的关系

Input 宏解决三件事：

- 输入类型（全可缺省，且 Option 不再套）
- 字段级校验代码生成（含默认 required 推断）
- DTO 与 ORM 类型隔离（Input -> InsertModel/UpdateModel）

运行时入口：

- `pgorm::changeset`：默认可用（仅类型）
- `pgorm::validate`：feature `validate`（默认开启）；email/regex/url/uuid/input_as 依赖它

## 宏配置（建议最小集合）

结构体级：

- `#[orm(input)]`：开启，默认命名 `#{StructName}Input`，默认 `pub`
- `#[orm(input = "TypeName")]`：自定义 input struct 名称
- `#[orm(input_vis = "pub(crate)")]`：自定义可见性（字符串解析为 `syn::Visibility`）

字段级：

- `#[orm(skip_input)]`：不生成该字段到 input
- `#[orm(input_as = "...")]`：输入类型覆盖（当前仅支持 `uuid::Uuid` / `url::Url`）
- `#[orm(required)]` / `#[orm(len = "...")]` / `#[orm(range = "...")]` / `#[orm(email)]` / `#[orm(regex = "...")]` / `#[orm(url)]` / `#[orm(uuid)]` / `#[orm(one_of = "...")]` / `#[orm(custom = "...")]`：字段级校验项（扁平化，少嵌套）

## 实现步骤（Milestones）

### M0：最小可用（InsertModel）

- [x] 在 `pgorm` crate 中落地 `pgorm::changeset::ValidationErrors`（字段错误 + code + metadata）
- [x] 在 `pgorm` crate 中落地 `pgorm::validate`（email/regex/url/uuid）
- [x] Cargo feature `validate`（聚合依赖：`regex`/`url`）
- [x] 扩展 `pgorm-derive` 的 `InsertModel`（input struct + validate + try_into_model）
- [ ] 添加 `crates/pgorm/examples/` 示例：`NewUserInput -> validate -> NewUser -> insert`（目前在 integration test 中已有覆盖）

### M1：UpdateModel 支持

- [x] 扩展 `UpdateModel` 同样支持 `#[orm(input...)]`
- [x] 支持 `#[orm(skip_input)]`
- [x] `try_into_patch()` + integration test 覆盖

### M2：完善验证体验（可选）

- [ ] 在 `pgorm::changeset` 补齐常用 helpers（len/range/format 等）
- [ ] 约束错误映射：为 `UniqueViolation/ForeignKeyViolation/CheckViolation` 提供结构化 `constraint`（推荐方向），或者提供 best-effort parser + 映射表

### M3：Graph（多表写入）输入（可选，后续）

- [ ] 允许 input struct 生成时包含 graph 字段（has_many/belongs_to/before/after steps 的字段）
- [ ] 约定 nested input 的命名与转换（例如 child input -> child insert model）
- [ ] 明确递归深度与错误路径表示（例如 `items[3].quantity`）

## 测试策略

- `pgorm-derive`：引入 `trybuild` 做编译期测试
  - [ ] Option 不再套（`Option<T>` 不变、`Option<Option<T>>` 不变）
  - [ ] `skip_input` 生效
  - [ ] 命名/可见性配置正确
- `pgorm`：integration test 覆盖基础校验与转换（`crates/pgorm/tests/changeset_input.rs`）
