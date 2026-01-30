use pgorm_check::{DbSchema, LintLevel, SqlCheckLevel};

#[derive(Debug, Clone, Copy, Default)]
pub struct SqlValidationSummary {
    pub had_error: bool,
    pub had_warning: bool,
}

pub fn validate_sql(header: &str, sql: &str, schema: &DbSchema) -> SqlValidationSummary {
    let mut out = SqlValidationSummary::default();

    // Syntax
    let parse = pgorm_check::is_valid_sql(sql);
    if !parse.valid {
        out.had_error = true;
        eprintln!(
            "[ERROR] {header}: SQL syntax error: {}",
            parse.error.unwrap_or_else(|| "unknown error".to_string())
        );
        return out;
    }

    // Multi-statement guard (MVP)
    if let Ok(parsed) = pg_query::parse(sql) {
        if parsed.protobuf.stmts.len() != 1 {
            out.had_error = true;
            eprintln!("[ERROR] {header}: multiple SQL statements are not supported (MVP)");
            return out;
        }
    }

    // Lint
    let lint = pgorm_check::lint_sql(sql);
    for issue in lint.issues {
        match issue.level {
            LintLevel::Error => out.had_error = true,
            LintLevel::Warning => out.had_warning = true,
            LintLevel::Info => {}
        }
        eprintln!(
            "[{:?}] {header}: {} {}",
            issue.level, issue.code, issue.message
        );
    }

    // Schema references
    match pgorm_check::check_sql(schema, sql) {
        Ok(issues) => {
            for issue in issues {
                match issue.level {
                    SqlCheckLevel::Error => out.had_error = true,
                    SqlCheckLevel::Warning => out.had_warning = true,
                }
                eprintln!("[{:?}] {header}: {}", issue.level, issue.message);
            }
        }
        Err(e) => {
            out.had_error = true;
            eprintln!("[ERROR] {header}: check failed: {e}");
        }
    }

    out
}

