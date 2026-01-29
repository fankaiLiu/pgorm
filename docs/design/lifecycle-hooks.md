# Hook 系统（QueryHook）

> 参考来源：GORM (Go)、Ecto (Elixir)、ActiveRecord (Ruby)

pgorm 只保留一个 hook 系统：**SQL 执行生命周期 hook**（`pgorm::monitor::QueryHook`）。

- 作用域：每一条 SQL（包括手写 SQL、宏生成 SQL）
- 用途：打 tag / 加注释、阻断危险 SQL、做额外的 query 级别观测

本文会围绕这一个系统说明 API、组合方式，以及“我已经有一个监控 hook 了，怎么在不覆盖的情况下再加一个？”。

## 1) 核心 API

### 1.1 如何在已有 hook 上再加一个？

`InstrumentedClient` / `PgClient` 的 `with_hook(...)` 只接受 **一个** hook；要叠加多个：

- 用 `CompositeHook` 明确组合（适合“先组装，再注入”）
- 或用 `add_hook(...)` 在已有 hook 基础上追加（更符合“builder 追加配置”的直觉）

```rust
use pgorm::{CompositeHook, HookAction, QueryContext, QueryHook};

struct ExistingHook;
impl QueryHook for ExistingHook {}

struct AddTagHook;
impl QueryHook for AddTagHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        HookAction::ModifySql {
            exec_sql: format!("/* tag=api */ {}", ctx.exec_sql),
            // 观测打标一般不该改变 canonical_sql（避免污染 cache key / 聚合维度）
            canonical_sql: None,
        }
    }
}

let hook = CompositeHook::new().add(ExistingHook).add(AddTagHook);

// InstrumentedClient / PgClient 都可以：
// let client = pgorm::InstrumentedClient::new(db).with_hook(hook);
// let pg = pgorm::PgClient::new(db).with_hook(hook);
```

也可以直接在 client builder 上追加：

```rust
use pgorm::{HookAction, PgClient, QueryContext, QueryHook};

struct ExistingHook;
impl QueryHook for ExistingHook {}

struct AddTagHook;
impl QueryHook for AddTagHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        HookAction::ModifySql {
            exec_sql: format!("/* tag=api */ {}", ctx.exec_sql),
            canonical_sql: None,
        }
    }
}

let pg = PgClient::new(db).with_hook(ExistingHook).add_hook(AddTagHook);
```

如果你手里的是 `Arc<dyn QueryHook>`（例如在配置里预先构造好的 hook），可以这样追加：

```rust
use std::sync::Arc;
use pgorm::{PgClient, QueryHook};

let existing: Arc<dyn QueryHook> = Arc::new(ExistingHook);
let pg = PgClient::new(db).with_hook_arc(existing).add_hook(AddTagHook);
```

#### `with_hook` vs `add_hook`

- `with_hook(...)`：设置（覆盖）
- `add_hook(...)`：追加（组合：旧的先跑，新的后跑）

> 如果你确实需要“后追加”，推荐的工程做法是：把 hook 的构造放在一个函数里，统一返回 `CompositeHook`，避免散落在不同地方各自 `.with_hook(...)`。

### 1.2 多个 hook 的执行顺序与 ModifySql

`CompositeHook` 会按添加顺序执行 `before_query`：

1) `hook1.before_query(ctx)`  
2) 如果返回 `ModifySql`，会更新 `exec_sql / canonical_sql` 再传给下一个 hook  
3) 任意 hook 返回 `Abort(...)` 会立刻终止

因此：

- **“观测打标”hook**：尽量只改 `exec_sql`（加注释/trace tag），不要改 `canonical_sql`  
- **“语义改写”hook**：如果会改查询语义，应该同时给出稳定的 `canonical_sql`，避免 statement cache / metrics 维度爆炸
## 2) 与 Write Graph 的关系（避免概念冲突）

`InsertModel` 现有的 `#[orm(before_insert(...))] / #[orm(after_insert(...))]` 是 **write graph 的“额外写入步骤”**：

- 它描述的是“在 root insert 之前/之后，再插入某个关联的 InsertModel”
- 适合：审计表插入、关联表写入、跨表写入编排

它不是“对当前类型做 callback”，也不属于 hook 系统本身（只是名字上有 before/after）。

如果你要做的是：

- **同一个 struct 的字段校验/填充**：显式调用你自己的 `validate()` / `normalize()` 方法，或用专用特性（例如自动时间戳）
- **跨表写入/审计日志**：优先用 write graph step（现有），或者放进 `transaction!` 里显式写

## 3) 检查清单（强化单一 hook 系统）

- [x] `CompositeHook::add_arc(Arc<dyn QueryHook>)`：更容易把“已有 hook”并进来
- [x] `InstrumentedClient` / `PgClient` 提供 `add_hook(...)`：让“追加 hook”是显式能力，而不是靠用户手动 wrap
