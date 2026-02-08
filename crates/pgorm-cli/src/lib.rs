//! `pgorm` command-line utilities.
//!
//! This crate powers the `pgorm` binary (see `crates/pgorm-cli/src/main.rs`).
//! The public surface is intentionally small: most logic lives in internal modules and is
//! dispatched from [`run`].

mod analyze;
mod cli;
mod codegen;
mod config;
mod gen_check;
mod generate;
mod init;
mod migrate_cmd;
mod model_codegen;
mod model_generate;
mod queries;
mod schema;
mod sql_check;
mod sql_validate;
mod type_mapper;
mod workflow;
mod write;

/// Runs the `pgorm` CLI with an argv-style argument list.
///
/// Most callers should pass `std::env::args().collect()`.
pub async fn run(args: Vec<String>) -> anyhow::Result<()> {
    let cmd = cli::parse_args(&args)?;
    match cmd {
        cli::Command::Help(topic) => {
            cli::print_help(topic);
            Ok(())
        }
        cli::Command::Init(args) => workflow::run_init(args).await,
        cli::Command::Build(args) => workflow::run_build(args).await,
        cli::Command::Check(args) => workflow::run_check(args).await,
        cli::Command::Schema(args) => schema::run(args).await,
        cli::Command::Sql(cmd) => match cmd {
            cli::SqlCommand::Check(args) => sql_check::run(args).await,
        },
        cli::Command::Migrate(cmd) => migrate_cmd::run(cmd).await,
    }
}
