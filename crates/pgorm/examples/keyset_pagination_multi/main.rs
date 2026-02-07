//! Example demonstrating multi-column keyset pagination with `KeysetN`.
//!
//! Run with:
//!   cargo run --example keyset_pagination_multi -p pgorm

use pgorm::{Condition, KeysetN, OrmResult, WhereExpr, sql};

fn main() -> OrmResult<()> {
    let keyset = KeysetN::desc(["created_at", "priority", "id"])?
        .after((1_700_000_000_i64, 10_i32, 99_i64))
        .limit(20);

    let where_expr =
        WhereExpr::atom(Condition::eq("status", "active")?).and_with(keyset.into_where_expr()?);

    let mut q = sql("SELECT id, created_at, priority FROM tasks");
    if !where_expr.is_trivially_true() {
        q.push(" WHERE ");
        where_expr.append_to_sql(&mut q);
    }
    keyset.append_order_by_limit_to_sql(&mut q)?;

    println!("SQL: {}", q.to_sql());
    println!("params: {}", q.params_ref().len());

    Ok(())
}
