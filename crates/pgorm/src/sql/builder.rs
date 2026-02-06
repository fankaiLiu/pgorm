use super::parts::SqlPart;
use crate::bulk::{DeleteManyBuilder, SetExpr, UpdateManyBuilder};
use crate::condition::Condition;
use crate::cte::WithBuilder;
use crate::error::{OrmError, OrmResult};
use crate::ident::IntoIdent;
use std::sync::Arc;
use tokio_postgres::types::ToSql;

/// A parameter-safe dynamic SQL builder.
///
/// `Sql` stores SQL pieces and parameters separately and generates `$1, $2, ...`
/// placeholders automatically in the final SQL string.
#[must_use]
pub struct Sql {
    parts: Vec<SqlPart>,
    params: Vec<Arc<dyn ToSql + Sync + Send>>,
    tag: Option<String>,
}

impl Sql {
    /// Create a new builder with an initial SQL fragment.
    pub fn new(initial_sql: impl Into<String>) -> Self {
        Self {
            parts: vec![SqlPart::Raw(initial_sql.into())],
            params: Vec::new(),
            tag: None,
        }
    }

    /// Create an empty builder.
    pub fn empty() -> Self {
        Self {
            parts: Vec::new(),
            params: Vec::new(),
            tag: None,
        }
    }

    /// Associate a tag for monitoring/observability.
    ///
    /// # Example
    /// ```ignore
    /// let users: Vec<User> = pgorm::sql("SELECT * FROM users WHERE username ILIKE ")
    ///     .tag("users.search")
    ///     .push_bind("%admin%")
    ///     .fetch_all_as(&pg)
    ///     .await?;
    /// ```
    pub fn tag(&mut self, tag: impl Into<String>) -> &mut Self {
        self.tag = Some(tag.into());
        self
    }

    /// Associate a tag for monitoring/observability (consuming version).
    ///
    /// This is the consuming counterpart of [`Sql::tag`], convenient for
    /// chaining on temporary values.
    ///
    /// # Example
    /// ```ignore
    /// let users: Vec<User> = pgorm::sql("SELECT * FROM users")
    ///     .tagged("users.all")
    ///     .fetch_all_as(&pg)
    ///     .await?;
    /// ```
    pub fn tagged(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Append raw SQL (no parameters).
    pub fn push(&mut self, sql: &str) -> &mut Self {
        if sql.is_empty() {
            return self;
        }

        match self.parts.last_mut() {
            Some(SqlPart::Raw(last)) => last.push_str(sql),
            _ => self.parts.push(SqlPart::Raw(sql.to_string())),
        }
        self
    }

    /// Append a parameter placeholder and bind its value.
    pub fn push_bind<T>(&mut self, value: T) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.parts.push(SqlPart::Param);
        self.params.push(Arc::new(value));
        self
    }

    pub(crate) fn push_bind_value(&mut self, value: Arc<dyn ToSql + Sync + Send>) -> &mut Self {
        self.parts.push(SqlPart::Param);
        self.params.push(value);
        self
    }

    /// Append a comma-separated list of placeholders and bind all values.
    ///
    /// If `values` is empty, this appends `NULL` (so `IN (NULL)` is valid SQL
    /// but always evaluates to `FALSE`/`UNKNOWN` since nothing equals NULL).
    ///
    /// **Warning**: An empty list produces `IN (NULL)`, which is *never* true for
    /// any row. If you need "match nothing" semantics this is correct, but it may
    /// be surprising. Consider checking for emptiness before calling this method.
    pub fn push_bind_list<T>(&mut self, values: impl IntoIterator<Item = T>) -> &mut Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        let mut iter = values.into_iter();
        let Some(first) = iter.next() else {
            return self.push("NULL");
        };

        self.push_bind(first);
        for v in iter {
            self.push(", ");
            self.push_bind(v);
        }
        self
    }

    /// Append another `Sql` fragment, consuming it.
    pub fn push_sql(&mut self, mut other: Sql) -> &mut Self {
        self.parts.append(&mut other.parts);
        self.params.append(&mut other.params);
        if self.tag.is_none() {
            self.tag = other.tag;
        }
        self
    }

    /// Append a SQL identifier (schema/table/column) safely.
    ///
    /// This does **not** use parameters (Postgres doesn't allow parameterizing
    /// identifiers). To prevent SQL injection when identifiers are dynamic, this
    /// parses and validates identifiers via [`crate::Ident`].
    pub fn push_ident<I>(&mut self, ident: I) -> OrmResult<&mut Self>
    where
        I: IntoIdent,
    {
        let ident = ident.into_ident()?;
        Ok(self.push_ident_ref(&ident))
    }

    /// Append a pre-validated [`Ident`](crate::Ident) without returning `Result`.
    ///
    /// Unlike [`push_ident`](Sql::push_ident), this takes an already-validated `Ident`
    /// reference and cannot fail, making it suitable for chaining.
    ///
    /// ```ignore
    /// let ident = "users".into_ident()?;
    /// sql.push_ident_ref(&ident).push(" WHERE id = ").push_bind(1);
    /// ```
    pub fn push_ident_ref(&mut self, ident: &crate::Ident) -> &mut Self {
        match self.parts.last_mut() {
            Some(SqlPart::Raw(last)) => ident.write_sql(last),
            _ => {
                let mut s = String::new();
                ident.write_sql(&mut s);
                self.parts.push(SqlPart::Raw(s));
            }
        }
        self
    }

    /// Render SQL with `$1, $2, ...` placeholders.
    pub fn to_sql(&self) -> String {
        // Fast integer digit count (avoids division loop for common cases).
        #[inline]
        fn decimal_digits(n: usize) -> usize {
            if n < 10 {
                1
            } else if n < 100 {
                2
            } else if n < 1000 {
                3
            } else if n < 10000 {
                4
            } else {
                // Fallback for very large parameter counts (unlikely in practice).
                (n.ilog10() as usize) + 1
            }
        }

        // Write a usize as decimal digits into `out` without going through fmt.
        #[inline]
        fn push_usize(out: &mut String, mut n: usize) {
            if n < 10 {
                out.push((b'0' + n as u8) as char);
                return;
            }
            // Stack buffer for up to 20 digits (u64::MAX).
            let mut buf = [0u8; 20];
            let mut pos = buf.len();
            while n > 0 {
                pos -= 1;
                buf[pos] = b'0' + (n % 10) as u8;
                n /= 10;
            }
            // SAFETY: buf[pos..] only contains ASCII digits.
            out.push_str(unsafe { std::str::from_utf8_unchecked(&buf[pos..]) });
        }

        // Pre-size to avoid repeated reallocations (hot path).
        let mut idx: usize = 0;
        let mut cap: usize = 0;
        for part in &self.parts {
            match part {
                SqlPart::Raw(s) => cap += s.len(),
                SqlPart::Param => {
                    idx += 1;
                    cap += 1 /* '$' */ + decimal_digits(idx);
                }
            }
        }

        let mut out = String::with_capacity(cap);
        idx = 0;
        for part in &self.parts {
            match part {
                SqlPart::Raw(s) => out.push_str(s),
                SqlPart::Param => {
                    idx += 1;
                    out.push('$');
                    push_usize(&mut out, idx);
                }
            }
        }
        out
    }

    /// Parameter refs compatible with `tokio-postgres`.
    pub fn params_ref(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params
            .iter()
            .map(|p| p.as_ref() as &(dyn ToSql + Sync))
            .collect()
    }

    fn validate(&self) -> OrmResult<()> {
        let placeholder_count = self
            .parts
            .iter()
            .filter(|p| matches!(p, SqlPart::Param))
            .count();

        if placeholder_count != self.params.len() {
            let params_len = self.params.len();
            return Err(OrmError::Validation(format!(
                "Sql: placeholders({placeholder_count}) != params({params_len})"
            )));
        }
        Ok(())
    }

    impl_query_exec! {
        prepare(self) {
            self.validate()?;
            let sql = self.to_sql();
            let params = self.params_ref();
            let tag = self.tag.as_deref();
            (sql, params, tag)
        }
    }

    /// Append a [`Condition`] to this SQL builder.
    ///
    /// This uses `Sql`'s placeholder generation to keep parameter indices correct.
    pub fn push_condition(&mut self, condition: &Condition) -> &mut Self {
        condition.append_to_sql(self);
        self
    }

    /// Append multiple [`Condition`]s joined by `AND`.
    ///
    /// If `conditions` is empty, this is a no-op.
    pub fn push_conditions_and(&mut self, conditions: &[Condition]) -> &mut Self {
        for (i, cond) in conditions.iter().enumerate() {
            if i > 0 {
                self.push(" AND ");
            }
            self.push_condition(cond);
        }
        self
    }

    /// Append a `WHERE ...` clause composed of [`Condition`]s joined by `AND`.
    ///
    /// If `conditions` is empty, this is a no-op.
    pub fn push_where_and(&mut self, conditions: &[Condition]) -> &mut Self {
        if conditions.is_empty() {
            return self;
        }
        self.push(" WHERE ");
        self.push_conditions_and(conditions)
    }

    /// Append `LIMIT $n` to the query with a bound parameter.
    ///
    /// # Example
    /// ```ignore
    /// let users = sql("SELECT * FROM users ORDER BY id")
    ///     .limit(10)
    ///     .fetch_all_as(&client)
    ///     .await?;
    /// ```
    pub fn limit(&mut self, n: i64) -> &mut Self {
        self.push(" LIMIT ").push_bind(n)
    }

    /// Append `OFFSET $n` to the query with a bound parameter.
    ///
    /// # Example
    /// ```ignore
    /// let users = sql("SELECT * FROM users ORDER BY id")
    ///     .limit(10)
    ///     .offset(20)
    ///     .fetch_all_as(&client)
    ///     .await?;
    /// ```
    pub fn offset(&mut self, n: i64) -> &mut Self {
        self.push(" OFFSET ").push_bind(n)
    }

    /// Append `LIMIT $n OFFSET $m` to the query with bound parameters.
    ///
    /// # Example
    /// ```ignore
    /// let users = sql("SELECT * FROM users ORDER BY id")
    ///     .limit_offset(10, 20)
    ///     .fetch_all_as(&client)
    ///     .await?;
    /// ```
    pub fn limit_offset(&mut self, limit: i64, offset: i64) -> &mut Self {
        self.push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset)
    }

    /// Append pagination using page number and page size.
    ///
    /// Converts page-based pagination to LIMIT/OFFSET. Page numbers start at 1.
    /// Returns an error if `page < 1`.
    ///
    /// # Example
    /// ```ignore
    /// // Get page 3 with 25 items per page
    /// let users = sql("SELECT * FROM users ORDER BY id")
    ///     .page(3, 25)?
    ///     .fetch_all_as(&client)
    ///     .await?;
    /// ```
    pub fn page(&mut self, page: i64, per_page: i64) -> OrmResult<&mut Self> {
        if page < 1 {
            return Err(OrmError::Validation(format!(
                "page must be >= 1, got {page}"
            )));
        }
        let offset = (page - 1) * per_page;
        Ok(self.limit_offset(per_page, offset))
    }

    // ==================== Consuming convenience APIs ====================

    /// Bind a parameter and return `self` (consuming version of [`push_bind`]).
    ///
    /// Useful for chaining in contexts where you need ownership, e.g. CTE sub-queries:
    ///
    /// ```ignore
    /// pgorm::sql("SELECT * FROM users WHERE status = ")
    ///     .bind("active")
    /// ```
    pub fn bind<T>(mut self, value: T) -> Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.push_bind(value);
        self
    }

    // ==================== Bulk operations ====================

    /// Create a bulk UPDATE builder.
    ///
    /// The initial SQL fragment is used as the table name.
    ///
    /// # Example
    /// ```ignore
    /// pgorm::sql("users")
    ///     .update_many([
    ///         SetExpr::set("status", "inactive")?,
    ///     ])?
    ///     .filter(Condition::lt("last_login", one_year_ago)?)
    ///     .execute(&client)
    ///     .await?;
    /// ```
    pub fn update_many(
        self,
        sets: impl IntoIterator<Item = SetExpr>,
    ) -> OrmResult<UpdateManyBuilder> {
        let table_name = self.to_sql();
        let table = table_name.trim().into_ident()?;
        let sets: Vec<SetExpr> = sets.into_iter().collect();
        if sets.is_empty() {
            return Err(OrmError::Validation(
                "update_many requires at least one SetExpr".to_string(),
            ));
        }
        Ok(UpdateManyBuilder {
            table,
            sets,
            where_clause: None,
            all_rows: false,
        })
    }

    /// Create a bulk DELETE builder.
    ///
    /// The initial SQL fragment is used as the table name.
    ///
    /// # Example
    /// ```ignore
    /// pgorm::sql("sessions")
    ///     .delete_many()?
    ///     .filter(Condition::lt("expires_at", now)?)
    ///     .execute(&client)
    ///     .await?;
    /// ```
    pub fn delete_many(self) -> OrmResult<DeleteManyBuilder> {
        let table_name = self.to_sql();
        let table = table_name.trim().into_ident()?;
        Ok(DeleteManyBuilder {
            table,
            where_clause: None,
            all_rows: false,
        })
    }

    // ==================== CTE (WITH clause) ====================

    /// Start building a CTE (WITH clause) query.
    ///
    /// # Example
    /// ```ignore
    /// pgorm::sql("")
    ///     .with("active_users", pgorm::sql("SELECT id FROM users WHERE status = ").bind("active"))?
    ///     .select(pgorm::sql("SELECT * FROM active_users"))
    ///     .fetch_all_as::<User>(&client)
    ///     .await?;
    /// ```
    pub fn with(self, name: impl IntoIdent, query: Sql) -> OrmResult<WithBuilder> {
        let name = name.into_ident()?;
        Ok(WithBuilder::new(name, query))
    }

    /// Start building a CTE with explicit column names.
    ///
    /// # Example
    /// ```ignore
    /// pgorm::sql("")
    ///     .with_columns(
    ///         "monthly_sales",
    ///         ["month", "total"],
    ///         pgorm::sql("SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY 1"),
    ///     )?
    ///     .select(pgorm::sql("SELECT * FROM monthly_sales"))
    /// ```
    pub fn with_columns(
        self,
        name: impl IntoIdent,
        columns: impl IntoIterator<Item = impl IntoIdent>,
        query: Sql,
    ) -> OrmResult<WithBuilder> {
        let name = name.into_ident()?;
        let cols: Vec<crate::Ident> = columns
            .into_iter()
            .map(|c| c.into_ident())
            .collect::<OrmResult<Vec<_>>>()?;
        Ok(WithBuilder::new_with_columns(name, cols, query))
    }

    /// Start building a recursive CTE (WITH RECURSIVE).
    ///
    /// Uses UNION ALL by default.
    ///
    /// # Example
    /// ```ignore
    /// pgorm::sql("")
    ///     .with_recursive(
    ///         "org_tree",
    ///         pgorm::sql("SELECT id, name, parent_id, 0 as level FROM employees WHERE parent_id IS NULL"),
    ///         pgorm::sql("SELECT e.id, e.name, e.parent_id, t.level + 1 FROM employees e JOIN org_tree t ON e.parent_id = t.id"),
    ///     )?
    ///     .select(pgorm::sql("SELECT * FROM org_tree ORDER BY level"))
    ///     .fetch_all_as::<OrgNode>(&client)
    ///     .await?;
    /// ```
    pub fn with_recursive(
        self,
        name: impl IntoIdent,
        base: Sql,
        recursive: Sql,
    ) -> OrmResult<WithBuilder> {
        let name = name.into_ident()?;
        Ok(WithBuilder::new_recursive(name, base, recursive, true))
    }

    /// Start building a recursive CTE using UNION (with deduplication).
    pub fn with_recursive_union(
        self,
        name: impl IntoIdent,
        base: Sql,
        recursive: Sql,
    ) -> OrmResult<WithBuilder> {
        let name = name.into_ident()?;
        Ok(WithBuilder::new_recursive(name, base, recursive, false))
    }
}
