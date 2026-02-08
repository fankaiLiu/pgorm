use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTopic {
    Root,
    Init,
    Build,
    Check,
    Schema,
    Sql,
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
    Init(InitArgs),
    Build(BuildArgs),
    Check(CheckArgs),
    Schema(GenSchemaArgs),
    Sql(SqlCommand),
    Migrate(MigrateCommand),
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
pub struct InitArgs {
    pub config: PathBuf,
    pub migrations_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct BuildArgs {
    pub config: PathBuf,
    pub database: Option<String>,
    pub dry_run: bool,
    pub check: bool,
    pub skip_queries: bool,
    pub skip_models: bool,
}

#[derive(Debug, Clone)]
pub struct CheckArgs {
    pub config: PathBuf,
    pub database: Option<String>,
    pub deny_warnings: bool,
    pub skip_queries: bool,
    pub skip_models: bool,
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
        "init" => parse_init(it.map(|s| s.as_str())),
        "build" => parse_build(it.map(|s| s.as_str())),
        "check" => parse_check(it.map(|s| s.as_str())),
        "schema" => parse_schema(it.map(|s| s.as_str())),
        "sql" => parse_sql(it.map(|s| s.as_str())),
        "migrate" => parse_migrate(it.map(|s| s.as_str())),
        _ => anyhow::bail!("unknown command: {first}"),
    }
}

fn split_csv(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn parse_init<'a>(mut it: impl Iterator<Item = &'a str>) -> anyhow::Result<Command> {
    let mut config = PathBuf::from("pgorm.toml");
    let mut migrations_dir = Some(PathBuf::from("migrations"));
    let mut migrations_dir_set = false;

    while let Some(token) = it.next() {
        match token {
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Init)),
            "--config" => {
                let Some(v) = it.next() else {
                    anyhow::bail!("--config requires a value");
                };
                config = PathBuf::from(v);
            }
            _ if token.starts_with("--config=") => {
                config = PathBuf::from(token.trim_start_matches("--config="));
            }
            "--migrations-dir" => {
                let Some(v) = it.next() else {
                    anyhow::bail!("--migrations-dir requires a value");
                };
                migrations_dir = Some(PathBuf::from(v));
                migrations_dir_set = true;
            }
            _ if token.starts_with("--migrations-dir=") => {
                migrations_dir = Some(PathBuf::from(token.trim_start_matches("--migrations-dir=")));
                migrations_dir_set = true;
            }
            "--no-migrations" => migrations_dir = None,
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    if migrations_dir.is_none() && migrations_dir_set {
        anyhow::bail!("--migrations-dir cannot be used with --no-migrations");
    }

    Ok(Command::Init(InitArgs {
        config,
        migrations_dir,
    }))
}

fn parse_build<'a>(mut it: impl Iterator<Item = &'a str>) -> anyhow::Result<Command> {
    let mut config = PathBuf::from("pgorm.toml");
    let mut database: Option<String> = None;
    let mut dry_run = false;
    let mut check = false;
    let mut skip_queries = false;
    let mut skip_models = false;

    while let Some(token) = it.next() {
        match token {
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Build)),
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
            "--no-queries" => skip_queries = true,
            "--no-models" => skip_models = true,
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    if skip_queries && skip_models {
        anyhow::bail!("nothing selected: --no-queries and --no-models cannot be used together");
    }

    Ok(Command::Build(BuildArgs {
        config,
        database,
        dry_run,
        check,
        skip_queries,
        skip_models,
    }))
}

fn parse_check<'a>(mut it: impl Iterator<Item = &'a str>) -> anyhow::Result<Command> {
    let mut config = PathBuf::from("pgorm.toml");
    let mut database: Option<String> = None;
    let mut deny_warnings = false;
    let mut skip_queries = false;
    let mut skip_models = false;

    while let Some(token) = it.next() {
        match token {
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Check)),
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
            "--deny-warnings" => deny_warnings = true,
            "--no-queries" => skip_queries = true,
            "--no-models" => skip_models = true,
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    if skip_queries && skip_models {
        anyhow::bail!("nothing selected: --no-queries and --no-models cannot be used together");
    }

    Ok(Command::Check(CheckArgs {
        config,
        database,
        deny_warnings,
        skip_queries,
        skip_models,
    }))
}

fn parse_schema<'a>(mut it: impl Iterator<Item = &'a str>) -> anyhow::Result<Command> {
    let mut config = PathBuf::from("pgorm.toml");
    let mut database: Option<String> = None;
    let mut schemas: Option<Vec<String>> = None;

    while let Some(token) = it.next() {
        match token {
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Schema)),
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
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    Ok(Command::Schema(GenSchemaArgs {
        config,
        database,
        schemas,
    }))
}

fn parse_sql<'a>(mut it: impl Iterator<Item = &'a str>) -> anyhow::Result<Command> {
    let mut config = PathBuf::from("pgorm.toml");
    let mut database: Option<String> = None;
    let mut schemas: Option<Vec<String>> = None;
    let mut deny_warnings = false;
    let mut files: Vec<PathBuf> = Vec::new();

    while let Some(token) = it.next() {
        match token {
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Sql)),
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
            "check" if files.is_empty() => {
                anyhow::bail!("`pgorm sql check` has been removed; use `pgorm sql [FILES...]`")
            }
            other => files.push(PathBuf::from(other)),
        }
    }

    if files.is_empty()
        && database.is_none()
        && schemas.is_none()
        && !deny_warnings
        && config == PathBuf::from("pgorm.toml")
    {
        return Ok(Command::Help(HelpTopic::Sql));
    }

    Ok(Command::Sql(SqlCommand::Check(SqlCheckArgs {
        config,
        database,
        schemas,
        deny_warnings,
        files,
    })))
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
pgorm - project workflow CLI for pgorm

USAGE:
  pgorm <COMMAND> [OPTIONS]

COMMANDS:
  init          Create pgorm.toml (+ migrations dir by default)
  build         Generate project outputs (queries + models)
  check         CI checks for generated outputs and SQL
  schema        Refresh schema cache
  sql           Validate SQL files/stdin
  migrate       Migration workflow (init/new/up/down/status/diff)

Run `pgorm <command> --help` for more."
            );
        }
        HelpTopic::Init => {
            println!(
                "\
USAGE:
  pgorm init [OPTIONS]

DESCRIPTION:
  Bootstraps project files:
  - writes pgorm.toml template
  - creates migrations directory by default

OPTIONS:
  --config <FILE>           Output config path (default: pgorm.toml)
  --migrations-dir <DIR>    Migration directory (default: migrations)
  --no-migrations           Skip migration directory initialization
  -h, --help                Print help"
            );
        }
        HelpTopic::Build => {
            println!(
                "\
USAGE:
  pgorm build [OPTIONS]

DESCRIPTION:
  Generates project outputs from pgorm.toml:
  - SQL package codegen (from [[packages]])
  - model codegen (from [models], if present)

OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Override database.url from config
  --dry-run             Print files that would change
  --check               Exit non-zero if output would change
  --no-queries          Skip [[packages]] code generation
  --no-models           Skip [models] code generation
  -h, --help            Print help"
            );
        }
        HelpTopic::Check => {
            println!(
                "\
USAGE:
  pgorm check [OPTIONS]

DESCRIPTION:
  Non-mutating project checks:
  - validate package SQL and query codegen inputs
  - verify model generated files are up to date (if [models] exists)

OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Override database.url from config
  --deny-warnings       Treat SQL warnings as errors
  --no-queries          Skip [[packages]] checks
  --no-models           Skip [models] checks
  -h, --help            Print help"
            );
        }
        HelpTopic::Schema => {
            println!(
                "\
USAGE:
  pgorm schema [OPTIONS]

DESCRIPTION:
  Refresh or dump schema cache from database.

OPTIONS:
  --config <FILE>       Config file path (default: pgorm.toml)
  --database <URL>      Database URL (overrides config)
  --schemas <CSV>       Comma-separated schema list (default: public)
  -h, --help            Print help"
            );
        }
        HelpTopic::Sql => {
            println!(
                "\
USAGE:
  pgorm sql [OPTIONS] [FILES...]

NOTES:
  - Reads stdin if no files are provided.
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
    fn parse_sql_with_files() {
        let args = vec![
            "pgorm".to_string(),
            "sql".to_string(),
            "--config".to_string(),
            "pgorm.toml".to_string(),
            "--deny-warnings".to_string(),
            "a.sql".to_string(),
            "b.sql".to_string(),
        ];

        let cmd = parse_args(&args).unwrap();
        let Command::Sql(SqlCommand::Check(sql)) = cmd else {
            panic!("expected sql command");
        };

        assert_eq!(sql.config, PathBuf::from("pgorm.toml"));
        assert!(sql.deny_warnings);
        assert_eq!(
            sql.files,
            vec![PathBuf::from("a.sql"), PathBuf::from("b.sql")]
        );
    }

    #[test]
    fn parse_sql_without_check_subcommand() {
        let args = vec![
            "pgorm".to_string(),
            "sql".to_string(),
            "--deny-warnings".to_string(),
            "q.sql".to_string(),
        ];

        let cmd = parse_args(&args).unwrap();
        let Command::Sql(SqlCommand::Check(sql)) = cmd else {
            panic!("expected sql command");
        };
        assert!(sql.deny_warnings);
        assert_eq!(sql.files, vec![PathBuf::from("q.sql")]);
    }

    #[test]
    fn parse_legacy_gen_is_unknown_command() {
        let args = vec!["pgorm".to_string(), "gen".to_string()];
        let err = parse_args(&args).unwrap_err();
        assert!(err.to_string().contains("unknown command: gen"));
    }

    #[test]
    fn parse_legacy_model_is_unknown_command() {
        let args = vec!["pgorm".to_string(), "model".to_string()];
        let err = parse_args(&args).unwrap_err();
        assert!(err.to_string().contains("unknown command: model"));
    }

    #[test]
    fn parse_legacy_sql_check_is_removed() {
        let args = vec![
            "pgorm".to_string(),
            "sql".to_string(),
            "check".to_string(),
            "a.sql".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err.to_string().contains("has been removed"));
    }

    #[test]
    fn parse_build_defaults() {
        let args = vec!["pgorm".to_string(), "build".to_string()];

        let cmd = parse_args(&args).unwrap();
        let Command::Build(build) = cmd else {
            panic!("expected build");
        };
        assert_eq!(build.config, PathBuf::from("pgorm.toml"));
        assert!(build.database.is_none());
        assert!(!build.skip_models);
        assert!(!build.skip_queries);
    }

    #[test]
    fn parse_init_no_migrations() {
        let args = vec![
            "pgorm".to_string(),
            "init".to_string(),
            "--config=cfg.toml".to_string(),
            "--no-migrations".to_string(),
        ];

        let cmd = parse_args(&args).unwrap();
        let Command::Init(init) = cmd else {
            panic!("expected init");
        };
        assert_eq!(init.config, PathBuf::from("cfg.toml"));
        assert!(init.migrations_dir.is_none());
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
