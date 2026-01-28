use super::traits::{MutationBuilder, SqlBuilder};
use super::where_builder::WhereBuilder;
use tokio_postgres::types::ToSql;

/// DELETE 语句构建器
pub struct DeleteBuilder {
    /// 表名
    table: String,
    /// WHERE 条件
    where_builder: WhereBuilder,
    /// RETURNING 列
    returning_cols: Vec<String>,
    /// 是否允许删除全表（无 WHERE 条件）
    allow_delete_all: bool,
}

impl DeleteBuilder {
    /// 创建新的 DELETE 构建器
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            where_builder: WhereBuilder::new(),
            returning_cols: Vec::new(),
            allow_delete_all: false,
        }
    }

    /// 允许删除全表（即无 WHERE 条件）
    pub fn allow_delete_all(&mut self, allow: bool) -> &mut Self {
        self.allow_delete_all = allow;
        self
    }

    // ==================== Conditions (delegated to WhereBuilder) ====================

    /// 添加 AND 相等条件
    pub fn and_eq<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_eq(col, val);
        self
    }

    /// 添加 AND 不等条件
    pub fn and_ne<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_ne(col, val);
        self
    }

    /// 添加 AND IN 条件
    pub fn and_in<T>(&mut self, col: &str, values: Vec<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_in(col, values);
        self
    }

    /// 添加 AND 小于条件 (column < $N)
    pub fn and_lt<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_lt(col, val);
        self
    }

    /// 添加 AND 小于等于条件
    pub fn and_lte<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_lte(col, val);
        self
    }

    /// 添加 AND 大于条件
    pub fn and_gt<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_gt(col, val);
        self
    }

    /// 添加 AND 大于等于条件
    pub fn and_gte<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.where_builder.and_gte(col, val);
        self
    }

    /// 添加 AND IS NULL 条件
    pub fn and_is_null(&mut self, col: &str) -> &mut Self {
        self.where_builder.and_is_null(col);
        self
    }

    /// 添加 AND IS NOT NULL 条件
    pub fn and_is_not_null(&mut self, col: &str) -> &mut Self {
        self.where_builder.and_is_not_null(col);
        self
    }

    /// 添加原始 WHERE 条件
    ///
    /// # Safety
    ///
    /// 此方法直接拼接 SQL 字符串，调用者必须确保 inputs 是安全的，防止 SQL 注入。
    pub fn and_raw(&mut self, sql: &str) -> &mut Self {
        self.where_builder.and_raw(sql);
        self
    }

    /// 设置 RETURNING 子句
    pub fn returning(&mut self, cols: &str) -> &mut Self {
        self.returning_cols = vec![cols.to_string()];
        self
    }

    /// 设置 RETURNING 子句（数组方式）
    pub fn returning_cols(&mut self, cols: &[&str]) -> &mut Self {
        self.returning_cols = cols.iter().map(|s| s.to_string()).collect();
        self
    }
}

impl SqlBuilder for DeleteBuilder {
    fn build_sql(&self) -> String {
        // 安全检查：如果无条件且不允许删除全表，生成安全 no-op
        if self.where_builder.is_empty() && !self.allow_delete_all {
            return format!("DELETE FROM {} WHERE 1=0", self.table);
        }

        let mut sql = format!("DELETE FROM {}", self.table);

        if !self.where_builder.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.where_builder.build_clause());
        }

        if !self.returning_cols.is_empty() {
            sql.push_str(" RETURNING ");
            sql.push_str(&self.returning_cols.join(", "));
        }

        sql
    }

    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.where_builder.params_ref()
    }
}

impl MutationBuilder for DeleteBuilder {}
