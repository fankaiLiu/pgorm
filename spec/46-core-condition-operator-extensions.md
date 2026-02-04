# Condition/Op 扩展计划（更丰富的操作符与表达式）

状态：Draft  
相关代码：`crates/pgorm/src/condition.rs`、`crates/pgorm/src/builder.rs`、`crates/pgorm/src/sql.rs`  
最后更新：2026-02-03

## 背景

`pgorm::Condition` / `pgorm::Op` 目前覆盖了常用比较（eq/ne/gt/gte/lt/lte）、LIKE、NULL、IN/NOT IN、BETWEEN 等。  
在真实业务里常见的 Postgres 操作还包括：

- `IS DISTINCT FROM`（处理 NULL-safe 比较）
- array/jsonb 操作符（`@>`, `<@`, `&&`, `?`, `?|`, `?&` 等）
- `ANY/ALL`（对数组参数的比较）
- 全文检索（`to_tsvector @@ plainto_tsquery` 等）

缺少这些会导致调用端回到 `WhereExpr::Raw`/`Condition::raw`，增加 SQL 注入与一致性风险。

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

## API 草案

优先以 `Condition` 的“构造函数”形式提供（而不是急着扩展 `Op<T>`），避免 `Op<T>` 过度膨胀：

```rust,ignore
impl Condition {
    pub fn is_distinct_from<I, T>(col: I, v: T) -> OrmResult<Self>;
    pub fn is_not_distinct_from<I, T>(col: I, v: T) -> OrmResult<Self>;

    pub fn contains<I, T>(col: I, v: T) -> OrmResult<Self>;      // @>
    pub fn contained_by<I, T>(col: I, v: T) -> OrmResult<Self>;  // <@
    pub fn overlaps<I, T>(col: I, v: T) -> OrmResult<Self>;      // &&

    pub fn eq_any<I, T>(col: I, v: Vec<T>) -> OrmResult<Self>;   // = ANY($n)

    pub fn ts_match<I>(col: I, q: impl Into<String>) -> OrmResult<Self>;
}
```

## 实施计划（Milestones）

### M1（NULL-safe 比较 + array/jsonb 基础）

- [ ] 在 `condition.rs` 扩展内部 operator 支持（注意 `build()` 与 `append_to_sql()` 两套路径都要改）。
- [ ] 单元测试：输出 SQL 文本与参数数量正确。

### M2（ANY/ALL）

- [ ] 新增 `eq_any` 等 helper。
- [ ] 文档：`IN (...)` vs `= ANY($1)` 的适用场景与差异。

### M3（全文检索 MVP）

- [ ] 增加 `ts_match` helper（固定模板 + bind query）。
- [ ] example：`examples/full_text_search`（可选）。

## 风险与取舍

- 某些操作符对参数类型敏感（array/jsonb），prepared statement 场景下可能需要显式 cast；文档需明确。
- 全文检索表达式涉及函数调用，必须严格禁止用户拼接 raw 表达式，避免注入。

## Open Questions

1) 这些 helper 是放在 `Condition` 里还是 `Op` 里？（推荐先放 `Condition`，更灵活）  
2) `eq_any` 接受 `Vec<T>` 还是更泛化的 `T: ToSql`（数组类型）？  
3) jsonb 的 key 操作符（`?` 等）是否纳入 M1（取决于实际需求优先级）？
