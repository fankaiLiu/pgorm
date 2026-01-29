# pgorm 设计文档：pgorm-derive 拆分与模块化计划（不改功能）

- 状态：Plan（可分阶段落地）
- 目标版本：0.1.x
- 最后更新：2026-01-29
- 范围：`crates/pgorm-derive/src`（重点：`insert_model.rs` / `update_model.rs` / `model.rs`）

> 核心目标：**功能不变、对外 API 形态不变**，只做内部“拆文件/拆模块/理边界”，降低单文件复杂度，提升可维护性与可测试性。

---

## 0. 背景（当前痛点）

当前 `pgorm-derive` 的主要实现集中在 3 个超大文件里：

- `insert_model.rs`（~2761 行）：包含 attrs 解析 + graph 声明解析 + 生成 insert/upsert/returning + 生成 insert_graph（多表写入）
- `update_model.rs`（~1288 行）：包含 attrs 解析 + graph 声明解析 + 生成 patch update + 生成 update_graph（子表策略）
- `model.rs`（~1518 行）：包含 attrs 解析 + join/relations 解析 + 查询构造器生成（Query struct）+ 生成 SQL registry 辅助

问题：

- 单文件承载过多职责（解析/语义/生成混在一起），改动易引入回归；
- 复用困难（Option/Vec 类型提取、attrs 合并等逻辑在多个宏里重复/形态不一致）；
- 定位问题成本高（阅读路径长、函数跨度大）。

---

## 1. 总体原则（约束）

### 1.1 对外不变

- `crates/pgorm-derive/src/lib.rs` 的 `#[proc_macro_derive(...)]` 函数签名与导出行为不变。
- `*_model::expand(DeriveInput) -> syn::Result<TokenStream>` 入口不变（调用方仍是 `lib.rs`）。
- 宏生成的公开 API（方法名/返回值类型/trait bound/语义）不变。

### 1.2 改动类型限制

- 允许：移动代码到子模块、提取函数、提取中间结构、调整 `use`、收紧/放宽内部可见性（如 `pub(super)`）。
- 尽量避免：改错误信息文案、改 token 生成逻辑、改 SQL 文案/顺序、改边界条件。

### 1.3 模块形态选择（不改 `lib.rs` 的 `mod xxx;`）

保持 `src/insert_model.rs` / `src/update_model.rs` / `src/model.rs` 作为“门面（facade）”文件：

- 在这些文件内部用 `mod attrs; mod gen; ...` 引入子模块；
- 新增子模块文件放在同名目录下，例如：
  - `src/insert_model/attrs.rs`
  - `src/update_model/gen_graph.rs`
  - `src/model/query.rs`

这样 **无需** 把 `insert_model.rs` 改成 `insert_model/mod.rs`，也就避免牵一发而动全身。

---

## 2. 目标结构（建议）

> 目标是“职责分层”：`parse/attrs`（syn 解析）→ `ir`（中间数据）→ `gen`（quote 输出）。

### 2.1 InsertModel（建议拆分）

```
crates/pgorm-derive/src/
  insert_model.rs                # 只保留 expand + orchestrate（门面）
  insert_model/
    attrs.rs                     # StructAttrs/FieldAttrs + get_*_attrs
    graph_decl.rs                # GraphDeclarations + HasRelation/BelongsTo/InsertStep + enums
    graph_parse.rs               # parse_graph_attr/parse_has_relation/parse_belongs_to/parse_insert_step
    gen_base.rs                  # insert/insert_many/upsert/returning 的生成
    gen_graph.rs                 # insert_graph_* 的生成（含 generate_* helpers）
    setters.rs                   # with_* setters + Option/Vec inner type helpers
```

### 2.2 UpdateModel（建议拆分）

```
crates/pgorm-derive/src/
  update_model.rs                # expand + orchestrate（门面）
  update_model/
    attrs.rs                     # StructAttrs/FieldAttrs + get_*_attrs
    graph_decl.rs                # UpdateGraphDeclarations/HasManyUpdate/HasOneUpdate/UpdateStrategy
    graph_parse.rs               # parse_update_graph_attr/parse_has_many_update/parse_has_one_update
    gen_base.rs                  # update_by_id/update_by_ids/*_returning
    gen_graph.rs                 # update_by_id_graph*（含 children dispatch）
    gen_children.rs              # generate_has_many_update_code / generate_has_one_update_code
    types.rs                     # option_inner 等类型分析小工具（若不放 common）
```

### 2.3 Model（建议拆分）

```
crates/pgorm-derive/src/
  model.rs                       # expand + orchestrate（门面）
  model/
    attrs.rs                     # get_table_name/get_field_info/is_id_field/FieldAttr
    join.rs                      # join attr 解析 + JOIN_CLAUSE 生成
    relations.rs                 # has_many/belongs_to 解析 + methods 生成
    query.rs                     # generate_query_struct（内部再拆 filtering/ordering/paging/execution）
    gen.rs                       # 组合各块生成最终 TokenStream（或把 orchestration 留在 model.rs）
```

### 2.4 可选：common（后置优化，先不做也可以）

当 Insert/Update/Model 都完成拆分后，如果发现“重复工具函数”仍然多，再做一次小收敛：

```
crates/pgorm-derive/src/
  common/
    syn_types.rs                 # option_inner/vec_inner/parse_path_from_lit 等
    errors.rs                    # 常用错误构造（保持文案不变）
```

---

## 3. 分阶段 TODO（带检查点）

> 每个阶段都要求“可编译、可回滚”。建议每阶段单独 commit，便于 `git bisect`。

### Phase 0：基线与护栏（不动逻辑）

- [x] 建一个工作分支（可选，但推荐）。
- [x] 确认当前 workspace 可编译（作为 baseline）。
- [x] 记录当前文件行数（方便拆分后对比体积变化）。

**检查点（必须过）**

- [x] `cargo test --workspace --no-run`
- [x] `cargo check -p pgorm-derive`
- [ ] （可选）`cargo fmt --all --check`

---

### Phase 1：InsertModel 拆分（优先做，体积最大、收益最高）

#### 1.1 搭骨架（只新增模块，不改实现）

- [x] 新建目录 `crates/pgorm-derive/src/insert_model/`
- [x] 在 `crates/pgorm-derive/src/insert_model.rs` 声明空模块：
  - [x] `mod attrs; mod graph_decl; mod graph_parse; mod gen_base; mod gen_graph; mod setters;`
- [x] 确保 `cargo check -p pgorm-derive` 仍然通过（此时子模块可为空或只放占位代码）。

**检查点**

- [x] `cargo check -p pgorm-derive`

#### 1.2 先搬"纯数据结构"（最安全）

- [x] 把 graph 相关 struct/enum 搬到 `insert_model/graph_decl.rs`
  - [x] `GraphDeclarations/HasRelation/BelongsTo/InsertStep`
  - [x] `HasRelationMode/BelongsToMode/StepMode`
- [x] 调整可见性为 `pub(super)` 或 `pub(crate)`（仅在 insert_model 内部用到的尽量收紧）。

**检查点**

- [x] `cargo check -p pgorm-derive`
- [x] `cargo test --workspace --no-run`

#### 1.3 搬 attrs 解析（StructAttrs/FieldAttrs 合并逻辑）

- [x] 搬到 `insert_model/attrs.rs`：
  - [x] `StructAttrs/StructAttrList`
  - [x] `FieldAttrs`
  - [x] `get_struct_attrs/get_field_attrs`
- [x] 保持错误文案与触发条件不变（重点：缺 table、冲突约束等）。

**检查点**

- [x] `cargo check -p pgorm-derive`
- [x] `cargo test --workspace --no-run`

#### 1.4 搬 graph 声明解析（parse_*）

- [x] 搬到 `insert_model/graph_parse.rs`：
  - [x] `parse_graph_attr`
  - [x] `parse_has_relation / HasRelationAttr`
  - [x] `parse_belongs_to / BelongsToAttr`
  - [x] `parse_insert_step / InsertStepAttr`
  - [x] `extract_string_value`
- [x] 注意：当前实现里 `tokens.to_string()` 的分发逻辑较脆弱，阶段内只搬不改。

**检查点**

- [x] `cargo check -p pgorm-derive`
- [x] `cargo test --workspace --no-run`

#### 1.5 搬 setters 与 type helpers

- [x] 搬到 `insert_model/setters.rs`：
  - [x] `generate_with_setters`
  - [x] `extract_option_inner_type / extract_vec_inner_type`

**检查点**

- [x] `cargo check -p pgorm-derive`
- [x] `cargo test --workspace --no-run`

#### 1.6 搬生成逻辑（base → graph）

- [ ] `insert_model/gen_base.rs`：与 graph 无关的 insert/upsert/returning 生成
- [ ] `insert_model/gen_graph.rs`：`generate_insert_graph_methods` + `generate_*` helpers
- [ ] `insert_model.rs` 只做 orchestrate：parse → 组装 → quote 输出

**检查点（必须过）**

- [ ] `cargo test --workspace --no-run`
- [ ] （可选）`cargo fmt --all --check`

---

### Phase 2：UpdateModel 拆分

#### 2.1 搭骨架

- [x] 新建目录 `crates/pgorm-derive/src/update_model/`
- [x] 在 `crates/pgorm-derive/src/update_model.rs` 声明子模块：
  - [x] `mod attrs; mod graph_decl; mod graph_parse; mod gen_base; mod gen_graph; mod gen_children; mod types;`

**检查点**

- [x] `cargo check -p pgorm-derive`

#### 2.2 搬数据结构 + attrs + parse

- [x] `update_model/graph_decl.rs`：`UpdateGraphDeclarations/*Update/UpdateStrategy`
- [x] `update_model/attrs.rs`：`StructAttrs/FieldAttrs/get_*_attrs`
- [x] `update_model/graph_parse.rs`：`parse_update_graph_attr/parse_has_many_update/parse_has_one_update`

**检查点**

- [x] `cargo test --workspace --no-run`

#### 2.3 搬生成（base → graph → children）

- [x] `update_model/types.rs`：`option_inner`（以及其它小型类型工具，如果有）
- [ ] `update_model/gen_base.rs`：`update_by_id/update_by_ids/*_returning`
- [ ] `update_model/gen_children.rs`：has_many/has_one 子表生成
- [ ] `update_model/gen_graph.rs`：`generate_update_graph_methods`（组合 children 生成）

**检查点（必须过）**

- [ ] `cargo test --workspace --no-run`
- [ ] （可选）`cargo fmt --all --check`

---

### Phase 3：Model 拆分（Query struct 最大块）

#### 3.1 搭骨架

- [x] 新建目录 `crates/pgorm-derive/src/model/`
- [x] 在 `crates/pgorm-derive/src/model.rs` 声明子模块：
  - [x] `mod attrs; mod join; mod relations; mod query;`（`gen` 可选）

**检查点**

- [x] `cargo check -p pgorm-derive`

#### 3.2 先拆 Query（风险可控、收益最高）

- [ ] 把 `generate_query_struct` 迁到 `model/query.rs`
- [ ] 在 `model/query.rs` 内再细分为小函数（只拆函数，不改行为）：
  - [ ] `gen_column_consts`
  - [ ] `gen_filtering_methods`
  - [ ] `gen_ordering_methods`
  - [ ] `gen_pagination_methods`
  - [ ] `gen_execution_methods`

**检查点**

- [ ] `cargo test --workspace --no-run`

#### 3.3 拆 attrs / join / relations

- [ ] `model/attrs.rs`：`get_table_name/get_field_info/is_id_field/FieldAttr`
- [ ] `model/join.rs`：`JoinAttr + get_join_clauses`
- [ ] `model/relations.rs`：`HasManyAttr/BelongsToAttr + get_*_relations + methods 生成`

**检查点（必须过）**

- [ ] `cargo test --workspace --no-run`
- [ ] （可选）`cargo fmt --all --check`

---

### Phase 4：可选收敛（common 工具函数）

> 只有当重复明显且确认“不会影响行为/错误信息”时才做。

- [ ] 抽出 `Option/Vec` inner type 提取到 `common/syn_types.rs`
- [ ] 抽出 litstr → syn::Path 的 parse 工具（Insert/Update 里都有）
- [ ] 清理各模块间 `use`，让每个子模块的依赖更直观

**检查点**

- [ ] `cargo test --workspace --no-run`

---

### Phase 5：收尾（可读性与维护体验）

- [ ] `insert_model.rs/update_model.rs/model.rs` 顶部加“模块说明”注释（简短）
- [ ] 统一子模块命名（`attrs/graph_* / gen_* / types`）
- [ ] `pub(crate)` / `pub(super)` 可见性收紧，避免内部类型泄漏
- [ ] 更新/补充相关 design doc 的索引（如有）

**检查点（必须过）**

- [ ] `cargo test --workspace --no-run`
- [ ] （可选）`cargo fmt --all --check`

---

## 4. 验收清单（Definition of Done）

- [ ] `crates/pgorm-derive/src/insert_model.rs` / `update_model.rs` / `model.rs` 体积显著下降（各自主要只剩“入口 + orchestrate”）
- [ ] `cargo test --workspace --no-run` 通过
- [ ] 新增子模块边界清晰：解析/声明/生成分层明确
- [ ] 没有对外 API 变更（方法名、trait bound、返回类型、SQL 文案/结构保持一致）

---

## 5. 风险点与注意事项（只列要紧的）

- `tokens.to_string()` 分发逻辑（Insert/Update 的 graph parse）短期不建议改；本计划只搬迁，避免行为差异。
- `insert_graph` 的 direct `T` 字段支持目前仍有“未完成分支”（保留/搬迁时避免误删或改控制流）。
- 错误信息文案（`syn::Error::new_spanned` 的字符串）尽量保持原样，否则下游可能依赖错误提示进行定位。

