# pgorm CRUD Derive 设计文档（草案）

> 目标：在现有 `#[derive(Model)]`（读/查询能力）基础上，补齐 Insert / Update / Delete 的宏生成方法，并引入更清晰的“读模型（ViewModel）”与“写模型（InsertModel/UpdateModel）”分层；插入/更新可返回 ViewModel（含 JOIN 的 view）结果。

## 1. 背景与现状

目前 `crates/pgorm-derive/src/model.rs` 生成的能力以 **查询** 为主：

- `TABLE / ID / SELECT_LIST / JOIN_CLAUSE`
- `select_all / select_one`
- 关系：`has_many / belongs_to`
- 动态查询：`{Model}Query`

但缺少写操作的“开箱即用”体验（INSERT / UPDATE / DELETE），开发者通常要手写 SQL 或使用 builder。

你提到的方向：

- 把现在的 `Model` 看作 **ViewModel（只读）**
- 新增 **InsertModel**（可插入、可返回）
- 补齐 **Update / Delete**

这份文档给出可落地的宏与 API 设计。

---

## 2. 设计目标

**核心目标**

1. **分层清晰**：ViewModel 负责读（可 JOIN），InsertModel/UpdateModel 负责写（单表）。
2. **写操作可选返回**：`insert_returning / update_returning / delete_returning` 支持返回 ViewModel（含 JOIN 的 view）。
3. **安全默认**：UPDATE/DELETE 必须带 WHERE（按主键）；避免生成“全表更新/删除”。
4. **兼容性**：保持现有 `#[derive(Model)]` 行为不变；新增能力不破坏现有代码。

**非目标（暂不做）**

- 自动迁移/DDL
- 复杂的变更追踪（ActiveRecord save/diff）
- 跨表级联写入（关系自动保存）
- 多表 INSERT/UPDATE（除 view returning 外）

---

## 3. 术语与模型分层

### 3.1 ViewModel（读模型）

- 对应一行“查询结果”
- **允许 JOIN**（view pattern）
- 必须 `#[orm(table = "...")]`
- 可选 `#[orm(join(...))]`
- 需标注主键：`#[orm(id)]`（如果要生成 by-id 更新/删除）

> 兼容：继续使用 `#[derive(Model)]`；另外提供别名 `#[derive(ViewModel)]`（两者生成代码一致）。

### 3.2 InsertModel（写模型：插入）

- 对应一次 INSERT 的 payload
- **必须是单表**（不允许 join/table=其它表）
- 通过 `#[orm(table = "...")]` 指定目标表
- 可选 `#[orm(returning = "Path")]` 指定返回类型（通常为 ViewModel）

### 3.3 UpdateModel（写模型：更新）

推荐用“补丁（patch）结构体”表示更新字段集合：

- 目标表：`#[orm(table = "...")]`
- 更新字段通常为 `Option<T>`：`Some(v)` 表示更新，`None` 表示跳过
- 如果要“显式写 NULL”，推荐 `Option<Option<T>>`：
  - `None`：跳过该列
  - `Some(None)`：把列设为 NULL
  - `Some(Some(v))`：把列设为 v

### 3.4 Delete（删除）

建议由 ViewModel 提供 `delete_by_id`，并可选 `delete_returning`（可返回 ViewModel）。

---

## 4. 宏与属性设计

### 4.1 ViewModel / Model（只读）

**现有**：`#[derive(Model)]` + `#[orm(table="...")]`、`#[orm(id)]`、`#[orm(column="...")]`、`#[orm(join(...))]` 等。

**新增（建议）**

- `#[proc_macro_derive(ViewModel, attributes(orm))]`：实现为 `model::expand` 的别名，文档层面推荐用 ViewModel 表达语义；`Model` 保持兼容。

### 4.2 InsertModel

```rust
#[derive(InsertModel)]
#[orm(table = "products", returning = "ProductView")]
struct NewProduct {
    name: String,
    price_cents: i64,
    #[orm(column = "category_id")]
    category: i64,
    // 可选：#[orm(skip_insert)] / #[orm(default)]
}
```

**字段属性（建议）**

- `#[orm(column = "...")]`：列名映射
- `#[orm(skip_insert)]`：永不插入该字段（例如客户端不允许写）
- `#[orm(default)]`：使用数据库默认值（生成 SQL `DEFAULT`）

> `#[orm(id)]` 在 InsertModel 中允许出现但默认 **不参与 insert**（常见自增主键）。

### 4.3 UpdateModel

```rust
#[derive(UpdateModel)]
#[orm(table = "products", returning = "ProductView")]
struct ProductPatch {
    name: Option<String>,
    price_cents: Option<i64>,
    // nullable 列要支持“设 NULL”，可用 Option<Option<T>>
    brand_id: Option<Option<i64>>,
}
```

**字段属性（建议）**

- `#[orm(column = "...")]`：列名映射
- `#[orm(skip_update)]`：永不更新该字段
- `#[orm(default)]`：更新为 DEFAULT（生成 `col = DEFAULT`）

**主键输入方式（两种方案，择一优先）**

1. **推荐**：由方法参数传入 id：
   - `patch.update_by_id(conn, id).await?`
2. UpdateModel 内自带 id 字段：
   - `#[orm(id)] id: i64`，然后 `patch.update(conn).await?`

---

## 5. 生成的 API（建议）

> 下面所有 `conn` 统一使用 `&impl pgorm::GenericClient`，返回 `pgorm::OrmResult<_>`。

### 5.1 InsertModel 生成的方法

当未指定 `returning`：

- `insert(self, conn) -> OrmResult<u64>`：返回影响行数（消费 `self`，避免对字段值额外 Clone）

当指定 `#[orm(returning = "T")]`：

- `insert_returning(self, conn) -> OrmResult<T>`

可选补充：

- `insert_returning_opt(&self, conn) -> OrmResult<Option<T>>`（极少用，通常 INSERT 要么成功要么报错）

### 5.2 UpdateModel 生成的方法

**前置约束（需要能定位主键列）**

- UpdateModel 必须提供主键列名来源之一：
  - `#[orm(id_column = "id")]`（显式给出列名），或
  - `#[orm(model = "User")]`（复用 ViewModel/Model 上的 `User::ID`），或
  - `#[orm(returning = "UserView")]` 且 `UserView` 含 `ID` 常量（即 view 上标了 `#[orm(id)]`）

当未指定 `returning`：

- `update_by_id(self, conn, id: impl ToSql) -> OrmResult<u64>`

当指定 `#[orm(returning = "T")]`：

- `update_by_id_returning(self, conn, id: impl ToSql) -> OrmResult<T>`

**重要约束**

- 如果 patch 里没有任何字段需要更新：
  - 方案 A：直接返回 `Ok(0)`（不发 SQL）
  - 方案 B：返回 `Err(OrmError::Validation("no fields to update"))`
  - 推荐 B（更早暴露 bug）

### 5.3 ViewModel 生成的删除方法

- `delete_by_id(conn, id) -> OrmResult<u64>`
- `delete_by_id_returning(conn, id) -> OrmResult<Self>`（或返回指定类型）

> `delete_returning` 是否返回“view（含 JOIN）”见下一节 SQL 方案。

---

## 6. SQL 生成策略（关键）

### 6.1 参数绑定

推荐生成代码时使用 `pgorm::query()`（内部 `Vec<Box<dyn ToSql>>`），优点：

- 动态拼列/动态拼 placeholder 更简单（不受引用生命周期影响）
- 代码风格与现有 examples 一致

> 实际实现里：INSERT 适合用 `query()`（SQL 静态、placeholder 数量固定），UPDATE/DELETE patch 更适合用 `sql()`（自动维护 `$1, $2...`）。

### 6.2 返回 ViewModel（含 JOIN）的统一方案：CTE 覆盖表名

Postgres 支持 CTE：

```sql
WITH products AS (
  INSERT INTO products (...) VALUES (...) RETURNING *
)
SELECT <ProductView::SELECT_LIST>
FROM products <ProductView::JOIN_CLAUSE>;
```

关键点：CTE 名称 **刻意与表名相同**（这里都是 `products`），使得 `JOIN_CLAUSE` / `SELECT_LIST` 里写死的 `products.xxx` 仍然成立。

因此：

- **INSERT returning view**：可一条 SQL 返回 view
- **UPDATE returning view**：可一条 SQL 返回 view
- **DELETE returning view**：同样可行（删除后仍可从 CTE `products` 拿到被删行的数据，再 JOIN 其它表拿到 view 字段）

DELETE 示例：

```sql
WITH products AS (
  DELETE FROM products WHERE products.id = $1 RETURNING *
)
SELECT <ProductView::SELECT_LIST>
FROM products <ProductView::JOIN_CLAUSE>;
```

### 6.3 returning 类型的约束

当 Insert/Update/Delete 指定 `returning = "T"` 时，要求：

1. `T: pgorm::FromRow`
2. `T` 必须是由 `#[derive(Model)]`/`ViewModel` 生成的（至少要有 `TABLE / SELECT_LIST / JOIN_CLAUSE` 常量）
3. `T::TABLE` 必须与当前写入表一致（或者 join clause 里引用的“主表名”与写入表名一致）

若不满足，编译期会因为找不到常量或 trait bound 失败而报错（这是可接受的“类型级约束”）。

---

## 7. 列选择规则

### 7.1 InsertModel 的插入列

默认插入列 = 所有字段对应的列名，排除：

- `#[orm(skip_insert)]`
- `#[orm(id)]`（默认不插入）

对于 `#[orm(default)]`：

- 生成 `col = DEFAULT`（INSERT 场景是 `VALUES (..., DEFAULT, ...)`）
- 不占用参数位置

### 7.2 UpdateModel 的更新列

默认更新列 = 所有字段对应的列名，排除：

- `#[orm(skip_update)]`

并按字段类型决定是否参与：

- `Option<T>`：`Some(v)` 才生成 `col = $n`
- `Option<Option<T>>`：`Some(None)` 生成 `col = NULL`；`Some(Some(v))` 生成 `col = $n`
- `#[orm(default)]`：生成 `col = DEFAULT`

### 7.3 WHERE 与安全默认

- Update/Delete 必须有 by-id WHERE：
  - `WHERE <table>.<id_col> = $n`
  - 避免误生成全表更新/删除

---

## 8. 错误与行为约定

- 未提供 `#[orm(table="...")]`：沿用现有 compile_error（必填）
- returning 指定了类型但目标类型不满足约束：让 Rust 编译器报错（缺少 const/trait）
- UpdateModel 没有任何字段需要更新：推荐返回 `OrmError::Validation`

---

## 9. 示例（期望用户体验）

### 9.1 纯表模型

```rust
#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    price_cents: i64,
}

#[derive(Debug, InsertModel)]
#[orm(table = "products", returning = "Product")]
struct NewProduct {
    name: String,
    price_cents: i64,
}

let created: Product = NewProduct { name: "A".into(), price_cents: 100 }.insert_returning(&client).await?;
```

### 9.2 ViewModel（JOIN view）+ Insert returning view

```rust
#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
#[orm(join(table = "categories", on = "products.category_id = categories.id", type = "inner"))]
struct ProductView {
    #[orm(id, table = "products", column = "id")]
    id: i64,
    #[orm(table = "products", column = "name")]
    product_name: String,
    #[orm(table = "categories", column = "name")]
    category_name: String,
}

#[derive(Debug, InsertModel)]
#[orm(table = "products", returning = "ProductView")]
struct NewProduct {
    name: String,
    category_id: i64,
}

let view: ProductView = NewProduct { name: "A".into(), category_id: 1 }.insert_returning(&client).await?;
```

---

## 10. 实现拆分建议（代码组织）

在 `crates/pgorm-derive`：

- `src/view_model.rs`（或继续复用现有 `model.rs`）
- `src/insert_model.rs`
- `src/update_model.rs`
- （可选）`src/delete_model.rs`：如果你更喜欢把 delete 也做成单独 derive

在 `crates/pgorm-derive/src/lib.rs`：

- 新增 `#[proc_macro_derive(ViewModel, attributes(orm))]`
- 新增 `#[proc_macro_derive(InsertModel, attributes(orm))]`
- 新增 `#[proc_macro_derive(UpdateModel, attributes(orm))]`

在 `crates/pgorm`：

- 视需要新增一个小工具类型（可选）：例如 `Set<T>`/`Patch<T>`，用于“可跳过/可置 NULL/可赋值”的更好表达（如果不想用 `Option<Option<T>>`）。

---

## 11. 里程碑与迭代顺序（推荐）

1. **M1：ViewModel 别名**：新增 `ViewModel` derive（等价于现有 Model），更新 README 示例（可选）。
2. **M2：InsertModel**：支持 `insert` 和 `insert_returning`（含 JOIN view returning，使用 CTE 覆盖表名）。
3. **M3：UpdateModel**：支持 `update_by_id`（Option patch），以及 returning。
4. **M4：Delete by id**：在 ViewModel 上生成 `delete_by_id` / `delete_by_id_returning`。
5. **M5：测试**：补充 examples +（可选）trybuild 编译测试，覆盖各种属性组合与错误提示。
