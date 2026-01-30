//! Eager loading (batch preloading for relations).
//!
//! This module provides small, explicit building blocks:
//! - `load_*_map*` helpers that run exactly one extra query per relation.
//! - `Loaded<M, R>` wrapper for the optional "attach" style.

use crate::{FromRow, GenericClient, ModelPk, OrmResult, RowExt, Sql, sql};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use tokio_postgres::types::{FromSqlOwned, ToSql};

pub type HasManyMap<Id, Child> = HashMap<Id, Vec<Child>>;
pub type BelongsToMap<Id, Parent> = HashMap<Id, Parent>;

/// A wrapper returned by "attach" style eager loading.
#[derive(Debug, Clone)]
pub struct Loaded<M, R> {
    pub base: M,
    pub rel: R,
}

impl<M, R> std::ops::Deref for Loaded<M, R> {
    type Target = M;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl<M, R> std::ops::DerefMut for Loaded<M, R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

pub async fn load_has_many_map<Child, Id>(
    conn: &impl GenericClient,
    parent_ids: Vec<Id>,
    select_list: &'static str,
    table: &'static str,
    join_clause: &'static str,
    fk_col: &'static str,
) -> OrmResult<HasManyMap<Id, Child>>
where
    Child: FromRow,
    Id: ToSql + FromSqlOwned + Eq + Hash + Send + Sync + 'static,
{
    load_has_many_map_with(
        conn,
        parent_ids,
        select_list,
        table,
        join_clause,
        fk_col,
        |_| {},
    )
    .await
}

pub async fn load_has_many_map_with<Child, Id, F>(
    conn: &impl GenericClient,
    parent_ids: Vec<Id>,
    select_list: &'static str,
    table: &'static str,
    join_clause: &'static str,
    fk_col: &'static str,
    with: F,
) -> OrmResult<HasManyMap<Id, Child>>
where
    Child: FromRow,
    Id: ToSql + FromSqlOwned + Eq + Hash + Send + Sync + 'static,
    F: FnOnce(&mut Sql),
{
    if parent_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let parent_ids: Vec<Id> = parent_ids
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if parent_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut q = sql("SELECT ");
    q.push(select_list);
    q.push(" FROM ");
    q.push_ident(table)?;
    if !join_clause.trim().is_empty() {
        q.push(" ");
        q.push(join_clause);
    }
    q.push(" WHERE ");
    q.push_ident(table)?;
    q.push(".");
    q.push_ident(fk_col)?;
    q.push(" = ANY(");
    q.push_bind(parent_ids);
    q.push(")");
    with(&mut q);

    let rows = q.fetch_all(conn).await?;

    let mut out: HashMap<Id, Vec<Child>> = HashMap::new();
    for row in rows {
        let fk: Option<Id> = row.try_get_column(fk_col)?;
        let Some(fk) = fk else { continue };
        let child = Child::from_row(&row)?;
        out.entry(fk).or_default().push(child);
    }
    Ok(out)
}

pub async fn load_belongs_to_map<Parent, Id>(
    conn: &impl GenericClient,
    ids: Vec<Id>,
    select_list: &'static str,
    table: &'static str,
    join_clause: &'static str,
    id_col: &'static str,
) -> OrmResult<BelongsToMap<Id, Parent>>
where
    Parent: FromRow + ModelPk<Id = Id>,
    Id: ToSql + Clone + Eq + Hash + Send + Sync + 'static,
{
    load_belongs_to_map_with(conn, ids, select_list, table, join_clause, id_col, |_| {}).await
}

pub async fn load_belongs_to_map_with<Parent, Id, F>(
    conn: &impl GenericClient,
    ids: Vec<Id>,
    select_list: &'static str,
    table: &'static str,
    join_clause: &'static str,
    id_col: &'static str,
    with: F,
) -> OrmResult<BelongsToMap<Id, Parent>>
where
    Parent: FromRow + ModelPk<Id = Id>,
    Id: ToSql + Clone + Eq + Hash + Send + Sync + 'static,
    F: FnOnce(&mut Sql),
{
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let ids: Vec<Id> = ids
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut q = sql("SELECT ");
    q.push(select_list);
    q.push(" FROM ");
    q.push_ident(table)?;
    if !join_clause.trim().is_empty() {
        q.push(" ");
        q.push(join_clause);
    }
    q.push(" WHERE ");
    q.push_ident(table)?;
    q.push(".");
    q.push_ident(id_col)?;
    q.push(" = ANY(");
    q.push_bind(ids);
    q.push(")");
    with(&mut q);

    let rows = q.fetch_all(conn).await?;
    let mut out: HashMap<Id, Parent> = HashMap::new();
    for row in rows {
        let parent = Parent::from_row(&row)?;
        out.insert(ModelPk::pk(&parent).to_owned(), parent);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrmError, OrmResult};
    use tokio_postgres::Row;
    use tokio_postgres::types::ToSql;

    struct PanicClient;

    impl GenericClient for PanicClient {
        async fn query(&self, _sql: &str, _params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
            panic!("unexpected query() call")
        }

        async fn query_one(&self, _sql: &str, _params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
            Err(OrmError::not_found("unexpected query_one() call"))
        }

        async fn query_opt(
            &self,
            _sql: &str,
            _params: &[&(dyn ToSql + Sync)],
        ) -> OrmResult<Option<Row>> {
            panic!("unexpected query_opt() call")
        }

        async fn execute(&self, _sql: &str, _params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
            panic!("unexpected execute() call")
        }
    }

    struct DummyChild;
    impl FromRow for DummyChild {
        fn from_row(_row: &Row) -> OrmResult<Self> {
            panic!("unexpected DummyChild::from_row() call")
        }
    }

    #[derive(Debug)]
    struct DummyParent;
    impl FromRow for DummyParent {
        fn from_row(_row: &Row) -> OrmResult<Self> {
            panic!("unexpected DummyParent::from_row() call")
        }
    }
    impl ModelPk for DummyParent {
        type Id = i64;
        fn pk(&self) -> &Self::Id {
            static ID: i64 = 0;
            &ID
        }
    }

    #[tokio::test]
    async fn empty_input_fast_path() {
        let conn = PanicClient;

        let hm: HasManyMap<i64, DummyChild> =
            load_has_many_map::<DummyChild, i64>(&conn, vec![], "*", "posts", "", "user_id")
                .await
                .unwrap();
        assert!(hm.is_empty());

        let bt: BelongsToMap<i64, DummyParent> =
            load_belongs_to_map::<DummyParent, i64>(&conn, vec![], "*", "users", "", "id")
                .await
                .unwrap();
        assert!(bt.is_empty());
    }
}
