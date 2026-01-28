use super::traits::{MutationBuilder, SqlBuilder};
use tokio_postgres::types::ToSql;

/// DELETE 语句构建器
pub struct DeleteBuilder {
    /// 表名
    table: String,
    /// WHERE 条件
    where_conditions: Vec<String>,
    /// 参数值
    params: Vec<Box<dyn ToSql + Sync + Send>>,
    /// 当前参数计数
    param_count: usize,
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
            where_conditions: Vec::new(),
            params: Vec::new(),
            param_count: 0,
            returning_cols: Vec::new(),
            allow_delete_all: false,
        }
    }

    /// 允许删除全表（即无 WHERE 条件）
    pub fn allow_delete_all(&mut self, allow: bool) -> &mut Self {
        self.allow_delete_all = allow;
        self
    }

    /// 添加 AND 相等条件
    pub fn and_eq<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.param_count += 1;
        self.where_conditions
            .push(format!("{} = ${}", col, self.param_count));
        self.params.push(Box::new(val));
        self
    }

    /// 添加 AND IN 条件
    pub fn and_in<T>(&mut self, col: &str, values: Vec<T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        if values.is_empty() {
            self.where_conditions.push("1=0".to_string());
            return self;
        }

        let mut placeholders = Vec::new();
        for value in values {
            self.param_count += 1;
            placeholders.push(format!("${}", self.param_count));
            self.params.push(Box::new(value));
        }

        self.where_conditions
            .push(format!("{} IN ({})", col, placeholders.join(", ")));
        self
    }

    /// 添加 AND 小于条件 (column < $N)
    pub fn and_lt<T>(&mut self, col: &str, val: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.param_count += 1;
        self.where_conditions
            .push(format!("{} < ${}", col, self.param_count));
        self.params.push(Box::new(val));
        self
    }

    /// 添加原始 WHERE 条件
    ///
    /// # Safety
    ///
    /// 此方法直接拼接 SQL 字符串，调用者必须确保 inputs 是安全的，防止 SQL 注入。
    pub fn and_raw(&mut self, sql: &str) -> &mut Self {
        self.where_conditions.push(sql.to_string());
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
        if self.where_conditions.is_empty() && !self.allow_delete_all {
            return format!("DELETE FROM {} WHERE 1=0", self.table);
        }

        let mut sql = format!("DELETE FROM {}", self.table);

        if !self.where_conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.where_conditions.join(" AND "));
        }

        if !self.returning_cols.is_empty() {
            sql.push_str(" RETURNING ");
            sql.push_str(&self.returning_cols.join(", "));
        }

        sql
    }

    fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params
            .iter()
            .map(|v| &**v as &(dyn ToSql + Sync))
            .collect()
    }
}

impl MutationBuilder for DeleteBuilder {}
