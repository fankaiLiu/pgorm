# Condition/Op 扩展计划（更丰富的操作符与表达式）

状态：Draft（M1–M3 已落地；example/用户文档待补）  
相关代码：`crates/pgorm/src/condition.rs`、`crates/pgorm/src/builder.rs`、`crates/pgorm/src/sql.rs`  
最后更新：2026-02-04

## 背景

`pgorm::Condition` / `pgorm::Op` 目前覆盖了常用比较（eq/ne/gt/gte/lt/lte）、LIKE、NULL、IN/NOT IN、BETWEEN 等。  
在真实业务里常见的 Postgres 操作还包括：

- `IS DISTINCT FROM`（处理 NULL-safe 比较）
- array/jsonb 操作符（`@>`, `<@`, `&&`, `?`, `?|`, `?&` 等）
- `ANY/ALL`（对数组参数的比较）
- 全文检索（`to_tsvector @@ plainto_tsquery` 等）

缺少这些会导致调用端回到 `WhereExpr::Raw`/`Condition::raw`，增加 SQL 注入与一致性风险。

## 结论（TL;DR）

- 以 **新增 `Condition::*` 构造函数** 为主（不急着扩展 `Op<T>`），保持 API 简单、语义稳定。
- 对于 `IS DISTINCT FROM`、`@>/<@/&&/?/?|/?&` 这类“`<col> <op> <bind>`”形态：复用现有 `ConditionInner::Expr` 即可。
- 对于 `ANY/ALL` 与全文检索这类“模板表达式”（需要括号/函数包裹 ident/bind）：在 `ConditionInner` 内部增加一个 **受控的 template/parts 变体**（只由库内构造），同时在 `build()` 与 `append_to_sql()` 两条路径上实现一致的占位符与参数顺序。

## 目标 / 非目标

### 目标

1) 为高频 Postgres 操作提供 **安全封装**：列名走 `Ident` 校验，值走 bind。  
2) 尽量复用现有 `ConditionValue`（Single/Pair/List/None）并保持 `build()` 与 `append_to_sql()` 语义一致。  
3) 以“增量扩展”为主：先覆盖最通用的一批，避免一次性做成表达式 AST。

### 非目标

- 不实现通用表达式系统（函数、算术、CASE、复杂嵌套等）。
- 不自动注入 cast（仍然建议用户显式写 `::type` 或在 schema 侧保证类型）。
- 不覆盖所有 pg 操作符（按需求迭代）。

## 方案（分层）

### Layer 1：NULL-safe 比较

新增：
- `Condition::is_distinct_from(column, value)`
- `Condition::is_not_distinct_from(column, value)`

SQL：
- `col IS DISTINCT FROM $n`
- `col IS NOT DISTINCT FROM $n`

### Layer 2：array/jsonb 基础操作符

新增（命名可讨论）：
- `Condition::contains(column, value)` → `@>`
- `Condition::contained_by(column, value)` → `<@`
- `Condition::overlaps(column, value)` → `&&`

约束：
- value 必须是 `ToSql + 'static`，通常是 `Vec<T>` 或 `serde_json::Value`/`Json<T>`。

补充（jsonb key existence，优先级较高且实现成本低）：

- `Condition::has_key(column, key)` → `?`
- `Condition::has_any_keys(column, keys)` → `?|`（`keys` 作为 text[] bind）
- `Condition::has_all_keys(column, keys)` → `?&`（`keys` 作为 text[] bind）

> 注意：`?|/?&` 的 RHS 在 SQL 层语义是 `text[]`，Rust 侧建议用 `Vec<String>`（或由调用端自己准备数组类型）。

### Layer 3：ANY/ALL

新增：
- `Condition::eq_any(column, values)` → `col = ANY($n)`
- `Condition::ne_all(column, values)` → `col != ALL($n)`（按需）

其中 `values` 绑定为一个数组参数（`Vec<T>`），比 `IN (..)` 更适合大列表与 prepared 复用。

### Layer 4：全文检索（MVP）

全文检索不是“列 + 操作符 + 值”即可表达，建议提供专用 helper（内部生成固定表达式）：

- `Condition::ts_match(column, query)` → `to_tsvector(column) @@ plainto_tsquery($n)`
- 或支持语言：`ts_match_lang("english", column, query)`

注意：这里的 `to_tsvector(...)` 是固定模板，不允许用户注入 raw SQL。

## 关键实现点：Condition 的“模板表达式”能力（不引入通用 AST）

现状：`ConditionInner::Expr` 只能表达：

- `col <op> $n`
- `col <op> ($1, $2, ...)`
- `col BETWEEN $1 AND $2`

这不足以表达：

- `col = ANY($n)`（bind 需要被 `ANY(...)` 包裹）
- `to_tsvector(col) @@ plainto_tsquery($n)`（ident 需要被函数包裹）

因此建议在 `condition.rs` 的内部表示中新增一个“受控模板”变体（仅供库内 helper 使用），形态类似：

```rust,ignore
enum ConditionPart {
    Raw(&'static str),
    Ident(Ident),
    Param(Arc<dyn ToSql + Send + Sync>),
}

enum ConditionInner {
    Raw(String),
    Expr { column: Ident, operator: &'static str, value: ConditionValue },
    Tuple2 { /* ... */ },
    Parts(Vec<ConditionPart>), // NEW
}
```

语义：

- `build(&self, param_idx)`：按顺序遍历 `Parts`，遇到 `Param` 就 `param_idx += 1` 并写入 `$<idx>`，同时把 `&param` 放进返回的 `Vec<&ToSql>`。
- `append_to_sql(&self, sql)`：`Raw` 直接 `sql.push()`，`Ident` 用 `sql.push_ident_ref()`，`Param` 用 `sql.push_bind_value()`。

这样可以在不暴露“任意表达式拼接”的前提下，让库内以固定模板安全扩展更多 pg 操作。

## API 草案

优先以 `Condition` 的“构造函数”形式提供（而不是急着扩展 `Op<T>`），避免 `Op<T>` 过度膨胀：

```rust,ignore
impl Condition {
    pub fn is_distinct_from<I, T>(col: I, v: T) -> OrmResult<Self>;
    pub fn is_not_distinct_from<I, T>(col: I, v: T) -> OrmResult<Self>;

    pub fn contains<I, T>(col: I, v: T) -> OrmResult<Self>;      // @>
    pub fn contained_by<I, T>(col: I, v: T) -> OrmResult<Self>;  // <@
    pub fn overlaps<I, T>(col: I, v: T) -> OrmResult<Self>;      // &&

    pub fn has_key<I>(col: I, key: impl Into<String>) -> OrmResult<Self>;               // ?
    pub fn has_any_keys<I>(col: I, keys: Vec<String>) -> OrmResult<Self>;               // ?|
    pub fn has_all_keys<I>(col: I, keys: Vec<String>) -> OrmResult<Self>;               // ?&

    pub fn eq_any<I, T>(col: I, v: Vec<T>) -> OrmResult<Self>;   // = ANY($n)
    pub fn ne_all<I, T>(col: I, v: Vec<T>) -> OrmResult<Self>;   // != ALL($n)

    pub fn ts_match<I>(col: I, q: impl Into<String>) -> OrmResult<Self>;
    pub fn ts_match_lang<I>(lang: impl Into<String>, col: I, q: impl Into<String>) -> OrmResult<Self>;
}
```

## 使用示例（目标写法）

### A) NULL-safe 比较

```rust,ignore
use pgorm::{Condition, WhereExpr};

// WHERE deleted_at IS DISTINCT FROM NULL
let expr = WhereExpr::atom(Condition::is_distinct_from("deleted_at", Option::<i64>::None)?);
```

### B) jsonb contains / key existence

```rust,ignore
use pgorm::Condition;

let cond = Condition::contains("metadata", serde_json::json!({"env": "prod"}))?;
let cond2 = Condition::has_key("metadata", "user_id")?;
```

### C) = ANY($1)（替代大 IN）

```rust,ignore
use pgorm::Condition;

let user_ids = vec![1_i64, 2, 3];
let cond = Condition::eq_any("user_id", user_ids)?;
```

### D) 全文检索（MVP）

```rust,ignore
use pgorm::Condition;

let cond = Condition::ts_match("content", "hello world")?;
let cond2 = Condition::ts_match_lang("english", "content", "hello world")?;
```

> 注：如果你需要对列做 `::text` cast、拼接多个列、或用 `websearch_to_tsquery` 等更高级形式，仍然建议使用 `pgorm::Sql`（而不是 `Condition::raw`）。

## 实施计划（Milestones）

### M1（NULL-safe 比较 + array/jsonb 基础）

- [x] `Condition::is_distinct_from/is_not_distinct_from`
- [x] `Condition::contains/contained_by/overlaps`
- [x] `Condition::has_key/has_any_keys/has_all_keys`
- [x] 单元测试：输出 SQL 文本与参数数量正确。

### M2（ANY/ALL）

- [x] `ConditionInner` 增加受控模板（`Parts`）能力
- [x] `Condition::eq_any/ne_all`
- [x] 单元测试：`build()` 与 `append_to_sql()` 的 SQL/参数顺序一致
- [ ] 文档：`IN (...)` vs `= ANY($1)` 的适用场景与差异（prepared/大列表）

### M3（全文检索 MVP）

- [x] `Condition::ts_match/ts_match_lang`（固定模板 + bind query）
- [ ] example：`examples/full_text_search`（可选）。

## 风险与取舍

- 某些操作符对参数类型敏感（array/jsonb），prepared statement 场景下可能需要显式 cast；文档需明确。
- 全文检索表达式涉及函数调用，必须严格禁止用户拼接 raw 表达式，避免注入。

## 兼容性与迁移

- 全部为新增 API：不修改现有 `Condition/Op/WhereExpr/Sql` 行为，对外无 breaking change。
- 建议迁移方向：把业务侧 `WhereExpr::Raw("col = ANY(...)")` / `Condition::raw("...")` 替换为对应 helper，减少注入面与 SQL 风格不一致。

## Open Questions

1) 这些 helper 是放在 `Condition` 里还是 `Op` 里？（推荐先放 `Condition`，更灵活）  
2) `eq_any` 接受 `Vec<T>` 还是更泛化的 `T: ToSql`（数组类型）？（MVP：先 `Vec<T>`，避免生命周期与 `'static` 复杂度）  
3) jsonb 的 key 操作符（`?` 等）是否纳入 M1？（建议纳入：实现成本低、使用频率高）  
4) `ts_match_lang` 的 `lang` 是 bind 还是字面量？（建议 bind + `::regconfig` cast，避免注入与 allowlist 问题）
