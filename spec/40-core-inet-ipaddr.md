# 支持 PostgreSQL INET（`std::net::IpAddr`）的设计与计划

状态：Draft（M1 核心已落地；M2 已落地；M3 待做）  
相关代码：`crates/pgorm` / `crates/pgorm-derive`  
最后更新：2026-01-30

## 背景

在审计日志、登录记录、风控等场景中，`ip_address` 常用 PostgreSQL `inet` 类型存储。当前在 `pgorm` 的真实使用中，会出现两类“使用成本过高”的情况：

1) **用户把 `inet/jsonb` 当成 `String` 映射**  
   - 结果：查询/返回时需要在 SQL 里到处写 `ip_address::text` / `request_params::text`，并维护一份自定义 `SELECT_COLS`（无法复用 `Model::SELECT_LIST`）。

2) **批量插入（UNNEST）缺少类型 cast 支持**  
   - `InsertModel::insert_many*` 会为数组参数生成 `::type[]` cast（由 `pgorm::PgType` 提供）。  
   - 目前 `PgType` 没有覆盖 `std::net::IpAddr`，导致 `insert_many` 不能直接用于 `inet` 字段。

目标是在不改变“SQL-first”定位的前提下，让 `inet` 的读写路径更自然：模型字段用 `IpAddr`，写入/查询都不需要手写 `::text`。

## 结论（先说人话）

- **单条写入/查询**：`tokio-postgres`/`postgres-types` 已经原生支持 `std::net::IpAddr <-> inet`（`ToSql/FromSql`），`pgorm::query().bind(ip)` 直接可用。
- **批量写入（UNNEST）**：`InsertModel::insert_many*` 会生成 `UNNEST($1::type[], ...)`，需要 `PgType` 给出 `"inet[]"`，否则宏生成的 SQL 无法为该列生成正确 cast。
- **常见坑（与本 RFC 直接相关）**：如果表列是 `inet`，但绑定的是 `String`/`Option<String>`，会在编码参数时直接报 `error serializing parameter N`（因为服务端期望 `inet`，客户端按 `text` 编码，类型不接受）。

## 目标 / 非目标

### 目标

- `inet` 列 **一等支持**：`Option<std::net::IpAddr>` 可直接用于：
  - `query().bind(ip_addr)` / `Sql::push_bind(ip_addr)`
  - `FromRow` 映射（`SELECT ... ip_address` -> `IpAddr`）
  - `InsertModel::insert_many*`（UNNEST bulk insert）
- 提供可选的“输入层”能力：当 API 接收 `String` IP 时，可统一校验/转换并返回 `ValidationErrors`（changeset 风格）。
- 在文档和 example 中给出“正确姿势”，减少用户误用 `String` 的概率。

### 非目标（本 RFC 不覆盖）

- `cidr` 的完整支持（网段、掩码与一系列运算）。可以作为后续里程碑拆分。
- IP 地理位置、反向解析、代理链（XFF）解析等业务逻辑。
- 在 `Condition` 中支持任意 SQL 表达式列（例如 `ip_address::text`）的通用安全机制（这属于更大范围的表达式系统）。

## 方案概述

采用“从底到上”的增量方式：

1) **先解决 `insert_many` 的硬障碍**：补齐 `PgType` -> `"inet[]"`。  
2) **再补齐输入层校验/转换（可选但强烈推荐）**：`validate::parse_ip` + `#[orm(ip)]` + `input_as = "String"`（Input 收 String，转换成模型的 `IpAddr`）。  
3) **最后补强文档/示例**：让用户默认就能写对。

## 详细设计

### 1) 核心：`PgType` 支持 INET

新增 `PgType` 实现（用于 UNNEST bulk insert 的参数 cast）：

- `impl PgType for std::net::IpAddr { "inet[]" }`
- （可选）`impl PgType for std::net::Ipv4Addr { "inet[]" }`
- （可选）`impl PgType for std::net::Ipv6Addr { "inet[]" }`

说明：

- `Option<T>` 已经通过 `impl<T: PgType> PgType for Option<T>` 自动覆盖，因此只需实现内层类型。
- 这一步不改变用户的查询写法，只是让 `InsertModel::insert_many*` 不再因为 cast 缺失而卡住。
- 这里的 cast 形态大致是：`INSERT ... SELECT * FROM UNNEST($1::inet[], $2::text[], ...)`（避免“数组参数类型不明确/不匹配”）。
- `Ipv4Addr/Ipv6Addr` 是否要“真支持”取决于上游是否实现了 `ToSql/FromSql`；当前推荐类型仍然是 `IpAddr`。

验证：

- 单元测试：断言 `pg_array_type()` 输出 `"inet[]"`。
- 可运行示例：`examples/inet`（需要 `DATABASE_URL`）。

实现状态（已落地）：

- 代码：`crates/pgorm/src/row.rs` 新增 `PgType for IpAddr`（以及可选 `Ipv4Addr/Ipv6Addr`）
- 测试：`crates/pgorm/src/row.rs` 单元测试覆盖

> 注：这一步解决的是 **bulk insert 的 cast**，不是 `ToSql/FromSql`。后者由 `tokio-postgres` 生态提供，普通 `bind(ip)` 本来就能用。

### 2) 输入层（validate + derive，可选）

问题：很多业务入口（HTTP、消息队列、审计中间件）拿到的是 `Option<String>`，如果每个 repository 自己 `parse()`，会重复且难以给出一致的错误返回。

方案：

1) 在 `pgorm::validate` 增加：
   - `is_ip(s: &str) -> bool`
   - `parse_ip(s: &str) -> Result<std::net::IpAddr, std::net::AddrParseError>`

2) 在 `pgorm::changeset::ValidationCode` 增加 `Ip`（新增 variant；该 enum 已标注 `#[non_exhaustive]`，对外兼容）。

3) 扩展 `InsertModel/UpdateModel` 的输入生成能力：
   - 新增字段属性 `#[orm(ip)]`：当输入字段是 `String/Option<String>` 时做格式校验；
   - 扩展 `input_as`：允许在 `IpAddr` 字段上使用 `#[orm(input_as = "String")]`，将输入 `String` 转为模型字段 `IpAddr`，并在失败时返回 `ValidationErrors`（保持与 uuid/url 的行为一致）。

语义约定（建议在实现时固定下来）：

- `Option<String>` 的 `Some("")` **不做空串归一化**：直接视为无效 IP（与 `url/uuid` 的行为一致），返回 `ValidationCode::Ip`。
- “空串 -> None” 这种“输入清洗”如果要支持，应另加显式属性（例如 `#[orm(trim_empty)]`/`#[orm(empty_as_none)]`），不要让 `#[orm(ip)]` 偷偷承担两种语义。

收益：

- 用户不再在业务代码里写重复的 `ip.parse()`。
- 错误格式统一（`ValidationErrors` + `ValidationCode`），便于 API 直接返回。

代价：

- `derive` 逻辑需要扩展类型识别与转换分支；
- 该能力依赖 `pgorm` 的 `validate` feature（默认已开启）。

### 3) 文档与示例（强制交付）

补齐“用户会照抄的示例”，避免再出现 `inet` 映射成 `String` 的误导：

- `docs/docs/*`（中英文）新增/补充：
  - `inet` 推荐映射：`Option<IpAddr>`
  - 查询过滤：入参 `&str` 先 parse 成 `IpAddr` 再 `.eq(...)`
- 新增 `crates/pgorm/examples/inet`：
  - `insert_returning`（单条）
  - `insert_many`（批量，验证 `PgType` 生效）

## 对外接口（API/宏）

### core（必做）

- `pgorm::PgType` 新增实现：`IpAddr`（以及可选的 `Ipv4Addr/Ipv6Addr`）

### validate（可选）

仅在 feature `validate` 下：

- `pgorm::validate::{is_ip, parse_ip}`
- `pgorm::changeset::ValidationCode::Ip`

### derive（可选）

- `#[orm(ip)]`（InsertModel/UpdateModel 的 input 校验）
- `#[orm(input_as = "String")]`（在 `IpAddr` 字段上：从输入 `String` 转换为模型 `IpAddr`）

## 兼容性与迁移

- `PgType` 新增实现：纯新增，不影响现有行为。
- `ValidationCode` 新增 variant：由于 `#[non_exhaustive]`，对外兼容；但内部文档需要提醒用户匹配时用 `_` 分支。
- derive 新增属性：纯新增，不影响现有宏输出。

迁移建议（给使用者）：

- 将 `ip_address: Option<String>` 改为 `Option<IpAddr>`（模型层）。
- 输入层仍是 `Option<String>` 时：
  - 短期：在边界层 `parse()` 成 `Option<IpAddr>`；
  - 中期：启用 `#[orm(ip)]` + `input_as` 让宏生成转换与校验。

附：审计日志推荐映射（典型字段）

- `ip_address`（PG: `inet`）→ `Option<std::net::IpAddr>`
- `request_params`（PG: `jsonb`）→ `Option<serde_json::Value>` 或 `Option<pgorm::Json<T>>`

## 里程碑 / TODO

### M1（MVP）：让 `insert_many` 直接可用

- [x] `PgType for IpAddr`（+可选 Ipv4Addr/Ipv6Addr）
- [x] 单元测试覆盖
- [ ] `examples/inet`（可运行）

### M2（输入层体验）：减少业务侧重复 parse

- [x] `validate::is_ip/parse_ip`
- [x] `ValidationCode::Ip`
- [x] `derive`：支持 `#[orm(ip)]`
- [x] `derive`：`input_as` 支持 `IpAddr` 字段（`input_as = "String"`）

### M3（文档闭环）

- [ ] 中英文 guide 增加 INET 章节与审计日志示例
- [ ] README/示例索引补充

## Open Questions

1) 是否要把 `cidr` 作为独立 RFC？（大概率是）  
2) `#[orm(ip)]` 的语义边界：只校验格式，还是同时做“空串 -> None”的规范化？  
3) 是否需要提供更“无痛”的绑定 API（例如 `push_bind_str_ip("1.2.3.4")`），还是保持“先 parse 再 bind”的显式风格？  

建议答案：

- Q1：拆分。`cidr` 不只是类型映射，还会牵涉操作符/函数/索引策略与 API 设计，单独 RFC 更清晰。
- Q2：只校验格式；不做空串归一化（如需归一化，另加显式属性）。
- Q3：保持显式 `parse -> bind`。`pgorm` 继续做“SQL-first + 类型安全绑定”，不在 builder 里引入隐式解析/失败分支。
