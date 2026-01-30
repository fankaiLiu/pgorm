use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTopic {
    Root,
    Gen,
    GenCheck,
    GenInit,
    GenSchema,
}

#[derive(Debug, Clone)]
pub enum Command {
    Help(HelpTopic),
    Gen(GenCommand),
}

#[derive(Debug, Clone)]
pub enum GenCommand {
    Run(GenRunArgs),
    Check(GenCheckArgs),
    Init(GenInitArgs),
    Schema(GenSchemaArgs),
}

#[derive(Debug, Clone)]
pub struct GenRunArgs {
    pub config: PathBuf,
    pub database: Option<String>,
    pub dry_run: bool,
    pub check: bool,
}

#[derive(Debug, Clone)]
pub struct GenCheckArgs {
    pub config: PathBuf,
    pub database: Option<String>,
    pub deny_warnings: bool,
}

#[derive(Debug, Clone)]
pub struct GenInitArgs {
    pub config: PathBuf,
}

#[derive(Debug, Clone)]
pub struct GenSchemaArgs {
    pub config: PathBuf,
    pub database: Option<String>,
    pub schemas: Option<Vec<String>>,
}

pub fn parse_args(args: &[String]) -> anyhow::Result<Command> {
    let mut it = args.iter().skip(1);
    let Some(first) = it.next() else {
        return Ok(Command::Help(HelpTopic::Root));
    };

    match first.as_str() {
        "-h" | "--help" => Ok(Command::Help(HelpTopic::Root)),
        "gen" => parse_gen(it.map(|s| s.as_str())),
        _ => anyhow::bail!("unknown command: {first}"),
    }
}

fn parse_gen<'a>(mut it: impl Iterator<Item = &'a str>) -> anyhow::Result<Command> {
    let mut subcmd: Option<&str> = None;

    let mut config = PathBuf::from("pgorm.toml");
    let mut database: Option<String> = None;

    let mut dry_run = false;
    let mut check = false;

    let mut deny_warnings = false;
    let mut schemas: Option<Vec<String>> = None;

    while let Some(token) = it.next() {
        match token {
            "-h" | "--help" => {
                return Ok(Command::Help(match subcmd {
                    None => HelpTopic::Gen,
                    Some("check") => HelpTopic::GenCheck,
                    Some("init") => HelpTopic::GenInit,
                    Some("schema") => HelpTopic::GenSchema,
                    Some(other) => anyhow::bail!("unknown subcommand: {other}"),
                }));
            }
            "check" | "init" | "schema" if !token.starts_with('-') && subcmd.is_none() => {
                subcmd = Some(token);
            }
            "--config" => {
                let Some(v) = it.next() else {
                    anyhow::bail!("--config requires a value");
                };
                config = PathBuf::from(v);
            }
            _ if token.starts_with("--config=") => {
                config = PathBuf::from(token.trim_start_matches("--config="));
            }
            "--database" => {
                let Some(v) = it.next() else {
                    anyhow::bail!("--database requires a value");
                };
                database = Some(v.to_string());
            }
            _ if token.starts_with("--database=") => {
                database = Some(token.trim_start_matches("--database=").to_string());
            }
            "--dry-run" => dry_run = true,
            "--check" => check = true,
            "--deny-warnings" => deny_warnings = true,
            "--schemas" => {
                let Some(v) = it.next() else {
                    anyhow::bail!("--schemas requires a value");
                };
                let parsed = split_csv(v);
                if parsed.is_empty() {
                    anyhow::bail!("--schemas must not be empty");
                }
                schemas = Some(parsed);
            }
            _ if token.starts_with("--schemas=") => {
                let parsed = split_csv(token.trim_start_matches("--schemas="));
                if parsed.is_empty() {
                    anyhow::bail!("--schemas must not be empty");
                }
                schemas = Some(parsed);
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    let cmd = match subcmd {
        None => GenCommand::Run(GenRunArgs {
            config,
            database,
            dry_run,
            check,
        }),
        Some("check") => GenCommand::Check(GenCheckArgs {
            config,
            database,
            deny_warnings,
        }),
        Some("init") => GenCommand::Init(GenInitArgs { config }),
        Some("schema") => GenCommand::Schema(GenSchemaArgs {
            config,
            database,
            schemas,
        }),
        Some(other) => anyhow::bail!("unknown subcommand: {other}"),
    };

    Ok(Command::Gen(cmd))
}

fn split_csv(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn print_help(topic: HelpTopic) {
    match topic {
        HelpTopic::Root => {
            println!(
                "\
pgorm - SQL codegen CLI for pgorm

USAGE:
  pgorm gen <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
  gen           Generate Rust from SQL (sqlc-like)

Run `pgorm gen --help` for more."
            );
        }
        HelpTopic::Gen => {
            println!(
                "\
USAGE:
  pgorm gen [OPTIONS]
  pgorm gen check [OPTIONS]
  pgorm gen init [OPTIONS]
  pgorm gen schema [OPTIONS]

GLOBAL OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Override database.url from config
  -h, --help            Print help

GEN OPTIONS:
  --dry-run             Print files that would change
  --check               Exit non-zero if output would change

CHECK OPTIONS:
  --deny-warnings       Treat warnings as errors

SCHEMA OPTIONS:
  --schemas <CSV>       Comma-separated schema list (default: public)"
            );
        }
        HelpTopic::GenCheck => {
            println!(
                "\
USAGE:
  pgorm gen check [OPTIONS]

OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Override database.url from config
  --deny-warnings       Treat warnings as errors
  -h, --help            Print help"
            );
        }
        HelpTopic::GenInit => {
            println!(
                "\
USAGE:
  pgorm gen init [OPTIONS]

OPTIONS:
  --config <FILE>       Output config path (default: pgorm.toml)
  -h, --help            Print help"
            );
        }
        HelpTopic::GenSchema => {
            println!(
                "\
USAGE:
  pgorm gen schema [OPTIONS]

OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Database URL (overrides config)
  --schemas <CSV>       Comma-separated schema list (default: public)
  -h, --help            Print help"
            );
        }
    }
}
