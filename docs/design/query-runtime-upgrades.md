# pgorm 设计文档：流式查询 / Prepared Statement / 动态条件 / SQL 校验增强

- 状态：Draft
- 目标版本：0.1.x（可拆分为多个小版本逐步落地）
- 最后更新：2026-01-29

## 背景与问题

当前 `pgorm` 已具备：

- `Sql` 动态拼接（`sql()` / `query()`）与 `fetch_all/_one/_opt`（返回 `Vec<Row>` 或单行）
- `Condition` 原子条件与 derive 生成的 `Model::query()`（内部是 `Vec<Condition>` 且只支持 `AND`）
- `CheckedClient` / `PgClient` 的 SQL lint + schema 校验（运行时校验）

但在以下方向仍有明显空缺或不足：

1. **流式查询/游标**：所有查询默认 `Vec<Row>`，大结果集容易把内存拉满；也不利于“边拉边处理”的业务模式。
2. **Prepared statement + statement cache**：当前执行路径基于 SQL 字符串直跑；缺少可控的 prepare/caching 策略，可能带来解析/计划抖动与额外开销。
3. **常用便捷 API**：缺少 `fetch_scalar(_one/_opt)`、`exists`、统一的分页 helper（`LIMIT/OFFSET`），导致业务层重复造轮子。
4. **动态 WHERE 真正落地**：`Condition` 目前是“原子条件”，derive query 只会把它们用 `AND` 串起来；缺少 `AND/OR/grouping` 等表达能力，也缺少结构化的排序/分页 builder。
5. **SQL/schema 校验更强**：现有 schema 校验对 alias/CTE/JOIN 覆盖不足；并且每次校验都要解析 SQL，缺少解析结果缓存，可能导致运行时开销与误报/漏报。

## 设计目标

### Goals

- 提供 **流式结果消费** 的一等支持（避免一次性 `Vec<Row>`）。
- 提供 **prepared statement** 的可选支持，并带 **容量可控的 statement cache**。
- 提供一组高频 **便捷 API**：`fetch_scalar(_one/_opt)`、`exists`、`limit/offset`。
- 把 `Condition` 扩展为可表达 **AND/OR/NOT/grouping** 的 WHERE 表达式（并提供排序/分页 builder）。
- 增强 SQL/schema 校验：覆盖 **alias/CTE/JOIN**，并引入 **SQL 解析结果缓存**。

### Non-goals（暂不做）

- 不追求全自动 ORM（保持 “SQL-first、少魔法”）。
- 不做编译期 SQL 校验（仍以运行时校验为主）。
- 不在本设计中引入复杂的 DSL（以最小可用 builder 为目标）。

## 总体方案（可组合的能力层）

推荐采用“能力分层 + wrapper 可组合”的方式（与现有 `InstrumentedClient` / `CheckedClient` / `PgClient` 一致）：

1. **基础执行层**：`GenericClient` 仍保留作为最小能力集（query/execute 返回 `Vec<Row>` 或单行）。
2. **扩展能力层（新 trait）**：
   - 流式：`StreamingClient`（`query_stream` / `query_stream_as`）
   - prepared：`PreparedClient`（`prepare_cached` 或 `query_prepared`）
3. **可组合 wrapper**：
   - `StatementCached<C>`：在客户端侧做 statement LRU cache（每个连接/事务一份）
   - `StreamingChecked<C>`（可选）：对流式查询也做校验/监控（或由 `PgClient` 直接实现 streaming）
4. **Builder/API 层**：`Sql` 与新的 `WhereExpr/OrderBy/Pagination` 只负责“拼 SQL + 参数”，执行交给 *client traits*。
5. **SQL 分析与校验层**：引入 `SqlAnalysis` + LRU parse cache；schema 校验基于 analysis 结果做 alias/CTE/JOIN-aware 校验。

## 1) 流式查询 / 游标

### API 设计

新增 trait（建议放在 `crates/pgorm/src/client.rs` 或新模块 `stream.rs`，并通过 feature `stream` 控制对外暴露）：

```rust
/// Stream 内部持有 owned 的 SQL 和参数，避免 lifetime 被 sql/params 的借用卡死。
#[cfg(feature = "stream")]
pub struct BuiltSql {
    pub sql: String,
    pub params: Vec<Arc<dyn ToSql + Sync + Send>>,
}

#[cfg(feature = "stream")]
pub type RowStream =
    std::pin::Pin<Box<dyn futures_core::Stream<Item = OrmResult<tokio_postgres::Row>> + Send>>;

#[cfg(feature = "stream")]
pub trait StreamingClient: Send + Sync {
    /// 接受 owned 的 SQL 和参数，stream 内部持有它们的生命周期。
    /// 这样 Sql::fetch_stream 可以把 to_sql() 产生的 String 移入 stream。
    fn query_stream_owned(
        &self,
        built: BuiltSql,
    ) -> RowStream;
}
```

> **关键设计决策：owned 形态**
>
> 最初设计为 `query_stream<'a>(&'a self, sql: &'a str, params: &'a [...]) -> RowStream<'a>`，
> 但 `Sql::fetch_stream` 需要把 `to_sql()` 产生的临时 `String` 借出 `'a` 生命周期，
> 这在 Rust 的借用模型下无法实现。因此改为 **stream 内部持有 owned 数据**（`BuiltSql`），
> stream 本身为 `'static`（仅受 `&self` 约束）。

在 `Sql` builder 上提供对等能力：

```rust
#[cfg(feature = "stream")]
impl Sql {
    pub fn fetch_stream(
        &self,
        conn: &impl StreamingClient,
    ) -> OrmResult<RowStream> {
        self.validate()?;
        let built = BuiltSql {
            sql: self.to_sql(),
            params: self.params.clone(), // Arc clone, cheap
        };
        Ok(conn.query_stream_owned(built))
    }

    pub fn fetch_stream_as<T: FromRow + Send + 'static>(
        &self,
        conn: &impl StreamingClient,
    ) -> OrmResult<impl Stream<Item = OrmResult<T>> + Send> { /* row->T map */ }
}
```

说明：

- "游标"层面不一定需要显式 `DECLARE CURSOR`：`tokio-postgres` 的 `query_raw`/portal 已能实现按批拉取并流式消费（对业务就是 cursor-like）。
- 显式 server-side cursor（`DECLARE ... CURSOR` / `FETCH n`）可作为后续增强（需要事务语义与生命周期管理）。

### 连接占用警告

> ⚠️ **Streaming 会占用连接直到 stream 被完全消费或 drop。**
>
> 在 pool 场景下，这意味着一个 stream 的整个生命周期中，底层连接不能被归还给 pool、不能被其他查询复用。
> 如果消费端处理很慢或长时间不 poll，可能导致连接池耗尽。
>
> 建议：
> - 在文档和 API 注释中**显式标注**这一行为，避免用户把 stream 当作普通 `fetch_all` 使用。
> - 对于需要长时间处理每行数据的场景，建议先 `fetch_all` 到内存（如果数据量可控），或者使用分页查询。
> - 考虑在 `StreamConfig` 中提供 `max_idle_before_warn` 配置，当连接被 stream 占用超过阈值时记录 warning 日志。

### 与监控/统计/校验的集成

流式查询在监控维度需要解决两个问题：

1. **耗时**：开始时间到 stream 结束/被 drop 的时间。
2. **行数**：需要在 stream 迭代中累加。

建议为 streaming 提供一个包装 stream：

- `InstrumentedRowStream`：在 `poll_next` 中计数，在 `Drop` 或 “返回 None” 时上报 `QueryResult::Rows(n)` + duration。
- 对 `PgClient`：在 `query_stream` 前执行 `check_sql(sql)`，并复用 hook 逻辑（hook 若会修改 SQL，要明确对缓存/统计的影响）。

## 2) Prepared statement 支持 + statement cache

### 核心约束

- **Prepared statement 是连接级别** 的：不能跨连接复用（pool 场景每个 connection 都要维护自己的 cache）。
- 缓存要 **可控容量**（LRU/TTL），否则会导致内存增长或服务端 statement 过多。
- 需要处理 **失效**：例如 schema 变更导致 “cached plan must not change result type” 等错误时，自动驱逐并重试一次。

### API 设计：`StatementCached<C>` wrapper

新增 wrapper（建议放 `crates/pgorm/src/prepared.rs`）：

> **Trait 约束说明**
>
> `StatementCached<C>` **不能**通过 `GenericClient` trait 实现 prepared statement，因为
> `GenericClient` 只提供 `query(&str, ...)` 形式的接口，没有 `prepare()`/`query(&Statement, ...)` 入口。
>
> 实现路径（二选一）：
>
> 1. **直接约束 inner 类型**：`C` 必须是 `tokio_postgres::Client` 或 `Transaction`（它们原生支持 `prepare` + `query(&Statement, ...)`），而非任意 `impl GenericClient`。`StatementCached` 对外仍然实现 `GenericClient`（这样上层 `PgClient` 可以组合），但内部调用底层 prepared 接口。
> 2. **引入 `PreparedClient` trait**（设计文档总体方案中提到的）：新增 trait 提供 `prepare_cached()` 能力，让 `StatementCached` 的 `C` 约束为 `C: PreparedClient`。

```rust
/// StatementCached 的 inner 必须支持 prepare 能力。
/// 实际约束为 tokio_postgres::Client / Transaction / deadpool 的 ClientWrapper。
pub struct StatementCached<C> {
    inner: C,
    cfg: StatementCacheConfig,
    cache: tokio::sync::Mutex<lru::LruCache<String, tokio_postgres::Statement>>,
}

impl<C> StatementCached<C> {
    pub fn new(inner: C, cfg: StatementCacheConfig) -> Self { /* ... */ }
    pub fn inner(&self) -> &C { &self.inner }
}
```

并实现 `GenericClient`：

- `query/query_one/query_opt/execute`：
  1. 若 `cfg.enabled == false`：直接委托 `inner.query(...)`
  2. 否则：`prepare_cached(sql)` -> `inner.query(&stmt, params)`（或 equivalent）  
     - 失败且属于“statement invalid/plan invalid”类错误：驱逐 cache entry，重试一次

这样可以与 `PgClient` / `CheckedClient` / `InstrumentedClient` 组合：

```rust
// 组合：Prepared + Check + Monitor
let base = deadpool.get().await?;
let cached = StatementCached::new(base, StatementCacheConfig { capacity: 256, enabled: true, retry_on_invalid: true });
let pg = PgClient::new(cached); // 派生代码仍用 &impl GenericClient
```

### statement cache key 与 hook 的交互

若 hook 会 `ModifySql`（例如加注释/打 tag），会导致：

- cache key 变成“每次都不同”，cache 命中率下降甚至失效；
- 统计维度（按 SQL string 聚合）也会被打散。

建议：

- 把“tag”放到 `QueryContext.tag`（不通过改 SQL 实现），让监控可见而 cache 可复用；
- 若必须 ModifySql：statement cache key 可选择 “原始 SQL” 或 “修改后 SQL”，需在文档中明确语义（默认建议使用 *最终执行 SQL* 作为 key，更直观，但可能更碎）。

## 3) 便捷 API：fetch_scalar / exists / 分页

### fetch_scalar(_one/_opt)

新增在 `Sql` 上（也可在 `GenericClientExt` 上提供 `&str` 版本）：

- `fetch_scalar_one<T>() -> OrmResult<T>`：期望恰好一行一列，取 `row.get(0)`
- `fetch_scalar_opt<T>() -> OrmResult<Option<T>>`：0 行返回 `None`，否则取第一行第一列
- 可选：`fetch_scalar_all<T>() -> OrmResult<Vec<T>>`

示例：

```rust
let count: i64 = sql("SELECT COUNT(*) FROM users WHERE status = ")
    .push_bind("active")
    .fetch_scalar_one(&client)
    .await?;
```

### exists

两个层级的 API：

1. 基于 `Sql`：
   - `Sql::exists(&self, conn) -> OrmResult<bool>`
   - 通过包裹：`SELECT EXISTS(<original_select>)`
2. 基于 derive query builder：
   - `Model::query().eq(...).exists(&conn).await?`（内部拼 `SELECT 1 ... LIMIT 1`）

设计上需明确：

- `exists` 只对 SELECT-like SQL 有意义；若用户传入非 SELECT，返回 `OrmError::Validation`。
- **SELECT 判定不能只用 `starts_with("SELECT")`**：必须至少处理 `WITH ... SELECT`、`-- 注释开头`、`/* 块注释 */`、括号包裹 `(SELECT ...)` 等场景。当前实现（`crates/pgorm/src/sql.rs`）通过 `strip_sql_prefix()` 辅助函数跳过注释/空白/括号后再检测首个关键字。后续引入 `SqlAnalysis` parse cache 后，应改为基于 `analysis.statement_kind` 来判定（更准确、统一）。

### 分页 helper（limit/offset）

在 `Sql` 上提供最小集合：

- `limit(n)` / `offset(n)` / `limit_offset(limit, offset)`：使用 bind 参数（`LIMIT $n OFFSET $m`），避免拼接字符串。
- `page(page, per_page)`：内部转为 `limit_offset(per_page, (page-1)*per_page)`，并对 page 做 `>=1` 约束。

并在 derive query builder 中从“字符串拼 LIMIT/OFFSET”迁移为复用该 helper（保证行为一致、减少重复实现）。

## 4) 更完整的动态 WHERE + 排序/分页 builder（Condition 落地）

### Where 表达式：从 `Vec<Condition>` 升级为布尔表达式树

保留现有 `Condition` 作为“原子谓词”，新增表达式树类型（命名可讨论，下面以 `WhereExpr` 为例）：

```rust
pub enum WhereExpr {
    Atom(Condition),
    And(Vec<WhereExpr>),
    Or(Vec<WhereExpr>),
    Not(Box<WhereExpr>),
    Raw(String), // 明确标注为危险 API
}

impl WhereExpr {
    pub fn and_with(self, other: WhereExpr) -> WhereExpr { /* ... */ }
    pub fn or_with(self, other: WhereExpr) -> WhereExpr { /* ... */ }

    /// 将表达式追加到 Sql builder 中（通过 Sql 的 push/push_bind 生成正确的占位符）。
    /// 这是当前实现的主要入口，不提供独立的 build() -> (String, Vec<&ToSql>)。
    /// 原因：WhereExpr 内部通过 Condition::append_to_sql 与 Sql builder 交互，
    /// 由 Sql 统一管理占位符索引，避免手动 param_idx 传递的复杂性。
    pub fn append_to_sql(&self, sql: &mut Sql) { /* 递归拼接 + 括号 */ }
}
```

关键点：

- **括号策略**：为避免优先级歧义，复合节点统一输出括号：`(a AND b)`、`(a OR b)`、`(NOT a)`。
- **空表达式**：需要定义 `WhereExpr::And([])` / `WhereExpr::Or([])` 的语义（推荐：`And([]) == TRUE`，`Or([]) == FALSE`，用于按条件可选拼接）。

### derive query builder 的升级路径

把 derive 生成的 query struct 从：

- `conditions: Vec<Condition>`（固定 `AND`）

升级为：

- `where_expr: Option<WhereExpr>` 或 `where_expr: WhereExpr`（默认 TRUE）

并提供兼容 API：

- `.eq/.lt/...` 继续存在：默认将新条件 `AND` 到当前表达式
- 新增：
  - `.and(expr)` / `.or(expr)`（表达式级别拼接）
  - `.group(|q| ...)`（builder 风格生成子表达式并加括号）

示例：

```rust
let q = User::query()
    .eq(UserQuery::COL_STATUS, "active")
    .and(
        WhereExpr::Or(vec![
            Condition::eq(UserQuery::COL_ROLE, "admin").into(),
            Condition::eq(UserQuery::COL_ROLE, "owner").into(),
        ])
    )
    .order_by(OrderBy::new().desc(UserQuery::COL_CREATED_AT))
    .limit(50);
```

### 排序 builder

建议提供结构化 `OrderBy`（避免直接暴露 `order_by("created_at desc")` 的注入风险）：

```rust
pub enum SortDir { Asc, Desc }
pub enum NullsOrder { First, Last }

pub struct OrderBy {
    items: Vec<OrderItem>,
}

pub struct OrderItem {
    pub column: String,
    pub dir: SortDir,
    pub nulls: Option<NullsOrder>,
}
```

并提供：

- `OrderBy::asc("col")` / `desc("col")`
- 可选：`OrderBy::raw("...")`（明确为 escape hatch）
- 对 `column` 做 `Sql::push_ident` 同等校验（`[A-Za-z_][A-Za-z0-9_]*`，支持 dotted ident）

### Pagination builder

分页建议统一为一个小结构体并提供 `apply(&mut Sql)`：

```rust
pub struct Pagination {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
```

最终由 `Sql` 负责生成 `LIMIT/OFFSET` 并绑定参数，避免散落在各处。

## 5) SQL/schema 校验增强：alias/CTE/JOIN 覆盖 + 解析结果缓存

### 当前问题概括

- alias：`SELECT u.id FROM users u` 这类语句中，`u.id` 很难被正确映射到 `users`。
- JOIN：多表场景下的可见表集合、列歧义（ambiguous column）需要处理。
- CTE：`WITH t AS (...) SELECT ... FROM t` 需要至少做到“不把 CTE 当成物理表误报 missing table”。
- 运行时开销：每次 `check_sql`/lint 都可能触发一次或多次 `pg_query::parse`。

### 设计：SqlAnalysis + LRU parse cache

新增（建议在 `crates/pgorm-check` 侧实现更合适，然后 `pgorm` 复用）：

- `SqlAnalysis`：一次 parse 得到的“足够用于 lint + schema check”的结构化信息
  - statement kind
  - RangeVar 列表（schema/table/alias/location）
  - ColumnRef 列表（qualified/unqualified、location）
  - CTE 名集合
- `SqlParseCache`：LRU（key 为 SQL string 或 hash），value 为 `Arc<SqlAnalysis>`
  - 配置项：capacity、是否启用
  - 线程安全：`tokio::sync::Mutex` 或 `parking_lot::Mutex`

### 基于 analysis 的 schema 校验规则（目标行为）

1. 构建 `qualifier -> table` 映射：
   - qualifier = alias（若存在）否则为 table name
   - CTE 名出现在 FROM/JOIN 时跳过（不做 missing table）
2. 列校验：
   - `col`：在可见表中查找列名
     - 0 个匹配：MissingColumn
     - 1 个匹配：OK
     - >1 匹配：AmbiguousColumn（减少漏报）
   - `qualifier.col`：qualifier 必须已解析到表，否则报 UnknownQualifier（或降级为 warning，避免误杀）
   - `schema.table.col`：按 schema/table 解析
3. system columns：`ctid/xmin/...` 视为 always-ok，避免误报。

### SchemaRegistry 侧的落地方式

`pgorm` 内的 `SchemaRegistry::check_sql` 建议改为：

- 先走 `SqlParseCache::analyze(sql)` 获取 `SqlAnalysis`
- 再用 registry 内的 `TableSchema` 做匹配与报错
- 同时保留 lint 能力（最好也复用同一份 analysis）

并提供：

- `SchemaRegistry::with_parse_cache(config)`：允许用户配置 cache 容量（大项目可调大）
- `PgClientConfig` / `CheckedClient` 可透出 parse cache 的配置入口（避免每个 wrapper 各自建 cache）

## 兼容性与迁移

- `Sql` 的新增方法（scalar/exists/pagination/streaming）均为向后兼容。
- WHERE 表达式树：
  - 保留 `Condition` 原子 API
  - derive query builder 从 `Vec<Condition>` 内部实现迁移为 `WhereExpr`
  - 旧 `.order_by(String)` 可保留为 `raw_order_by`，同时新增结构化 `OrderBy`
- SQL 校验增强：
  - 保持默认行为不“更严格到误杀”，对不确定场景可先降级为 warning
  - 逐步提高覆盖（先 alias/JOIN，后续再补 CTE 输出列推导等高级能力）

## 分阶段实施计划（建议）

1. **Phase 1：便捷 API + 分页 helper** ✅
   - `fetch_scalar(_one/_opt/_all)`、`exists`、`limit/offset/page`
   - 已实现：`crates/pgorm/src/sql.rs`
2. **Phase 2：WhereExpr + OrderBy/Pagination builder** ✅
   - 已实现：`crates/pgorm/src/builder.rs`
   - 包含：`Ident`、`WhereExpr`、`OrderBy`、`Pagination`
3. **Phase 3：SQL 校验增强 + parse cache**
   - 先实现 analysis+cache，再替换 `SchemaRegistry::check_sql`
4. **Phase 4：流式查询**
   - 先提供基础 `StreamingClient` + `Sql::fetch_stream`
   - 再补 `PgClient/InstrumentedClient/CheckedClient` 的 streaming 集成与监控统计
5. **Phase 5：Statement cache wrapper**
   - `StatementCached<C>` + 错误驱逐/重试策略
   - 文档化与默认配置建议

## 需要解决的问题

> 说明：`pgorm` 还未发布，可在 0.1.x 阶段直接选择更干净、更一致的 API（不必为旧行为兜底）。

### 1) streaming + timeout：定义清晰的“超时”与“取消”语义

#### 目标

- streaming 下的 timeout **不应该因为业务消费/处理慢而误触发**（否则会把“业务处理”误判为“数据库慢”）。
- timeout 触发时应尽力 **释放服务端资源**（best-effort cancel），并保证连接尽可能可继续使用。

#### 决策：把 streaming timeout 定义为“拉取超时（idle timeout）”，而非“总耗时（wall timeout）”

- `idle_timeout`：**等待下一批/下一行数据到达的最长时间**（发生在 `poll_next` 期间）。
  - 优点：不受用户处理速度影响；只约束“数据库/网络停顿”。
  - 语义：如果用户在处理一行时很慢、长时间不 poll，**不会触发超时**（这是预期行为）。
- `wall_timeout`（可选高级能力）：**从 query 开始到 stream 结束的总时长上限**。
  - 默认不提供（或默认关闭），避免误杀“正常但消费慢”的场景。
  - 若业务确实需要“整体 SLA”，建议由业务侧包一层：对“消费循环”用 `tokio::time::timeout`。

#### API/配置建议

新增 streaming 专用配置（建议放在 `PgClientConfig` 或 `MonitorConfig`，但与非 streaming 的 timeout 明确区分）：

```rust
pub struct StreamConfig {
    pub idle_timeout: Option<Duration>,  // 拉取超时：每次 poll_next 等待上限
    pub cancel_on_timeout: bool,         // 超时触发后 best-effort cancel_query
    pub cancel_on_drop: bool,            // 提前 drop stream 时是否 cancel_query（可选）
}
```

默认建议：

- `idle_timeout = PgClientConfig.query_timeout`（若已配置）；否则 `None`。
- `cancel_on_timeout = true`（尽力释放服务端正在执行的 query）。
- `cancel_on_drop = false`（默认不“强 cancel”，避免误伤“只是提前停止消费但仍希望连接保持稳定”的场景；需要时用户显式开启）。

#### 取消语义（实现要点）

- 触发 timeout 时：返回 `OrmError::Timeout(idle_timeout)`，并 **结束 stream**（避免继续产出半截结果导致难以推理）。
- 若底层 `GenericClient::cancel_token()` 可用：在 timeout 分支 `spawn` 一个任务调用 `cancel_token.cancel_query(...)`（best-effort）。
- 若用户提前 drop stream：
  - 依赖底层 driver（`tokio-postgres` 的 portal/query stream）在 drop 时做清理；
  - 若启用 `cancel_on_drop`，同样 best-effort cancel（不保证一定成功）。

### 2) prepared statement 的“泛化计划（generic plan）”风险：提供策略、覆盖面与可回退

#### 风险本质（为什么会退化）

在 PostgreSQL 中，prepared statement 的计划缓存可能在某些场景下倾向于 **generic plan**（对参数值不敏感）。
当数据分布极度倾斜、并且参数值决定了是否走索引/走分区裁剪时，generic plan 可能比“每次按参数生成 custom plan”更差。

#### 决策：prepared + statement cache 默认关闭；启用后用“策略 + 覆盖”控制

1. **默认关闭**（0.1.x 先保证可预期）：`StatementCacheConfig.enabled = false`。
2. 启用后必须有可控策略：
   - 按 `QueryType`（SELECT/INSERT/UPDATE/DELETE/Other）开关；
   - allowlist/denylist（按 `tag` 或按 SQL pattern）；
   - 每条查询可覆写（强制启用/禁用）。

#### 建议的策略形态

```rust
pub enum PrepareMode {
    Auto,          // 默认策略
    ForcePrepared, // 强制 prepared
    ForceSimple,   // 强制不 prepared（simple query / 直跑 SQL）
}

pub struct StatementCacheConfig {
    pub enabled: bool,
    pub capacity: usize,
    pub retry_on_invalid: bool,
    pub mode: PrepareMode, // 全局默认
    pub allow_query_types: [bool; 5], // Select/Insert/Update/Delete/Other
    pub allow_tags: Vec<String>,      // 命中则强制 prepared（可选）
    pub deny_tags: Vec<String>,       // 命中则强制不 prepared（可选）
}
```

并提供“每条查询覆写”的入口（不靠改 SQL）：

- `Sql::prepare_mode(PrepareMode)`（builder 级别）
- 或 `QueryContext` 增加字段：`prepare_mode: PrepareMode`（wrapper 内透传）

#### 回退策略（性能/正确性）

- **正确性回退**（必须做）：遇到典型 prepared 失效错误，驱逐 cache entry 后重试一次。

  > **失效错误的识别必须基于 SQLSTATE，不要用 error message 字符串匹配。**
  >
  > 需要处理的 SQLSTATE：
  > - `0A000`：`feature_not_supported`（`cached plan must not change result type`）
  > - `42P18`：`indeterminate_datatype`
  > - `42804`：`datatype_mismatch`（plan/type 不一致）
  > - `42P01`：`undefined_table`（表被 DROP 后 cached plan 引用的表不存在）
  > - `42703`：`undefined_column`（schema 变更导致列不存在）
  >
  > 在 `tokio-postgres` 中，通过 `err.as_db_error().map(|e| e.code())` 获取 SQLSTATE code。
- **性能回退**（尽量提供、但不强依赖自动检测）：
  - 由用户通过 `deny_tags/denylist` 快速关掉热点问题 SQL；
  - 文档中明确提示：若怀疑 generic plan 退化，先对该查询 `ForceSimple`（或全局关闭），再做对比分析。

### 3) hook ModifySql 与 statement cache/统计：拆分“语义变更”与“观测打标”

#### 问题

- 通过 hook 给 SQL 加注释/trace tag（`ModifySql`）会导致：
  - statement cache key 被打散（命中率下降，甚至“每次都新 prepare”）；
  - 指标聚合维度变碎（按 SQL 字符串聚合时会变成多条）。

#### 决策：观测信息走结构化字段；SQL 改写只用于“语义层”且必须稳定

1. **结构化 tag/fields** 作为一等能力：用于日志、metrics、trace，不依赖改 SQL。
   - `QueryContext.tag` 已存在，建议再加 `fields: BTreeMap<String, String>`（可选）。
   - 对外提供：`query_tagged(...)` / `Sql::tag("...")`（建议）/ `PgClient::..._tagged`。
2. `HookAction::ModifySql` 的定位调整：
   - **不用于打标**（注释/trace id 禁止走 ModifySql）；
   - 仅用于“语义改写”（例如 multi-tenant 自动追加谓词、自动注入 `SET LOCAL ...` 等），且改写必须是 **可缓存、可重复** 的稳定函数（不要把 request_id/user_id 直接拼到 SQL 文本里）。

#### cache key / metrics key 的统一规则（重要）

为了避免“执行 SQL 与聚合 key 不一致”的混乱，建议在 runtime 里引入两个概念：

- `canonical_sql`：用于 **statement cache key** + **指标聚合 key**（稳定、可复用）
- `exec_sql`：最终发给 PostgreSQL 的 SQL（允许包含非语义注释，但不建议）

规则：

1. 默认 `canonical_sql == exec_sql`。
2. 如果需要“只为观测而变更 SQL”（例如加固定注释），必须保证：
   - 注释内容是 **低基数且稳定**（例如 service 名、模块名），并且
   - `canonical_sql` 仍保持原值（statement cache 仍按 canonical 复用）。

实现层面可通过（择一）：

- 改造 hook 返回值：`ModifySql { exec_sql, canonical_sql: Option<String> }`
- 或拆分 hook：`RewriterHook`（产生 canonical_sql）与 `ObserverHook`（只写 tag/fields，不动 SQL）

### 4) WHERE/ORDER ident 校验：支持 dotted + quoted ident，并提供明确的 escape hatch

#### 目标

- builder API 默认不允许注入：动态列名/表名必须被验证或被安全地引用（quoted）。
- 支持常见用法：`schema.table`、`table.column`、以及引用标识符（`"CamelCase"` / `"weird-name"`）。

#### 决策：引入 `Ident` 结构类型，替换"到处传 String"

> **与现有 `Sql::push_ident` 的关系和迁移策略**
>
> 现有 `Sql::push_ident(&str)` 只允许未引用标识符（`[A-Za-z_][A-Za-z0-9_]*`，不含 `$`），
> 而新的 `Ident` 支持 quoted 标识符和 `$`。两者规则不一致。
>
> 最终以 `Ident` 为准。迁移策略：
> 1. 新增 `Sql::push_safe_ident(&Ident)` 方法，接受 `Ident` 类型。
> 2. 旧 `Sql::push_ident(&str)` 标记为 `#[deprecated]`，内部改为调用 `Ident::parse()` + `push_safe_ident()`。
> 3. `OrderBy`、`WhereExpr` 等 builder 内部统一使用 `Ident`（当前 `OrderBy` 的 column 字段为 `String`，通过 `Ident::parse` 校验）。

建议在 `pgorm` 内新增（已实现于 `crates/pgorm/src/builder.rs`）：

```rust
pub struct Ident {
    pub parts: Vec<IdentPart>,
}

pub enum IdentPart {
    Unquoted(String), // 校验 [A-Za-z_][A-Za-z0-9_]*（可选允许 $）
    Quoted(String),   // 允许任意字符（除 NUL），输出时按 SQL 规则转义双引号
}
```

并提供：

- `Ident::parse(r#"public.users"#)` / `Ident::parse(r#""CamelCase"."User""#)`（解析 dotted + quoted）
- `Sql::push_ident(&Ident)`（统一输出）
- `OrderBy::asc(Ident)` / `Condition::eq(Ident, v)`（WHERE/ORDER 不再直接接收 `&str column`）

#### 解析/校验规则（建议最小可用）

- dotted：以 `.` 分隔，但 `.` 不允许出现在 quoted 片段之外；不允许空片段（禁止 `..`、前后 `.`）。
- Unquoted：只接受 ASCII 安全子集（`_`、字母数字、可选 `$`），首字符必须是 `_` 或字母。
- Quoted：接受 SQL 标准 quoted identifier（`"..."`），内部 `""` 表示一个 `"`；拒绝未闭合引号与 NUL。
- 明确 escape hatch：
  - `OrderBy::raw("...")` / `WhereExpr::Raw("...")` 继续存在，但名字上强调 `raw`，并在文档标记为危险 API。
