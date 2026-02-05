//! Styled terminal output helpers

#![allow(dead_code)]

use colored::Colorize;

/// Print a section header with styling
pub fn print_header(title: &str) {
    println!();
    println!("{}", "─".repeat(70).bright_black());
    println!("{}", title.bold().cyan());
    println!("{}", "─".repeat(70).bright_black());
}

/// Print success message with green checkmark
pub fn print_success(msg: &str) {
    println!("  {} {}", "✓".green().bold(), msg);
}

/// Print warning message with yellow warning sign
pub fn print_warning(msg: &str) {
    println!("  {} {}", "⚠".yellow().bold(), msg);
}

/// Print info message with blue info icon
pub fn print_info(msg: &str) {
    println!("  {} {}", "ℹ".blue(), msg);
}

/// Print a styled banner for example start
pub fn print_banner(title: &str) {
    let width = 70;
    let padding = (width - 2 - title.len()) / 2;
    let title_line = format!(
        "║{}{}{}║",
        " ".repeat(padding),
        title,
        " ".repeat(width - 2 - padding - title.len())
    );

    println!();
    println!("{}", format!("╔{}╗", "═".repeat(width - 2)).cyan());
    println!("{}", title_line.cyan().bold());
    println!("{}", format!("╚{}╝", "═".repeat(width - 2)).cyan());
}

/// Print a styled completion message
pub fn print_done() {
    println!();
    println!("{}", "═".repeat(70).cyan());
    println!("{}", "  Demo completed successfully!".green().bold());
    println!("{}", "═".repeat(70).cyan());
    println!();
}
