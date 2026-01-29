//! SQL linting example
//!
//! Run with:
//! `cargo run --example sql_lint -p pgorm-check --features sql`

use colored::Colorize;
use pgorm_check::{
    LintLevel, StatementKind, delete_has_where, detect_statement_kind, get_table_names,
    is_valid_sql, lint_select_many, lint_sql, select_has_limit, select_has_star, update_has_where,
};

fn main() {
    println!("{}", "=== SQL Lint Examples ===".bold().cyan());
    println!();

    // 1. Syntax validation
    println!("{}", "1. Syntax Validation".bold());
    check_syntax("SELECT * FROM users");
    check_syntax("SELEC * FROM users");
    check_syntax("SELECT id, name FROM users WHERE id = $1");
    println!();

    // 2. Statement type detection
    println!("{}", "2. Statement Type Detection".bold());
    detect_type("SELECT * FROM users");
    detect_type("INSERT INTO users (name) VALUES ('foo')");
    detect_type("UPDATE users SET name = 'bar' WHERE id = 1");
    detect_type("DELETE FROM users WHERE id = 1");
    detect_type("CREATE TABLE foo (id INT PRIMARY KEY)");
    detect_type("TRUNCATE users");
    println!();

    // 3. SELECT checks
    println!("{}", "3. SELECT Checks".bold());
    check_select("SELECT * FROM users");
    check_select("SELECT id, name FROM users");
    check_select("SELECT * FROM users LIMIT 10");
    check_select("SELECT t.* FROM users t LIMIT 100");
    println!();

    // 4. DELETE/UPDATE safety checks
    println!("{}", "4. DELETE/UPDATE Safety Checks".bold());
    check_dangerous("DELETE FROM users");
    check_dangerous("DELETE FROM users WHERE id = 1");
    check_dangerous("UPDATE users SET active = false");
    check_dangerous("UPDATE users SET active = false WHERE id = 1");
    println!();

    // 5. Table extraction
    println!("{}", "5. Table Name Extraction".bold());
    extract_tables("SELECT * FROM users");
    extract_tables("SELECT * FROM users u JOIN orders o ON u.id = o.user_id");
    extract_tables("SELECT * FROM public.users, public.orders WHERE users.id = orders.user_id");
    extract_tables(
        "WITH active_users AS (SELECT * FROM users WHERE active) SELECT * FROM active_users",
    );
    println!();

    // 6. Full lint
    println!("{}", "6. Full SQL Linting".bold());
    full_lint("SELECT * FROM users");
    full_lint("SELECT id, name FROM users WHERE active = true");
    full_lint("DELETE FROM users");
    full_lint("DELETE FROM users WHERE id = 1");
    full_lint("UPDATE users SET name = 'foo'");
    full_lint("TRUNCATE users");
    full_lint("DROP TABLE users");
    println!();

    // 7. Select many lint (for queries that return multiple rows)
    println!("{}", "7. Select Many Linting".bold());
    lint_many("SELECT * FROM users");
    lint_many("SELECT * FROM users LIMIT 100");
    lint_many("SELECT id, name FROM users ORDER BY created_at DESC LIMIT 50");
    lint_many("DELETE FROM users"); // Error: not a SELECT
}

fn check_syntax(sql: &str) {
    let result = is_valid_sql(sql);
    let status = if result.valid {
        "✓".green().bold()
    } else {
        "✗".red().bold()
    };
    println!("  {} {}", status, sql.dimmed());
    if let Some(err) = result.error {
        println!("    {}: {}", "Error".red(), err);
    }
}

fn detect_type(sql: &str) {
    let kind = detect_statement_kind(sql);
    let kind_str = match kind {
        Some(StatementKind::Select) => "SELECT".cyan(),
        Some(StatementKind::Insert) => "INSERT".green(),
        Some(StatementKind::Update) => "UPDATE".yellow(),
        Some(StatementKind::Delete) => "DELETE".red(),
        Some(StatementKind::CreateTable) => "CREATE TABLE".blue(),
        Some(StatementKind::Truncate) => "TRUNCATE".magenta(),
        Some(k) => format!("{:?}", k).normal(),
        None => "Unknown".dimmed(),
    };
    println!("  {} {}", kind_str, sql.dimmed());
}

fn check_select(sql: &str) {
    let has_star = select_has_star(sql).unwrap_or(false);
    let has_limit = select_has_limit(sql).unwrap_or(false);

    let star_icon = if has_star {
        "★".yellow()
    } else {
        "○".dimmed()
    };
    let limit_icon = if has_limit {
        "LIMIT".green()
    } else {
        "no-limit".red()
    };

    println!("  {} {} {}", star_icon, limit_icon, sql.dimmed());
}

fn check_dangerous(sql: &str) {
    let kind = detect_statement_kind(sql);
    let has_where = match kind {
        Some(StatementKind::Delete) => delete_has_where(sql),
        Some(StatementKind::Update) => update_has_where(sql),
        _ => None,
    };

    let (icon, status) = match has_where {
        Some(true) => ("✓".green().bold(), "safe".green()),
        Some(false) => ("⚠".red().bold(), "DANGEROUS".red().bold()),
        None => ("?".dimmed(), "N/A".dimmed()),
    };

    println!("  {} {} {}", icon, status, sql.dimmed());
}

fn extract_tables(sql: &str) {
    let tables = get_table_names(sql);
    let tables_str = if tables.is_empty() {
        "none".dimmed().to_string()
    } else {
        tables
            .iter()
            .map(|t| t.cyan().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!("  [{}] {}", tables_str, sql.dimmed());
}

fn full_lint(sql: &str) {
    let result = lint_sql(sql);

    let icon = if result.has_errors() {
        "✗".red().bold()
    } else if result.has_warnings() {
        "⚠".yellow().bold()
    } else if result.is_ok() {
        "✓".green().bold()
    } else {
        "ℹ".blue()
    };

    println!("  {} {}", icon, sql.dimmed());

    for issue in &result.issues {
        let level = match issue.level {
            LintLevel::Error => "ERROR".red().bold(),
            LintLevel::Warning => "WARN".yellow().bold(),
            LintLevel::Info => "INFO".blue(),
        };
        println!(
            "    {} [{}] {}",
            level,
            issue.code.cyan(),
            issue.message.white()
        );
    }
}

fn lint_many(sql: &str) {
    let result = lint_select_many(sql);

    let icon = if result.has_errors() {
        "✗".red().bold()
    } else if result.has_warnings() {
        "⚠".yellow().bold()
    } else {
        "✓".green().bold()
    };

    println!("  {} {}", icon, sql.dimmed());

    for issue in &result.issues {
        let level = match issue.level {
            LintLevel::Error => "ERROR".red().bold(),
            LintLevel::Warning => "WARN".yellow().bold(),
            LintLevel::Info => "INFO".blue(),
        };
        println!(
            "    {} [{}] {}",
            level,
            issue.code.cyan(),
            issue.message.white()
        );
    }
}
