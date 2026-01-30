mod cli;
mod codegen;
mod config;
mod generate;
mod gen_check;
mod init;
mod model_codegen;
mod model_generate;
mod sql_check;
mod sql_validate;
mod analyze;
mod queries;
mod schema;
mod type_mapper;
mod write;

pub async fn run(args: Vec<String>) -> anyhow::Result<()> {
    let cmd = cli::parse_args(&args)?;
    match cmd {
        cli::Command::Help(topic) => {
            cli::print_help(topic);
            Ok(())
        }
        cli::Command::Gen(cmd) => match cmd {
            cli::GenCommand::Init(args) => init::run(args),
            cli::GenCommand::Schema(args) => schema::run(args).await,
            cli::GenCommand::Check(args) => gen_check::run(args).await,
            cli::GenCommand::Run(args) => generate::run(args).await,
        },
        cli::Command::Model(args) => model_generate::run(args).await,
        cli::Command::Sql(cmd) => match cmd {
            cli::SqlCommand::Check(args) => sql_check::run(args).await,
        },
    }
}
