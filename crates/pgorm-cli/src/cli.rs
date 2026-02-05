use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTopic {
    Root,
    Gen,
    GenCheck,
    GenInit,
    GenSchema,
    Model,
    Sql,
    SqlCheck,
}

#[derive(Debug, Clone)]
pub enum Command {
    Help(HelpTopic),
    Gen(GenCommand),
    Model(ModelRunArgs),
    Sql(SqlCommand),
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

#[derive(Debug, Clone)]
pub struct ModelRunArgs {
    pub config: PathBuf,
    pub database: Option<String>,
    pub dry_run: bool,
    pub check: bool,
}

#[derive(Debug, Clone)]
pub enum SqlCommand {
    Check(SqlCheckArgs),
}

#[derive(Debug, Clone)]
pub struct SqlCheckArgs {
    pub config: PathBuf,
    pub database: Option<String>,
    pub schemas: Option<Vec<String>>,
    pub deny_warnings: bool,
    pub files: Vec<PathBuf>,
}

pub fn parse_args(args: &[String]) -> anyhow::Result<Command> {
    let mut it = args.iter().skip(1);
    let Some(first) = it.next() else {
        return Ok(Command::Help(HelpTopic::Root));
    };

    match first.as_str() {
        "-h" | "--help" => Ok(Command::Help(HelpTopic::Root)),
        "gen" => parse_gen(it.map(|s| s.as_str())),
        "model" => parse_model(it.map(|s| s.as_str())),
        "sql" => parse_sql(it.map(|s| s.as_str())),
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
            "check" | "init" | "schema" if subcmd.is_none() => {
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

fn parse_model<'a>(mut it: impl Iterator<Item = &'a str>) -> anyhow::Result<Command> {
    let mut config = PathBuf::from("pgorm.toml");
    let mut database: Option<String> = None;
    let mut dry_run = false;
    let mut check = false;

    while let Some(token) = it.next() {
        match token {
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Model)),
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
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    Ok(Command::Model(ModelRunArgs {
        config,
        database,
        dry_run,
        check,
    }))
}

fn parse_sql<'a>(mut it: impl Iterator<Item = &'a str>) -> anyhow::Result<Command> {
    let mut subcmd: Option<&str> = None;

    let mut config = PathBuf::from("pgorm.toml");
    let mut database: Option<String> = None;
    let mut schemas: Option<Vec<String>> = None;
    let mut deny_warnings = false;
    let mut files: Vec<PathBuf> = Vec::new();

    while let Some(token) = it.next() {
        match token {
            "-h" | "--help" => {
                return Ok(Command::Help(match subcmd {
                    None => HelpTopic::Sql,
                    Some("check") => HelpTopic::SqlCheck,
                    Some(other) => anyhow::bail!("unknown subcommand: {other}"),
                }));
            }
            "check" if subcmd.is_none() => {
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
            "--deny-warnings" => deny_warnings = true,
            other if other.starts_with('-') => anyhow::bail!("unknown argument: {other}"),
            other => files.push(PathBuf::from(other)),
        }
    }

    let cmd = match subcmd {
        None => {
            // Treat `pgorm sql` (no args) as help, but keep other cases strict.
            if files.is_empty()
                && database.is_none()
                && schemas.is_none()
                && !deny_warnings
                && config == PathBuf::from("pgorm.toml")
            {
                return Ok(Command::Help(HelpTopic::Sql));
            }
            anyhow::bail!("missing subcommand: expected `pgorm sql check`")
        }
        Some("check") => SqlCommand::Check(SqlCheckArgs {
            config,
            database,
            schemas,
            deny_warnings,
            files,
        }),
        Some(other) => anyhow::bail!("unknown subcommand: {other}"),
    };

    Ok(Command::Sql(cmd))
}

pub fn print_help(topic: HelpTopic) {
    match topic {
        HelpTopic::Root => {
            println!(
                "\
pgorm - SQL codegen CLI for pgorm

USAGE:
  pgorm <COMMAND> [OPTIONS]

COMMANDS:
  gen           Generate Rust from SQL (sqlc-like)
  model         Generate Rust models from schema
  sql           Check raw SQL against schema

Run `pgorm gen --help`, `pgorm model --help`, or `pgorm sql --help` for more."
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
        HelpTopic::Model => {
            println!(
                "\
USAGE:
  pgorm model [OPTIONS]

NOTES:
  Requires a [models] section in the config file.

OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Override database.url from config
  --dry-run             Print files that would change
  --check               Exit non-zero if output would change
  -h, --help            Print help"
            );
        }
        HelpTopic::Sql => {
            println!(
                "\
USAGE:
  pgorm sql check [OPTIONS] [FILES...]

SUBCOMMANDS:
  check         Validate SQL syntax/lint/schema (reads stdin if no files)

Run `pgorm sql check --help` for more."
            );
        }
        HelpTopic::SqlCheck => {
            println!(
                "\
USAGE:
  pgorm sql check [OPTIONS] [FILES...]

NOTES:
  - Supports multi-statement input; each statement is validated separately.

OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Database URL (overrides config)
  --schemas <CSV>       Comma-separated schema list (default: from config or public)
  --deny-warnings       Treat warnings as errors
  -h, --help            Print help"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sql_check_with_files() {
        let args = vec![
            "pgorm".to_string(),
            "sql".to_string(),
            "check".to_string(),
            "--config".to_string(),
            "pgorm.toml".to_string(),
            "--deny-warnings".to_string(),
            "a.sql".to_string(),
            "b.sql".to_string(),
        ];

        let cmd = parse_args(&args).unwrap();
        let Command::Sql(SqlCommand::Check(sql)) = cmd else {
            panic!("expected sql check");
        };

        assert_eq!(sql.config, PathBuf::from("pgorm.toml"));
        assert!(sql.deny_warnings);
        assert_eq!(
            sql.files,
            vec![PathBuf::from("a.sql"), PathBuf::from("b.sql")]
        );
    }
}
