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
#[cfg(feature = "stream")]
pub type RowStream<'a> =
    std::pin::Pin<Box<dyn futures_core::Stream<Item = OrmResult<tokio_postgres::Row>> + Send + 'a>>;

#[cfg(feature = "stream")]
pub trait StreamingClient: Send + Sync {
    fn query_stream<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [&'a (dyn tokio_postgres::types::ToSql + Sync)],
    ) -> RowStream<'a>;
}
```

在 `Sql` builder 上提供对等能力：

```rust
#[cfg(feature = "stream")]
impl Sql {
    pub fn fetch_stream<'a>(
        &'a self,
        conn: &'a impl StreamingClient,
    ) -> OrmResult<RowStream<'a>> { /* build + validate + delegate */ }

    pub fn fetch_stream_as<'a, T: FromRow + 'a>(
        &'a self,
        conn: &'a impl StreamingClient,
    ) -> OrmResult<impl Stream<Item = OrmResult<T>> + Send + 'a> { /* row->T map */ }
}
```

说明：

- “游标”层面不一定需要显式 `DECLARE CURSOR`：`tokio-postgres` 的 `query_raw`/portal 已能实现按批拉取并流式消费（对业务就是 cursor-like）。
- 显式 server-side cursor（`DECLARE ... CURSOR` / `FETCH n`）可作为后续增强（需要事务语义与生命周期管理）。

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

```rust
pub struct StatementCacheConfig {
    pub capacity: usize,      // LRU 容量
    pub enabled: bool,        // 开关（可用于灰度）
    pub retry_on_invalid: bool,
}

pub struct StatementCached<C> {
    inner: C,
    cfg: StatementCacheConfig,
    // key: SQL string (or hash) -> tokio_postgres::Statement
    // 典型实现：lru::LruCache<String, Statement> + tokio::sync::Mutex
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
    pub fn and(self, other: WhereExpr) -> WhereExpr { /* ... */ }
    pub fn or(self, other: WhereExpr) -> WhereExpr { /* ... */ }
    pub fn group(self) -> WhereExpr { /* 语义上等价于返回 self；build 时加括号 */ }

    pub fn build(&self, param_idx: &mut usize) -> (String, Vec<&(dyn ToSql + Sync)>) { /* 递归拼接 + 括号 */ }
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

1. **Phase 1：便捷 API + 分页 helper**
   - `fetch_scalar(_one/_opt)`、`exists`、`limit/offset/page`
2. **Phase 2：WhereExpr + OrderBy/Pagination builder**
   - 先在 `pgorm` 内提供类型，再迁移 derive query builder
3. **Phase 3：SQL 校验增强 + parse cache**
   - 先实现 analysis+cache，再替换 `SchemaRegistry::check_sql`
4. **Phase 4：流式查询**
   - 先提供基础 `StreamingClient` + `Sql::fetch_stream`
   - 再补 `PgClient/InstrumentedClient/CheckedClient` 的 streaming 集成与监控统计
5. **Phase 5：Statement cache wrapper**
   - `StatementCached<C>` + 错误驱逐/重试策略
   - 文档化与默认配置建议

## 风险与开放问题

- streaming + timeout：streaming 查询的 timeout/取消语义如何定义（选择每次 poll 超时）。
- prepared statement 的“泛化计划”风险：某些查询 prepared 后可能出现性能退化，是否需要 allowlist/denylist 或按 statement kind 控制。
- hook ModifySql 与 statement cache/统计的相互影响：，否提供“结构化 tag”替代方案并引导使用。
- WHERE/ORDER ident 校验的严格度：允许 `schema.table`、`table.column`、以及引用标识符（`"CamelCase"`）等。
