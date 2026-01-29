# Changeset Input 宏（基于 InsertModel / UpdateModel）实现计划

- 状态：Plan / Draft
- 目标版本：0.1.x（先实现 flat input，再扩展到 graph）
- 最后更新：2026-01-29

## 目标

在 `#[derive(InsertModel)]` / `#[derive(UpdateModel)]` 的同一份 struct 定义上，通过额外的 `#[orm(...)]` 配置，自动生成一个“用于接收外部输入”的 **全字段可缺省（Option）** struct（下称 *Input struct*），从而：

- 创建（insert）场景：可以接收“字段不齐全”的 payload，先做验证（required / format / range ...），再转换成真正的 InsertModel。
- 更新（patch）场景：UpdateModel 本身往往已经是 Option patch 语义；生成 Input struct 主要用于隔离 API DTO 与 ORM patch type，并提供统一的验证入口。
- 关键约束：**如果字段本身已经是 `Option<...>`，生成 Input 时不再额外套一层 `Option`**（避免 `Option<Option<T>>` 或更深层嵌套仅为了“可缺省”）。
- 验证风格：尽量对齐 Rust 里常见的 derive + attribute DSL（参考 `validator` 这类库的用法），降低学习成本。

## 非目标（Non-goals）

- 不做动态“按字符串字段名 cast + Value 容器”的通用输入层（Rust 更推荐先 `Deserialize` 到 typed struct）。
- 不强行把验证塞进 `insert()` / `update_*()`；验证仍然是显式调用。
- 不在 v1 做“任意深度嵌套输入/关联写入”一把梭（graph 支持分阶段）。

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

也可以直接在源字段上声明验证（语法建议尽量贴近常见验证库）：

```rust
#[derive(InsertModel)]
#[orm(table = "users")]
#[orm(input = "NewUserInput")]
pub struct NewUser {
    #[orm(validate(length(min = 2, max = 100)))]
    pub name: String,

    #[orm(validate(email))]
    pub email: String,

    #[orm(validate(range(min = 0, max = 150)))]
    pub age: Option<i32>,
}
```

生成（示意）：

```rust
#[derive(Debug, Clone, Default, serde::Deserialize)]
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
#[derive(Debug, Clone, Default, serde::Deserialize)]
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

## 验证 DSL（模仿常见验证库）

本设计建议提供一个 `pgorm::validate::Validate`（或 `pgorm::changeset::Validate`）trait，形态与常见库一致：

```rust
pub trait Validate {
    fn validate(&self) -> Result<(), pgorm::changeset::ValidationErrors>;
}
```

### 字段级验证 attribute

推荐主语法：`#[orm(validate(...))]`（因为 `pgorm` 已经使用 `#[orm(...)]` 作为统一配置入口）。

可选兼容语法：同时支持 `#[validate(...)]` 作为 alias（仅为了贴近生态习惯；实现上由 `InsertModel/UpdateModel` derive 读取该 attribute 并生成校验代码）。

### Option 语义（非常关键）

Input struct 的字段往往是 `Option<T>`；验证行为建议与常见库保持一致：

- 除 `required` 外，其它校验 **只在值存在时运行**（`None` 直接跳过）
- `required` 仅检查 “是否为 Some”，不负责检查字符串空白等（那是 `length/min` 或 custom 的职责）
- `Option<Option<T>>`：当外层 `Some(v)` 时继续校验 `v`（`Some(None)` 通常代表“显式置空”，此时：
  - 对 `required`：应视为不满足（因为值为 NULL）
  - 对其它校验：默认跳过或报错取决于规则；v1 建议跳过，交给业务自定义）

### v1 内置校验集合（建议先小后大）

- `required`：仅用于 Input 侧；默认会对“源字段非 Option，但在 Input 中被 Option 化”的字段自动注入 required（用户通常不需要手写）
- `length(min = ?, max = ?)`：字符串/集合长度
- `range(min = ?, max = ?)`：数值范围（支持 `i*`/`u*`/`f*` 先按可实现性收敛）
- `email`：邮箱格式（v1 可用简化规则或依赖 `email_address`/`validator` 等第三方 crate，建议 feature gating）
- `custom(fn = "path::to::fn")`：自定义校验（参数建议为 `&T` 或 `&Option<T>`，返回 `Result<(), &'static str>` / `Result<(), ValidationError>`）

> 备注：如果你说的“很有名的验证库”不是 Rust 的 `validator` 风格（derive + attribute），这块 DSL 我再按你要对齐的库改语法。

## 字段选择规则（v1 建议）

默认生成 input 时使用“源 struct 的字段集合”，但需要提供可控的排除能力，否则 UpdateModel 里出现 “T: always bind” 的内部字段会让 input 变得不合理。

建议新增字段级 attribute（命名待定）：

- `#[orm(skip_input)]`：不出现在 input struct 里，也不参与 `try_into_model()` 的 required 检查

并在文档中给出推荐：

- 对于 DB 自动生成/内部填充的字段（`id`、`updated_at`、服务端计算字段等），要么源 struct 用 `Option<T>` 表达“可缺省”，要么标 `#[orm(skip_input)]`。

## 转换策略（InsertModel / UpdateModel）

### InsertModel：`Input -> InsertModel`

生成 `try_into_model()`（建议顺序：先 validate，再转换）：

- 1) `self.validate()`：根据字段上的 `validate(...)` 规则（含“默认 required 推断”）产出 `ValidationErrors`
- 2) 构造 InsertModel：对每个源字段：
  - 源字段是 `Option<_>`：直接赋值（input 同型）
  - 源字段是 `T`：从 `Option<T>` 取值（理论上在 validate 后应为 `Some`；实现上仍建议做一次防御性检查，避免误用时 panic）
- 返回 `Result<InsertModel, ValidationErrors>`

> v1 目标是做到：required/length/range/email/custom 这类“字段级校验”可由宏生成；跨字段/跨对象校验仍由用户显式代码完成（后续可加 hook 点/扩展语法）。

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

验证模块（`pgorm::changeset`）负责提供：

- `ValidationError(s)`：结构化错误容器
- `Validate` trait（或等价的 `Input::validate()` 约定）
- `Changeset<T>`：可选的 validate 管道（显式调用）

**决策点（需要你确认）**：`pgorm::changeset` 是否做成默认可用（仅类型，无行为），还是 feature gating。

- 若要让 derive 生成的 `try_into_model()` 始终可用，最简单是：`pgorm::changeset` 类型默认存在（不要求用户额外开 feature）。

## 宏配置（建议最小集合）

结构体级：

- `#[orm(input)]`：开启，默认命名 `#{StructName}Input`，默认 `pub`
- `#[orm(input = "TypeName")]`：自定义 input struct 名称
- `#[orm(input_vis = "pub(crate)")]`：自定义可见性（字符串解析为 `syn::Visibility`）

字段级：

- `#[orm(skip_input)]`：不生成该字段到 input
- `#[orm(validate(...))]`：字段级校验（建议 DSL 贴近常见验证库）
- （可选）`#[validate(...)]`：与 `#[orm(validate(...))]` 等价的 alias

## 实现步骤（Milestones）

### M0：最小可用（InsertModel）

- [ ] 在 `pgorm` crate 中落地 `pgorm::changeset::ValidationErrors` + `Validate` trait（先只需要 `Required`/`length`/`range` 这类基础即可）
- [ ] 扩展 `pgorm-derive` 的 `InsertModel`：
  - [ ] 解析 `#[orm(input...)]` 配置
  - [ ] 生成 input struct（按“Option 不再套”规则）
  - [ ] 解析字段上的 `#[orm(validate(...))]`（可选：同时读取 `#[validate(...)]`）
  - [ ] 生成 `impl Validate for Input`（或 `Input::validate()` 方法）
  - [ ] 生成 `try_into_model()`：先 `validate()`，再做 required 缺失检查并累积错误
- [ ] 添加 `crates/pgorm/examples/` 示例：`NewUserInput -> validate -> NewUser -> insert`

### M1：UpdateModel 支持

- [ ] 扩展 `UpdateModel` 同样支持 `#[orm(input...)]`
- [ ] 支持 `#[orm(skip_input)]`
- [ ] 明确策略：同型/异型（见上文“转换策略”），并给出至少 1 个示例

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
- `pgorm`：单测 `ValidationErrors` 的 required 累积行为（输出稳定、可序列化）
