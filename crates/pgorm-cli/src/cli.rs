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
    Migrate,
    MigrateInit,
    MigrateNew,
    MigrateUp,
    MigrateDown,
    MigrateStatus,
    MigrateDiff,
}

#[derive(Debug, Clone)]
pub enum Command {
    Help(HelpTopic),
    Gen(GenCommand),
    Model(ModelRunArgs),
    Sql(SqlCommand),
    Migrate(MigrateCommand),
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

#[derive(Debug, Clone)]
pub enum MigrateCommand {
    Init(MigrateInitArgs),
    New(MigrateNewArgs),
    Up(MigrateUpArgs),
    Down(MigrateDownArgs),
    Status(MigrateStatusArgs),
    Diff(MigrateDiffArgs),
}

#[derive(Debug, Clone)]
pub struct MigrateInitArgs {
    pub dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct MigrateNewArgs {
    pub dir: PathBuf,
    pub name: String,
    pub with_down: bool,
}

#[derive(Debug, Clone)]
pub struct MigrateUpArgs {
    pub config: PathBuf,
    pub database: Option<String>,
    pub dir: Option<PathBuf>,
    pub to: Option<i64>,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct MigrateDownArgs {
    pub config: PathBuf,
    pub database: Option<String>,
    pub dir: Option<PathBuf>,
    pub to: Option<i64>,
    pub steps: Option<usize>,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct MigrateStatusArgs {
    pub config: PathBuf,
    pub database: Option<String>,
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct MigrateDiffArgs {
    pub config: PathBuf,
    pub database: Option<String>,
    pub dir: Option<PathBuf>,
    pub output: Option<PathBuf>,
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
        "migrate" => parse_migrate(it.map(|s| s.as_str())),
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

fn parse_migrate<'a>(mut it: impl Iterator<Item = &'a str>) -> anyhow::Result<Command> {
    let mut subcmd: Option<&str> = None;

    let mut config = PathBuf::from("pgorm.toml");
    let mut database: Option<String> = None;
    let mut dir: Option<PathBuf> = None;
    let mut to: Option<i64> = None;
    let mut steps: Option<usize> = None;
    let mut output: Option<PathBuf> = None;
    let mut dry_run = false;
    let mut with_down = true;
    let mut new_name: Option<String> = None;

    while let Some(token) = it.next() {
        match token {
            "-h" | "--help" => {
                return Ok(Command::Help(match subcmd {
                    None => HelpTopic::Migrate,
                    Some("init") => HelpTopic::MigrateInit,
                    Some("new") => HelpTopic::MigrateNew,
                    Some("up") => HelpTopic::MigrateUp,
                    Some("down") => HelpTopic::MigrateDown,
                    Some("status") => HelpTopic::MigrateStatus,
                    Some("diff") => HelpTopic::MigrateDiff,
                    Some(other) => anyhow::bail!("unknown subcommand: {other}"),
                }));
            }
            "init" | "new" | "up" | "down" | "status" | "diff" if subcmd.is_none() => {
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
            "--dir" => {
                let Some(v) = it.next() else {
                    anyhow::bail!("--dir requires a value");
                };
                dir = Some(PathBuf::from(v));
            }
            _ if token.starts_with("--dir=") => {
                dir = Some(PathBuf::from(token.trim_start_matches("--dir=")));
            }
            "--to" => {
                let Some(v) = it.next() else {
                    anyhow::bail!("--to requires a value");
                };
                to = Some(
                    v.parse::<i64>()
                        .map_err(|_| anyhow::anyhow!("invalid --to value: {v}"))?,
                );
            }
            _ if token.starts_with("--to=") => {
                let raw = token.trim_start_matches("--to=");
                to = Some(
                    raw.parse::<i64>()
                        .map_err(|_| anyhow::anyhow!("invalid --to value: {raw}"))?,
                );
            }
            "--steps" => {
                let Some(v) = it.next() else {
                    anyhow::bail!("--steps requires a value");
                };
                steps = Some(
                    v.parse::<usize>()
                        .map_err(|_| anyhow::anyhow!("invalid --steps value: {v}"))?,
                );
            }
            _ if token.starts_with("--steps=") => {
                let raw = token.trim_start_matches("--steps=");
                steps = Some(
                    raw.parse::<usize>()
                        .map_err(|_| anyhow::anyhow!("invalid --steps value: {raw}"))?,
                );
            }
            "--output" => {
                let Some(v) = it.next() else {
                    anyhow::bail!("--output requires a value");
                };
                output = Some(PathBuf::from(v));
            }
            _ if token.starts_with("--output=") => {
                output = Some(PathBuf::from(token.trim_start_matches("--output=")));
            }
            "--dry-run" => dry_run = true,
            "--no-down" => with_down = false,
            other if other.starts_with('-') => anyhow::bail!("unknown argument: {other}"),
            other => {
                if matches!(subcmd, Some("new")) && new_name.is_none() {
                    new_name = Some(other.to_string());
                } else {
                    anyhow::bail!("unexpected positional argument: {other}");
                }
            }
        }
    }

    let cmd = match subcmd {
        None => return Ok(Command::Help(HelpTopic::Migrate)),
        Some("init") => {
            if new_name.is_some() || to.is_some() || steps.is_some() || output.is_some() || dry_run
            {
                anyhow::bail!("invalid options for `migrate init`");
            }
            MigrateCommand::Init(MigrateInitArgs {
                dir: dir.unwrap_or_else(|| PathBuf::from("migrations")),
            })
        }
        Some("new") => {
            if to.is_some() || steps.is_some() || output.is_some() || dry_run {
                anyhow::bail!("invalid options for `migrate new`");
            }
            let Some(name) = new_name else {
                anyhow::bail!("missing migration name: usage `pgorm migrate new <name>`");
            };
            MigrateCommand::New(MigrateNewArgs {
                dir: dir.unwrap_or_else(|| PathBuf::from("migrations")),
                name,
                with_down,
            })
        }
        Some("up") => {
            if steps.is_some() || output.is_some() || new_name.is_some() || !with_down {
                anyhow::bail!("invalid options for `migrate up`");
            }
            MigrateCommand::Up(MigrateUpArgs {
                config,
                database,
                dir,
                to,
                dry_run,
            })
        }
        Some("down") => {
            if output.is_some() || new_name.is_some() || !with_down {
                anyhow::bail!("invalid options for `migrate down`");
            }
            if to.is_some() && steps.is_some() {
                anyhow::bail!("`migrate down` accepts either --to or --steps, not both");
            }
            MigrateCommand::Down(MigrateDownArgs {
                config,
                database,
                dir,
                to,
                steps,
                dry_run,
            })
        }
        Some("status") => {
            if to.is_some()
                || steps.is_some()
                || output.is_some()
                || dry_run
                || new_name.is_some()
                || !with_down
            {
                anyhow::bail!("invalid options for `migrate status`");
            }
            MigrateCommand::Status(MigrateStatusArgs {
                config,
                database,
                dir,
            })
        }
        Some("diff") => {
            if to.is_some() || steps.is_some() || dry_run || new_name.is_some() || !with_down {
                anyhow::bail!("invalid options for `migrate diff`");
            }
            MigrateCommand::Diff(MigrateDiffArgs {
                config,
                database,
                dir,
                output,
            })
        }
        Some(other) => anyhow::bail!("unknown subcommand: {other}"),
    };

    Ok(Command::Migrate(cmd))
}

pub fn print_help(topic: HelpTopic) {
    match topic {
        HelpTopic::Root => {
            println!(
                "\
pgorm - SQL codegen and migration CLI for pgorm

USAGE:
  pgorm <COMMAND> [OPTIONS]

COMMANDS:
  gen           Generate Rust from SQL (sqlc-like)
  model         Generate Rust models from schema
  sql           Check raw SQL against schema
  migrate       Migration workflow (init/new/up/down/status/diff)

Run `pgorm <command> --help` for more."
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
        HelpTopic::Migrate => {
            println!(
                "\
USAGE:
  pgorm migrate <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
  init                  Create migrations directory
  new <name>            Create migration skeleton files
  up                    Apply pending up migrations
  down                  Roll back migrations
  status                Show local/applied/pending migrations
  diff                  Render pending migration SQL draft

Run `pgorm migrate <subcommand> --help` for more."
            );
        }
        HelpTopic::MigrateInit => {
            println!(
                "\
USAGE:
  pgorm migrate init [--dir <DIR>]

OPTIONS:
  --dir <DIR>           Migration directory (default: migrations)
  -h, --help            Print help"
            );
        }
        HelpTopic::MigrateNew => {
            println!(
                "\
USAGE:
  pgorm migrate new <name> [OPTIONS]

OPTIONS:
  --dir <DIR>           Migration directory (default: migrations)
  --no-down             Create only up migration file
  -h, --help            Print help"
            );
        }
        HelpTopic::MigrateUp => {
            println!(
                "\
USAGE:
  pgorm migrate up [OPTIONS]

OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Database URL (overrides config)
  --dir <DIR>           Migration directory (default: migrations)
  --to <VERSION>        Apply up to target version (inclusive)
  --dry-run             Print migrations that would be applied
  -h, --help            Print help"
            );
        }
        HelpTopic::MigrateDown => {
            println!(
                "\
USAGE:
  pgorm migrate down [OPTIONS]

OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Database URL (overrides config)
  --dir <DIR>           Migration directory (default: migrations)
  --steps <N>           Roll back latest N migrations (default: 1)
  --to <VERSION>        Roll back until target version
  --dry-run             Print migrations that would be rolled back
  -h, --help            Print help"
            );
        }
        HelpTopic::MigrateStatus => {
            println!(
                "\
USAGE:
  pgorm migrate status [OPTIONS]

OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Database URL (overrides config)
  --dir <DIR>           Migration directory (default: migrations)
  -h, --help            Print help"
            );
        }
        HelpTopic::MigrateDiff => {
            println!(
                "\
USAGE:
  pgorm migrate diff [OPTIONS]

OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Database URL (overrides config)
  --dir <DIR>           Migration directory (default: migrations)
  --output <FILE>       Write draft SQL to file (default: stdout)
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

    #[test]
    fn parse_migrate_new() {
        let args = vec![
            "pgorm".to_string(),
            "migrate".to_string(),
            "new".to_string(),
            "add_users".to_string(),
            "--dir".to_string(),
            "db/migrations".to_string(),
        ];

        let cmd = parse_args(&args).unwrap();
        let Command::Migrate(MigrateCommand::New(m)) = cmd else {
            panic!("expected migrate new");
        };
        assert_eq!(m.name, "add_users");
        assert_eq!(m.dir, PathBuf::from("db/migrations"));
        assert!(m.with_down);
    }

    #[test]
    fn parse_migrate_down_steps() {
        let args = vec![
            "pgorm".to_string(),
            "migrate".to_string(),
            "down".to_string(),
            "--steps=3".to_string(),
            "--dry-run".to_string(),
        ];

        let cmd = parse_args(&args).unwrap();
        let Command::Migrate(MigrateCommand::Down(m)) = cmd else {
            panic!("expected migrate down");
        };
        assert_eq!(m.steps, Some(3));
        assert!(m.dry_run);
        assert_eq!(m.to, None);
    }
}
