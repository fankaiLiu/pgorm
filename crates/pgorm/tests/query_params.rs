#![allow(dead_code)]

use pgorm::{FromRow, Model, OrderBy, Pagination, QueryParams, WhereExpr};

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "audit_logs")]
struct AuditLog {
    #[orm(id)]
    id: i64,
    user_id: uuid::Uuid,
    operation_type: String,
    created_at: chrono::DateTime<chrono::Utc>,
    ip_address: Option<std::net::IpAddr>,
    status_code: i16,
}

fn parse_ip(s: &str) -> Option<std::net::IpAddr> {
    s.parse().ok()
}

#[derive(Clone, QueryParams)]
#[orm(model = "AuditLog")]
struct AuditLogSearchParams<'a> {
    #[orm(eq(AuditLogQuery::COL_USER_ID))]
    user_id: Option<uuid::Uuid>,

    #[orm(ne(AuditLogQuery::COL_STATUS_CODE))]
    status_code_ne: Option<i16>,

    #[orm(gt(AuditLogQuery::COL_STATUS_CODE))]
    status_code_gt: Option<i16>,

    #[orm(gte(AuditLogQuery::COL_CREATED_AT))]
    start_date: Option<chrono::DateTime<chrono::Utc>>,

    #[orm(lt(AuditLogQuery::COL_CREATED_AT))]
    before_date: Option<chrono::DateTime<chrono::Utc>>,

    #[orm(lte(AuditLogQuery::COL_CREATED_AT))]
    end_date: Option<chrono::DateTime<chrono::Utc>>,

    #[orm(like(AuditLogQuery::COL_OPERATION_TYPE))]
    op_like: Option<&'a str>,

    #[orm(ilike(AuditLogQuery::COL_OPERATION_TYPE))]
    op_ilike: Option<&'a str>,

    #[orm(eq(AuditLogQuery::COL_OPERATION_TYPE))]
    #[orm(ilike(AuditLogQuery::COL_OPERATION_TYPE))]
    op_eq_and_ilike: Option<&'a str>,

    #[orm(
        eq(AuditLogQuery::COL_OPERATION_TYPE),
        like(AuditLogQuery::COL_OPERATION_TYPE)
    )]
    op_eq_and_like_same_attr: Option<&'a str>,

    #[orm(not_like(AuditLogQuery::COL_OPERATION_TYPE))]
    op_not_like: Option<&'a str>,

    #[orm(not_ilike(AuditLogQuery::COL_OPERATION_TYPE))]
    op_not_ilike: Option<&'a str>,

    #[orm(is_null(AuditLogQuery::COL_IP_ADDRESS))]
    ip_is_null: Option<bool>,

    #[orm(is_not_null(AuditLogQuery::COL_IP_ADDRESS))]
    ip_is_not_null: bool,

    #[orm(in_list(AuditLogQuery::COL_STATUS_CODE))]
    status_in: Option<Vec<i16>>,

    #[orm(not_in(AuditLogQuery::COL_STATUS_CODE))]
    status_not_in: Vec<i16>,

    #[orm(between(AuditLogQuery::COL_STATUS_CODE))]
    status_between: Option<(i16, i16)>,

    #[orm(not_between(AuditLogQuery::COL_STATUS_CODE))]
    status_not_between: (i16, i16),

    #[orm(eq(AuditLogQuery::COL_IP_ADDRESS), map(parse_ip))]
    ip_address: Option<&'a str>,

    #[orm(raw)]
    raw_where: Option<&'a str>,

    #[orm(and)]
    and_expr: Option<WhereExpr>,

    #[orm(or)]
    or_expr: Option<WhereExpr>,

    #[orm(order_by_asc)]
    order_by_asc: Option<&'a str>,

    #[orm(order_by_desc)]
    order_by_desc: Option<&'a str>,

    #[orm(order_by_raw)]
    order_by_raw: Option<&'a str>,

    #[orm(order_by)]
    order_by: Option<OrderBy>,

    #[orm(paginate)]
    pagination: Option<Pagination>,

    #[orm(limit)]
    limit: Option<i64>,

    #[orm(offset)]
    offset: i64,

    #[orm(page(per_page = per_page.unwrap_or(10)))]
    page: Option<i64>,

    per_page: Option<i64>,
}

#[test]
fn query_params_apply_and_into_query() {
    let now = chrono::Utc::now();

    let params = AuditLogSearchParams {
        user_id: Some(uuid::Uuid::nil()),
        status_code_ne: Some(500),
        status_code_gt: Some(200),
        start_date: Some(now),
        before_date: None,
        end_date: Some(now),
        op_like: Some("%login%"),
        op_ilike: Some("%login%"),
        op_eq_and_ilike: Some("login"),
        op_eq_and_like_same_attr: Some("login"),
        op_not_like: None,
        op_not_ilike: None,
        ip_is_null: Some(false),
        ip_is_not_null: true,
        status_in: Some(vec![200, 201]),
        status_not_in: vec![404],
        status_between: Some((100, 300)),
        status_not_between: (400, 499),
        ip_address: Some("127.0.0.1"),
        raw_where: Some("1=1"),
        and_expr: Some(WhereExpr::raw("1=1")),
        or_expr: Some(WhereExpr::raw("1=0")),
        order_by_asc: Some("created_at"),
        order_by_desc: None,
        order_by_raw: Some("created_at DESC"),
        order_by: Some(OrderBy::new().asc("created_at").unwrap()),
        pagination: Some(Pagination::new().limit(10).offset(0)),
        limit: Some(50),
        offset: 0,
        page: Some(1),
        per_page: None,
    };

    let _q1 = params.clone().into_query().unwrap();
    let _q2 = params.apply(AuditLog::query()).unwrap();
}
