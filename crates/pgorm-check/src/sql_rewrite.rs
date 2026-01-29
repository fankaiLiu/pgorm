use crate::error::{CheckError, CheckResult};

fn limit_node(limit: i32) -> pg_query::protobuf::Node {
    use pg_query::protobuf::{AConst, Integer, Node};
    use pg_query::protobuf::a_const;
    use pg_query::protobuf::node::Node as NodeEnum;

    Node {
        node: Some(NodeEnum::AConst(AConst {
            isnull: false,
            location: -1,
            val: Some(a_const::Val::Ival(Integer { ival: limit })),
        })),
    }
}

/// If the SQL is a single top-level SELECT without LIMIT/OFFSET, deparse a new SQL with `LIMIT`.
///
/// Returns:
/// - `Ok(Some(new_sql))` if rewritten
/// - `Ok(None)` if not a SELECT or already has LIMIT/OFFSET
pub fn ensure_select_limit(sql: &str, limit: i32) -> CheckResult<Option<String>> {
    if limit <= 0 {
        return Err(CheckError::Validation(
            "limit must be a positive integer".to_string(),
        ));
    }

    let mut parsed = pg_query::parse(sql)
        .map_err(|e| CheckError::Validation(format!("pg_query parse failed: {e}")))?;

    if parsed.protobuf.stmts.len() != 1 {
        return Err(CheckError::Validation(
            "LIMIT rewrite only supports single-statement SQL".to_string(),
        ));
    }

    let Some(raw) = parsed.protobuf.stmts.first_mut() else {
        return Ok(None);
    };
    let Some(stmt) = raw.stmt.as_deref_mut() else {
        return Ok(None);
    };
    let Some(node) = stmt.node.as_mut() else {
        return Ok(None);
    };

    match node {
        pg_query::NodeEnum::SelectStmt(select) => {
            if select.limit_count.is_some() || select.limit_offset.is_some() {
                return Ok(None);
            }

            select.limit_count = Some(Box::new(limit_node(limit)));
            select.limit_option = pg_query::protobuf::LimitOption::Count as i32;

            pg_query::deparse(&parsed.protobuf)
                .map(Some)
                .map_err(|e| CheckError::Validation(format!("pg_query deparse failed: {e}")))
        }
        _ => Ok(None),
    }
}
