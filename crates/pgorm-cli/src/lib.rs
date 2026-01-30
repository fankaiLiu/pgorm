mod cli;
mod codegen;
mod config;
mod generate;
mod gen_check;
mod init;
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
    }
}
