use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone)]
pub struct SelectColumn {
    /// Output column label (alias if provided, otherwise the column name).
    pub label: String,
    /// ColumnRef parts (excluding `*`), e.g. `["users","id"]` or `["id"]`.
    pub parts: Vec<String>,
}

pub fn extract_param_numbers(sql: &str) -> anyhow::Result<Vec<usize>> {
    let parsed = pg_query::parse(sql)?;

    let mut params: BTreeSet<usize> = BTreeSet::new();
    for (node, _depth, _context, _has_filter_columns) in parsed.protobuf.nodes() {
        if let pg_query::NodeRef::ParamRef(p) = node {
            if p.number <= 0 {
                continue;
            }
            params.insert(p.number as usize);
        }
    }

    let params: Vec<usize> = params.into_iter().collect();
    validate_param_sequence(&params)?;
    Ok(params)
}

pub fn extract_param_casts(sql: &str) -> HashMap<usize, String> {
    // MVP: only `$n::type` (single-token type name, e.g. uuid/jsonb/timestamptz/text/int8/uuid[]).
    let chars: Vec<char> = sql.chars().collect();
    let mut out: HashMap<usize, String> = HashMap::new();

    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '$' {
            i += 1;
            continue;
        }

        let mut j = i + 1;
        let mut num: usize = 0;
        let mut saw_digit = false;
        while j < chars.len() && chars[j].is_ascii_digit() {
            saw_digit = true;
            num = num
                .saturating_mul(10)
                .saturating_add((chars[j] as u8 - b'0') as usize);
            j += 1;
        }
        if !saw_digit {
            i += 1;
            continue;
        }

        // optional whitespace before `::`
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if j + 1 >= chars.len() || chars[j] != ':' || chars[j + 1] != ':' {
            i = j;
            continue;
        }
        j += 2;

        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }

        let mut ty = String::new();
        while j < chars.len() {
            let c = chars[j];
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '[' || c == ']' {
                ty.push(c);
                j += 1;
                continue;
            }
            break;
        }

        if !ty.is_empty() && num > 0 {
            out.insert(num, ty);
        }

        i = j;
    }

    out
}

pub fn extract_select_columns(sql: &str) -> anyhow::Result<Vec<SelectColumn>> {
    let parsed = pg_query::parse(sql)?;
    if parsed.protobuf.stmts.len() != 1 {
        anyhow::bail!("multiple SQL statements are not supported (MVP)");
    }

    let stmt_node = parsed
        .protobuf
        .stmts
        .first()
        .and_then(|s| s.stmt.as_ref())
        .and_then(|s| s.node.as_ref());

    let Some(stmt) = stmt_node else {
        anyhow::bail!("empty statement");
    };

    use pg_query::NodeEnum;
    let NodeEnum::SelectStmt(select) = stmt else {
        anyhow::bail!("expected SELECT statement for :one/:opt/:many queries (MVP)");
    };

    let mut out: Vec<SelectColumn> = Vec::new();

    for t in &select.target_list {
        let Some(NodeEnum::ResTarget(rt)) = t.node.as_ref() else {
            anyhow::bail!("unsupported SELECT target (MVP)");
        };

        let Some(val) = rt.val.as_ref().and_then(|v| v.node.as_ref()) else {
            anyhow::bail!("unsupported SELECT target (missing value) (MVP)");
        };

        let NodeEnum::ColumnRef(c) = val else {
            anyhow::bail!("only column references are supported in SELECT list (MVP)");
        };

        let mut parts: Vec<String> = Vec::new();
        let mut has_star = false;
        for f in &c.fields {
            match f.node.as_ref() {
                Some(NodeEnum::String(s)) => parts.push(s.sval.clone()),
                Some(NodeEnum::AStar(_)) => has_star = true,
                _ => {}
            }
        }
        if has_star || parts.is_empty() {
            anyhow::bail!("SELECT * is not supported for codegen (MVP)");
        }

        let label = if !rt.name.is_empty() {
            rt.name.clone()
        } else {
            parts
                .last()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("invalid column reference"))?
        };

        out.push(SelectColumn { label, parts });
    }

    if out.is_empty() {
        anyhow::bail!("empty SELECT target list");
    }

    Ok(out)
}

fn validate_param_sequence(params: &[usize]) -> anyhow::Result<()> {
    if params.is_empty() {
        return Ok(());
    }

    let max = *params.iter().max().unwrap();
    for expected in 1..=max {
        if !params.contains(&expected) {
            anyhow::bail!("SQL parameters must be contiguous ($1..$N); missing ${expected}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_param_numbers_dedup_and_sort() {
        let params = extract_param_numbers("SELECT $2, $1, $1").unwrap();
        assert_eq!(params, vec![1, 2]);
    }

    #[test]
    fn extract_param_numbers_requires_contiguous() {
        let err = extract_param_numbers("SELECT $2").unwrap_err();
        assert!(err.to_string().contains("missing $1"));
    }

    #[test]
    fn extract_param_casts_parses_simple_casts() {
        let casts = extract_param_casts("SELECT $1::uuid, $2 :: text, $3::uuid[]");
        assert_eq!(casts.get(&1).map(String::as_str), Some("uuid"));
        assert_eq!(casts.get(&2).map(String::as_str), Some("text"));
        assert_eq!(casts.get(&3).map(String::as_str), Some("uuid[]"));
    }

    #[test]
    fn extract_select_columns_supports_alias() {
        let cols = extract_select_columns(
            "SELECT id, email AS user_email FROM users WHERE id = $1",
        )
        .unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].label, "id");
        assert_eq!(cols[1].label, "user_email");
    }

    #[test]
    fn extract_select_columns_rejects_star() {
        let err = extract_select_columns("SELECT * FROM users").unwrap_err();
        assert!(err.to_string().contains("SELECT *"));
    }
}
