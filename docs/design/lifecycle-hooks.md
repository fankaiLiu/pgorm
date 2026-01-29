# 生命周期 Hooks

> 参考来源：GORM (Go)、Ecto (Elixir)、ActiveRecord (Ruby)

## 概述

生命周期 Hooks 允许在数据库操作的特定阶段执行自定义逻辑，如数据验证、自动填充、审计日志等。

## 设计原则

- **显式调用**：Hooks 通过 trait 实现，调用时机明确
- **可选启用**：通过 `#[orm(hooks)]` 显式启用，不影响现有代码
- **同步执行**：Hooks 在同一事务中同步执行
- **错误传播**：Hook 返回错误时，整个操作回滚
- **无隐藏状态**：不使用全局回调注册，每个类型自行实现

## Hook 类型

| Hook | 触发时机 | 用途 |
|------|----------|------|
| `BeforeInsert` | INSERT 执行前 | 数据验证、自动填充、UUID 生成 |
| `AfterInsert` | INSERT 执行后 | 审计日志、发送通知、缓存更新 |
| `BeforeUpdate` | UPDATE 执行前 | 数据验证、版本号递增 |
| `AfterUpdate` | UPDATE 执行后 | 审计日志、缓存失效 |
| `BeforeDelete` | DELETE 执行前 | 软删除检查、级联删除验证 |
| `AfterDelete` | DELETE 执行后 | 清理关联资源、审计日志 |
| `AfterLoad` | 从数据库加载后 | 数据解密、计算派生字段 |

## API 设计

### Trait 定义

```rust
// crates/pgorm/src/hooks.rs

use crate::{GenericClient, OrmResult};

/// INSERT 执行前调用
pub trait BeforeInsert {
    /// 在插入前修改数据或执行验证
    /// 返回 Err 将阻止插入并回滚事务
    fn before_insert(&mut self) -> OrmResult<()> {
        Ok(())
    }

    /// 带数据库访问的版本（用于唯一性检查等）
    fn before_insert_with_client(
        &mut self,
        _client: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<()>> + Send {
        async { self.before_insert() }
    }
}

/// INSERT 执行后调用
pub trait AfterInsert {
    /// 插入后的处理，接收生成的 ID
    fn after_insert(&mut self, id: i64) -> OrmResult<()> {
        Ok(())
    }

    /// 带数据库访问的版本
    fn after_insert_with_client(
        &mut self,
        id: i64,
        _client: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<()>> + Send {
        async move { self.after_insert(id) }
    }
}

/// UPDATE 执行前调用
pub trait BeforeUpdate {
    fn before_update(&mut self) -> OrmResult<()> {
        Ok(())
    }

    fn before_update_with_client(
        &mut self,
        _client: &impl GenericClient,
    ) -> impl std::future::Future<Output = OrmResult<()>> + Send {
        async { self.before_update() }
    }
}

/// UPDATE 执行后调用
pub trait AfterUpdate {
    fn after_update(&mut self, affected: u64) -> OrmResult<()> {
        Ok(())
    }
}

/// DELETE 执行前调用
pub trait BeforeDelete {
    fn before_delete(id: i64, client: &impl GenericClient)
        -> impl std::future::Future<Output = OrmResult<()>> + Send;
}

/// DELETE 执行后调用
pub trait AfterDelete {
    fn after_delete(id: i64, affected: u64) -> OrmResult<()> {
        Ok(())
    }
}

/// 从数据库加载后调用
pub trait AfterLoad {
    fn after_load(&mut self) -> OrmResult<()> {
        Ok(())
    }
}
```

### 基本使用

```rust
use pgorm::{InsertModel, BeforeInsert, AfterInsert, OrmResult};
use uuid::Uuid;

#[derive(InsertModel)]
#[orm(table = "users")]
#[orm(hooks)]  // 启用 hooks
pub struct NewUser {
    pub uuid: Option<Uuid>,
    pub email: String,
    pub password_hash: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

impl BeforeInsert for NewUser {
    fn before_insert(&mut self) -> OrmResult<()> {
        // 自动生成 UUID
        if self.uuid.is_none() {
            self.uuid = Some(Uuid::new_v4());
        }

        // 验证 email 格式
        if !self.email.contains('@') {
            return Err(OrmError::validation("Invalid email format"));
        }

        // 密码哈希（示例）
        if let Some(ref password) = self.password_hash {
            self.password_hash = Some(hash_password(password)?);
        }

        Ok(())
    }
}

impl AfterInsert for NewUser {
    fn after_insert(&mut self, id: i64) -> OrmResult<()> {
        // 记录审计日志
        tracing::info!(user_id = id, email = %self.email, "User created");
        Ok(())
    }
}

// 使用
let mut user = NewUser {
    uuid: None,
    email: "alice@example.com".to_string(),
    password_hash: Some("secret123".to_string()),
    created_at: None,
};

// Hooks 自动调用
NewUser::insert(&mut user, &client).await?;
// user.uuid 现在有值了
```

### 带数据库访问的 Hook

```rust
impl BeforeInsert for NewUser {
    async fn before_insert_with_client(
        &mut self,
        client: &impl GenericClient,
    ) -> OrmResult<()> {
        // 检查 email 唯一性
        let exists = sql("SELECT 1 FROM users WHERE email = ")
            .push_param(&self.email)
            .push(" LIMIT 1")
            .fetch_opt::<(i32,)>(client)
            .await?
            .is_some();

        if exists {
            return Err(OrmError::validation("Email already exists"));
        }

        self.before_insert()
    }
}
```

### UpdateModel Hooks

```rust
#[derive(UpdateModel)]
#[orm(table = "users")]
#[orm(hooks)]
pub struct UpdateUser {
    pub email: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,

    #[orm(skip_update)]  // 内部字段，不写入数据库
    pub _version: Option<i32>,
}

impl BeforeUpdate for UpdateUser {
    fn before_update(&mut self) -> OrmResult<()> {
        // 自动更新时间戳
        self.updated_at = Some(Utc::now());

        // 验证 email（如果提供）
        if let Some(ref email) = self.email {
            if !email.contains('@') {
                return Err(OrmError::validation("Invalid email format"));
            }
        }

        Ok(())
    }
}
```

### Model Hooks (AfterLoad)

```rust
#[derive(Model, FromRow)]
#[orm(table = "users")]
#[orm(hooks)]
pub struct User {
    #[orm(id)]
    pub id: i64,
    pub email: String,
    pub encrypted_data: Vec<u8>,

    #[orm(skip)]  // 不映射到数据库
    pub decrypted_data: Option<String>,
}

impl AfterLoad for User {
    fn after_load(&mut self) -> OrmResult<()> {
        // 解密敏感数据
        self.decrypted_data = Some(decrypt(&self.encrypted_data)?);
        Ok(())
    }
}

// 加载时自动调用 after_load
let user = User::select_one(1, &client).await?;
assert!(user.decrypted_data.is_some());
```

## 实现方案

### 宏生成代码

启用 `#[orm(hooks)]` 后，宏生成的代码会调用相应的 trait 方法：

```rust
// 生成的 insert 方法
impl NewUser {
    pub async fn insert(
        input: &mut NewUser,  // 注意：变为 &mut
        client: &impl GenericClient,
    ) -> OrmResult<i64> {
        // 调用 BeforeInsert hook
        input.before_insert_with_client(client).await?;

        // 执行 INSERT
        let id: i64 = sql("INSERT INTO users (uuid, email, password_hash, created_at) \
                          VALUES ($1, $2, $3, $4) RETURNING id")
            .push_param(&input.uuid)
            .push_param(&input.email)
            .push_param(&input.password_hash)
            .push_param(&input.created_at)
            .fetch_one_scalar(client)
            .await?;

        // 调用 AfterInsert hook
        input.after_insert_with_client(id, client).await?;

        Ok(id)
    }
}
```

### 条件编译

通过 trait bounds 实现条件调用：

```rust
// 当类型实现了 BeforeInsert 时才调用
fn maybe_call_before_insert<T>(input: &mut T) -> OrmResult<()>
where
    T: ?Sized,
{
    // 使用 specialization 或 trait 检测
    Ok(())
}

fn call_before_insert<T: BeforeInsert>(input: &mut T) -> OrmResult<()> {
    input.before_insert()
}
```

### 空实现优化

对于未实现特定 hook 的类型，编译器会内联空的默认实现，实现零成本：

```rust
// 默认实现会被内联优化掉
impl<T> BeforeInsert for T {
    default fn before_insert(&mut self) -> OrmResult<()> {
        Ok(())
    }
}
```

## Hook 执行顺序

### Insert 流程

```
1. before_insert() / before_insert_with_client()
2. INSERT SQL 执行
3. after_insert() / after_insert_with_client()
```

### Update 流程

```
1. before_update() / before_update_with_client()
2. UPDATE SQL 执行
3. after_update()
```

### Delete 流程

```
1. BeforeDelete::before_delete()
2. DELETE SQL 执行
3. AfterDelete::after_delete()
```

### Load 流程

```
1. SELECT SQL 执行
2. FromRow::from_row()
3. after_load()
```

## 与其他功能的交互

### 与 Write Graph 的交互

```rust
#[derive(InsertModel)]
#[orm(table = "orders")]
#[orm(hooks)]
#[orm(has_many(OrderItem, field = "items", fk_field = "order_id"))]
pub struct NewOrder {
    pub customer_id: i64,
    pub total: Decimal,
}

impl BeforeInsert for NewOrder {
    fn before_insert(&mut self) -> OrmResult<()> {
        // 验证总金额
        if self.total <= Decimal::ZERO {
            return Err(OrmError::validation("Total must be positive"));
        }
        Ok(())
    }
}

#[derive(InsertModel)]
#[orm(table = "order_items")]
#[orm(hooks)]
pub struct NewOrderItem {
    pub order_id: Option<i64>,
    pub product_id: i64,
    pub quantity: i32,
}

impl BeforeInsert for NewOrderItem {
    fn before_insert(&mut self) -> OrmResult<()> {
        if self.quantity <= 0 {
            return Err(OrmError::validation("Quantity must be positive"));
        }
        Ok(())
    }
}

// Graph 写入时，每个模型的 hooks 都会被调用
let order = NewOrder {
    customer_id: 1,
    total: Decimal::new(10000, 2),
    items: vec![
        NewOrderItem { order_id: None, product_id: 1, quantity: 2 },
        NewOrderItem { order_id: None, product_id: 2, quantity: 1 },
    ],
};

// 执行顺序：
// 1. NewOrder::before_insert()
// 2. INSERT orders
// 3. NewOrder::after_insert()
// 4. NewOrderItem::before_insert() (for each item)
// 5. INSERT order_items (batch)
// 6. NewOrderItem::after_insert() (for each item)
NewOrder::insert_graph(&order, &client).await?;
```

### 与事务的交互

Hooks 在同一事务中执行：

```rust
pgorm::transaction!(client, tx, {
    let mut user = NewUser { ... };

    // before_insert 在事务内执行
    // 如果返回 Err，整个事务回滚
    NewUser::insert(&mut user, &tx).await?;

    // after_insert 也在事务内
    // 可以安全地进行关联操作

    Ok(())
})?;
```

### 与 Monitor 的交互

Hooks 执行时间不计入查询监控：

```rust
// QueryMonitor 只记录 SQL 执行时间
// Hook 执行时间单独记录（如果需要）
```

## 错误处理

### 验证错误

```rust
// 新增错误类型
pub enum OrmError {
    // ... 现有类型 ...

    /// 验证失败
    Validation {
        message: String,
        field: Option<String>,
    },
}

impl OrmError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
            field: None,
        }
    }

    pub fn validation_field(field: &str, message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
            field: Some(field.to_string()),
        }
    }
}
```

### 错误传播

```rust
impl BeforeInsert for NewUser {
    fn before_insert(&mut self) -> OrmResult<()> {
        if self.email.is_empty() {
            return Err(OrmError::validation_field("email", "Email is required"));
        }
        Ok(())
    }
}

// 调用方
match NewUser::insert(&mut user, &client).await {
    Ok(id) => println!("Created user {}", id),
    Err(OrmError::Validation { message, field }) => {
        println!("Validation failed: {} (field: {:?})", message, field);
    }
    Err(e) => return Err(e),
}
```

## 测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_before_insert_hook() {
        let mut user = NewUser {
            uuid: None,
            email: "test@example.com".to_string(),
            password_hash: None,
            created_at: None,
        };

        // 直接测试 hook
        user.before_insert().unwrap();

        assert!(user.uuid.is_some());
    }

    #[test]
    fn test_validation_error() {
        let mut user = NewUser {
            uuid: None,
            email: "invalid-email".to_string(),  // 无效 email
            password_hash: None,
            created_at: None,
        };

        let result = user.before_insert();
        assert!(matches!(result, Err(OrmError::Validation { .. })));
    }
}
```

## 与 GORM 的对比

| 特性 | GORM | pgorm |
|------|------|-------|
| Hook 注册 | 方法名约定 | Trait 实现 |
| 调用方式 | 自动（反射） | 显式（编译时） |
| 类型安全 | 运行时 | 编译时 |
| 性能开销 | 有（反射） | 零（内联） |
| 全局 Hooks | 支持 | 不支持（显式） |

## 实现检查清单

- [ ] 定义 `BeforeInsert` trait
- [ ] 定义 `AfterInsert` trait
- [ ] 定义 `BeforeUpdate` trait
- [ ] 定义 `AfterUpdate` trait
- [ ] 定义 `BeforeDelete` trait
- [ ] 定义 `AfterDelete` trait
- [ ] 定义 `AfterLoad` trait
- [ ] 添加 `OrmError::Validation` 类型
- [ ] 修改 `InsertModel` 宏支持 `#[orm(hooks)]`
- [ ] 修改 `UpdateModel` 宏支持 `#[orm(hooks)]`
- [ ] 修改 `Model` 宏支持 `#[orm(hooks)]`
- [ ] 与 Write Graph 集成
- [ ] 单元测试
- [ ] 文档更新
