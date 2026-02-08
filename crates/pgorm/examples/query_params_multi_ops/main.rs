//! Example demonstrating multiple filter operations on a single `QueryParams` field.
//!
//! Run with:
//!   cargo run --example query_params_multi_ops -p pgorm

#![allow(dead_code)]

use pgorm::{FromRow, Model, OrmResult, QueryParams};

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "devices")]
struct Device {
    #[orm(id)]
    id: i64,
    label: String,
    version: i32,
}

#[derive(Debug, QueryParams)]
#[orm(model = "Device")]
struct DeviceSearchParams<'a> {
    // Multiple ops across multiple attrs on one field.
    #[orm(eq(DeviceQuery::COL_LABEL))]
    #[orm(ilike(DeviceQuery::COL_LABEL))]
    label: Option<&'a str>,

    // Multiple ops inside one attr list on one field.
    #[orm(gte(DeviceQuery::COL_VERSION), lte(DeviceQuery::COL_VERSION))]
    version: Option<i32>,

    #[orm(order_by_desc)]
    order_by_desc: Option<&'a str>,
}

fn main() -> OrmResult<()> {
    let params = DeviceSearchParams {
        label: Some("edge-router"),
        version: Some(3),
        order_by_desc: Some("id"),
    };

    let q = params.into_query()?;
    println!("built query builder = {q:?}");
    Ok(())
}
