# 事务增强（Savepoint 与嵌套事务）设计与计划

状态：Draft
相关代码：`crates/pgorm/src/transaction.rs` / `crates/pgorm/src/client.rs`
最后更新：2026-02-05

## 背景

当前 `pgorm` 提供了基础的事务支持（`pgorm::transaction!` 宏），但缺少：

1. **Savepoint**：在事务内创建保存点，允许部分回滚
2. **嵌套事务语义**：内层"事务"自动转换为 Savepoint

这些特性在以下场景中非常有用：
- 批量处理时单条失败不影响其他记录
- 复杂业务流程中的部分重试
- 测试中的数据隔离

## 目标 / 非目标

### 目标

1. 提供显式 Savepoint API：`savepoint()` / `release()` / `rollback_to()`。
2. 提供嵌套事务宏：内层 `transaction!` 自动使用 Savepoint。
3. 支持命名 Savepoint。
4. 错误时自动回滚到最近的 Savepoint。

### 非目标

- 分布式事务（XA）支持。
- 自动重试机制（应由业务层决定）。
- 事务隔离级别的动态切换（可通过 `SET TRANSACTION` 实现）。

## 方案

### 1) 显式 Savepoint API

```rust,ignore
impl<'a> Transaction<'a> {
    /// 创建命名保存点
    pub async fn savepoint(&mut self, name: &str) -> OrmResult<Savepoint<'_>>;

    /// 创建匿名保存点（自动命名）
    pub async fn savepoint_anon(&mut self) -> OrmResult<Savepoint<'_>>;
}

pub struct Savepoint<'a> {
    tx: &'a mut Transaction<'a>,
    name: String,
    released: bool,
}

impl<'a> Savepoint<'a> {
    /// 释放保存点（使其永久化）
    pub async fn release(mut self) -> OrmResult<()>;

    /// 回滚到此保存点
    pub async fn rollback(mut self) -> OrmResult<()>;

    /// 获取保存点名称
    pub fn name(&self) -> &str;
}

impl Drop for Savepoint<'_> {
    fn drop(&mut self) {
        // 如果既没有 release 也没有 rollback，自动 rollback
        if !self.released {
            // 记录警告日志
        }
    }
}
```

### 2) 嵌套事务宏

```rust,ignore
/// 嵌套事务：如果已在事务中，使用 Savepoint；否则开启新事务
#[macro_export]
macro_rules! nested_transaction {
    ($client:expr, async |$tx:ident| $body:expr) => { ... };
}
```

内部逻辑：
1. 检查 `client` 是否已是 `Transaction`
2. 如果是：创建 Savepoint，执行 body，成功则 release，失败则 rollback
3. 如果不是：开启新事务，执行 body

### 3) savepoint! 宏

```rust,ignore
/// 在事务内创建保存点作用域
#[macro_export]
macro_rules! savepoint {
    ($tx:expr, async |$sp:ident| $body:expr) => { ... };
    ($tx:expr, $name:expr, async |$sp:ident| $body:expr) => { ... };
}
```

## 使用示例

### A) 显式 Savepoint

```rust,ignore
use pgorm::prelude::*;

pgorm::transaction!(client, async |tx| {
    // 插入订单
    let order = NewOrder { user_id: 1, total: 100 }.insert_returning(tx).await?;

    // 创建保存点
    let sp = tx.savepoint("before_items").await?;

    // 尝试插入订单项
    match insert_order_items(tx, order.id, &items).await {
        Ok(_) => {
            sp.release().await?;  // 成功，释放保存点
        }
        Err(e) => {
            sp.rollback().await?;  // 失败，回滚到保存点
            // 订单仍然存在，只是没有订单项
            log::warn!("Failed to insert items: {}", e);
        }
    }

    Ok(())
});
```

### B) 批量处理（单条失败不影响整体）

```rust,ignore
pgorm::transaction!(client, async |tx| {
    let mut success_count = 0;
    let mut failed_ids = vec![];

    for record in records {
        // 每条记录使用独立的 savepoint
        let sp = tx.savepoint_anon().await?;

        match process_record(tx, &record).await {
            Ok(_) => {
                sp.release().await?;
                success_count += 1;
            }
            Err(e) => {
                sp.rollback().await?;
                failed_ids.push(record.id);
                log::warn!("Failed to process record {}: {}", record.id, e);
            }
        }
    }

    println!("Processed {} records, {} failed", success_count, failed_ids.len());
    Ok(())
});
```

### C) 嵌套事务宏

```rust,ignore
// 外层事务
pgorm::transaction!(client, async |tx| {
    create_user(tx, &user_data).await?;

    // 内层"事务"自动使用 Savepoint
    pgorm::nested_transaction!(tx, async |inner| {
        create_user_profile(inner, &profile_data).await?;
        create_user_settings(inner, &settings_data).await?;
        Ok(())
    })?;

    Ok(())
});

// 如果从非事务上下文调用，则开启新事务
async fn create_user_with_profile<C: GenericClient>(client: &C, data: &UserData) -> OrmResult<User> {
    pgorm::nested_transaction!(client, async |tx| {
        let user = create_user(tx, data).await?;
        create_user_profile(tx, &user).await?;
        Ok(user)
    })
}
```

### D) savepoint! 宏

```rust,ignore
pgorm::transaction!(client, async |tx| {
    // 主要操作
    let order = create_order(tx, &order_data).await?;

    // 使用 savepoint 宏
    let notification_result = pgorm::savepoint!(tx, "notify", async |sp| {
        send_notification(sp, order.user_id).await?;
        update_notification_log(sp, order.id).await?;
        Ok(())
    });

    // 通知失败不影响订单
    if let Err(e) = notification_result {
        log::warn!("Notification failed: {}", e);
    }

    Ok(order)
});
```

### E) 多层嵌套 Savepoint

```rust,ignore
pgorm::transaction!(client, async |tx| {
    // Level 0: 事务开始

    let sp1 = tx.savepoint("level1").await?;
    // Level 1

    {
        let sp2 = tx.savepoint("level2").await?;
        // Level 2

        // 假设这里失败
        if some_condition {
            sp2.rollback().await?;  // 回滚到 level1
        } else {
            sp2.release().await?;
        }
    }

    // 仍在 level1
    sp1.release().await?;

    Ok(())
});
```

## 生成的 SQL

### savepoint()

```sql
SAVEPOINT before_items;
```

### release()

```sql
RELEASE SAVEPOINT before_items;
```

### rollback()

```sql
ROLLBACK TO SAVEPOINT before_items;
```

## API 设计

### Transaction 方法

| 方法 | SQL | 说明 |
|------|-----|------|
| `savepoint(name)` | `SAVEPOINT name` | 创建命名保存点 |
| `savepoint_anon()` | `SAVEPOINT sp_N` | 创建匿名保存点（自动编号） |

### Savepoint 方法

| 方法 | SQL | 说明 |
|------|-----|------|
| `release()` | `RELEASE SAVEPOINT name` | 释放保存点 |
| `rollback()` | `ROLLBACK TO SAVEPOINT name` | 回滚到保存点 |
| `name()` | - | 获取保存点名称 |

### 宏

| 宏 | 语义 |
|-----|------|
| `transaction!` | 开启事务（已有） |
| `nested_transaction!` | 嵌套事务（自动选择事务或 Savepoint） |
| `savepoint!` | 在事务内创建 Savepoint 作用域 |

## 关键取舍

| 选项 | 优点 | 缺点 | 决策 |
|------|------|------|------|
| 显式 API vs 自动嵌套 | 更清晰 | 需要用户主动调用 | **两者都提供** |
| Drop 时自动 rollback vs panic | 安全 | 可能掩盖错误 | **自动 rollback + 警告** |
| 命名 vs 匿名 Savepoint | 命名更易调试 | 需要用户提供名称 | **两者都支持** |

## 与 GenericClient 的集成

`Savepoint` 实现 `GenericClient`，允许在 Savepoint 作用域内使用所有查询方法：

```rust,ignore
impl GenericClient for Savepoint<'_> {
    // 委托给内部的 Transaction
}
```

## 兼容性与迁移

- `transaction!` 宏行为不变。
- 新增 `nested_transaction!` 和 `savepoint!` 宏。
- 新增 `Transaction::savepoint()` 方法。

## 里程碑 / TODO

### M1（显式 Savepoint API）

- [ ] `Transaction::savepoint()` / `savepoint_anon()`
- [ ] `Savepoint` 结构体
- [ ] `release()` / `rollback()` 方法
- [ ] `Savepoint` 实现 `GenericClient`
- [ ] Drop 时自动 rollback
- [ ] 单元测试

### M2（嵌套事务宏）

- [ ] `nested_transaction!` 宏
- [ ] 自动检测事务上下文
- [ ] `savepoint!` 宏
- [ ] 集成测试

### M3（文档与示例）

- [ ] `examples/savepoint`
- [ ] `examples/nested_transaction`
- [ ] 中英文文档

## Open Questions

1. `Savepoint` Drop 时是否应该 panic 而不是静默 rollback？（建议警告 + rollback）
2. 是否需要 `savepoint_with_options()` 支持更多 PostgreSQL 选项？（建议 M1 不需要）
3. `nested_transaction!` 的错误传播语义：内层错误是否自动回滚外层？（建议只回滚内层 Savepoint）
4. 是否提供 `tx.with_savepoint(|sp| async { ... })` 的闭包风格 API？（建议提供，更符合 Rust 惯例）
