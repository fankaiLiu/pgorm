//! SQL migrations via [`refinery`].
//!
//! `pgorm` keeps migration definitions in your application crate (or a dedicated migrations crate)
//! and provides helpers for `up/down/status/diff` workflows.
//!
//! # Example (embedded SQL migrations)
//!
//! ```ignore
//! use pgorm::{create_pool, migrate};
//! use std::env;
//!
//! mod embedded {
//!     use pgorm::embed_migrations;
//!     embed_migrations!("./migrations");
//! }
//!
//! # async fn main_impl() -> pgorm::OrmResult<()> {
//! let pool = create_pool(&env::var("DATABASE_URL")?)?;
//! migrate::run_pool(&pool, embedded::migrations::runner()).await?;
//! # Ok(()) }
//! ```

use crate::error::{OrmError, OrmResult};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub use refinery::{Error, Migration, Report, Runner, SchemaVersion, Target, embed_migrations};

const DEFAULT_MIGRATION_TABLE: &str = "refinery_schema_history";

#[derive(Debug, Clone, PartialEq, Eq)]
enum MigrationFileKind {
    Up,
    Down,
}

/// Migration file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskMigration {
    pub version: i64,
    pub name: String,
    pub up_path: PathBuf,
    pub down_path: Option<PathBuf>,
}

/// Applied migration row from `refinery_schema_history`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMigration {
    pub version: i64,
    pub name: String,
    pub applied_on: Option<String>,
    pub checksum: Option<u64>,
}

/// Computed migration status for a directory + database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationStatus {
    pub local: Vec<DiskMigration>,
    pub applied: Vec<AppliedMigration>,
    pub pending: Vec<DiskMigration>,
    pub missing_local: Vec<AppliedMigration>,
}

fn parse_version_and_name_with_suffix(file_name: &str, suffix: &str) -> Option<(i64, String)> {
    if !file_name.starts_with('V') {
        return None;
    }
    let stem = file_name.strip_suffix(suffix)?;
    let (version_str, name) = stem[1..].split_once("__")?;
    if name.is_empty() {
        return None;
    }
    let version = version_str.parse::<i64>().ok()?;
    if version <= 0 {
        return None;
    }
    Some((version, name.to_string()))
}

fn parse_migration_filename(file_name: &str) -> Option<(i64, String, MigrationFileKind)> {
    if let Some((version, name)) = parse_version_and_name_with_suffix(file_name, ".down.sql") {
        return Some((version, name, MigrationFileKind::Down));
    }
    if let Some((version, name)) = parse_version_and_name_with_suffix(file_name, ".up.sql") {
        return Some((version, name, MigrationFileKind::Up));
    }
    parse_version_and_name_with_suffix(file_name, ".sql")
        .map(|(version, name)| (version, name, MigrationFileKind::Up))
}

#[derive(Debug, Clone)]
struct PartialDiskMigration {
    name: String,
    up_path: Option<PathBuf>,
    down_path: Option<PathBuf>,
}

/// Scan a migrations directory.
///
/// Supported file names:
/// - `V1__init.sql` (up)
/// - `V2__add_users.up.sql` (up)
/// - `V2__add_users.down.sql` (down)
pub fn scan_migrations_dir(dir: impl AsRef<Path>) -> OrmResult<Vec<DiskMigration>> {
    let dir = dir.as_ref();
    let entries = fs::read_dir(dir).map_err(|e| {
        OrmError::Other(format!(
            "failed to read migrations dir {}: {e}",
            dir.display()
        ))
    })?;

    let mut by_version: BTreeMap<i64, PartialDiskMigration> = BTreeMap::new();

    for entry in entries {
        let entry = entry.map_err(|e| {
            OrmError::Other(format!("failed to read entry in {}: {e}", dir.display()))
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };

        let Some((version, name, kind)) = parse_migration_filename(file_name) else {
            continue;
        };

        let slot = by_version
            .entry(version)
            .or_insert_with(|| PartialDiskMigration {
                name: name.clone(),
                up_path: None,
                down_path: None,
            });

        if slot.name != name {
            return Err(OrmError::Other(format!(
                "conflicting migration names for version {version}: '{}' vs '{}'",
                slot.name, name
            )));
        }

        match kind {
            MigrationFileKind::Up => {
                if slot.up_path.is_some() {
                    return Err(OrmError::Other(format!(
                        "duplicate up migration for version {version}"
                    )));
                }
                slot.up_path = Some(path);
            }
            MigrationFileKind::Down => {
                if slot.down_path.is_some() {
                    return Err(OrmError::Other(format!(
                        "duplicate down migration for version {version}"
                    )));
                }
                slot.down_path = Some(path);
            }
        }
    }

    let mut out = Vec::with_capacity(by_version.len());
    for (version, partial) in by_version {
        let Some(up_path) = partial.up_path else {
            return Err(OrmError::Other(format!(
                "migration V{version}__{} has down.sql but no up.sql",
                partial.name
            )));
        };
        out.push(DiskMigration {
            version,
            name: partial.name,
            up_path,
            down_path: partial.down_path,
        });
    }

    Ok(out)
}

fn version_to_schema(v: i64) -> OrmResult<SchemaVersion> {
    SchemaVersion::try_from(v)
        .map_err(|_| OrmError::Other(format!("migration version out of range: {v}")))
}

fn build_runner_from_dir(dir: &Path, target_version: Option<i64>) -> OrmResult<Runner> {
    let local = scan_migrations_dir(dir)?;
    let mut migrations = Vec::with_capacity(local.len());

    for m in local {
        let sql = fs::read_to_string(&m.up_path).map_err(|e| {
            OrmError::Other(format!(
                "failed to read migration {}: {e}",
                m.up_path.display()
            ))
        })?;

        // Feed refinery with canonical names regardless of *.up.sql on disk.
        let canonical_name = format!("V{}__{}.sql", m.version, m.name);
        migrations.push(Migration::unapplied(&canonical_name, &sql)?);
    }

    let mut runner = Runner::new(&migrations);
    if let Some(v) = target_version {
        runner = runner.set_target(Target::Version(version_to_schema(v)?));
    }
    Ok(runner)
}

fn quote_table_name(table_name: &str) -> OrmResult<String> {
    let mut parts = Vec::new();
    for part in table_name.split('.') {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(OrmError::Other(format!(
                "invalid migration table name: {table_name}"
            )));
        }
        parts.push(format!("\"{part}\""));
    }
    Ok(parts.join("."))
}

async fn fetch_applied(
    client: &mut tokio_postgres::Client,
    table_name: &str,
) -> OrmResult<Vec<AppliedMigration>> {
    let table_name = quote_table_name(table_name)?;
    let sql = format!(
        "SELECT version::bigint AS version, name, applied_on::text AS applied_on, checksum::text AS checksum \
         FROM {table_name} ORDER BY version ASC"
    );

    let rows = match tokio_postgres::Client::query(client, &sql, &[]).await {
        Ok(rows) => rows,
        Err(err) => {
            if err
                .as_db_error()
                .is_some_and(|db| db.code().code() == "42P01")
            {
                return Ok(Vec::new());
            }
            return Err(OrmError::from_db_error(err));
        }
    };

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let version: i64 = row.get("version");
        let name: String = row.get("name");
        let applied_on: Option<String> = row.get("applied_on");
        let checksum_raw: Option<String> = row.get("checksum");
        out.push(AppliedMigration {
            version,
            name,
            applied_on,
            checksum: checksum_raw.and_then(|v| v.parse::<u64>().ok()),
        });
    }
    Ok(out)
}

/// Run migrations on a single PostgreSQL connection.
pub async fn run(client: &mut tokio_postgres::Client, runner: Runner) -> OrmResult<Report> {
    Ok(runner.run_async(client).await?)
}

/// Run all up migrations from a directory.
pub async fn up_dir(
    client: &mut tokio_postgres::Client,
    dir: impl AsRef<Path>,
) -> OrmResult<Report> {
    let runner = build_runner_from_dir(dir.as_ref(), None)?;
    run(client, runner).await
}

/// Run up migrations from a directory until target version (inclusive).
pub async fn run_to(
    client: &mut tokio_postgres::Client,
    dir: impl AsRef<Path>,
    target_version: i64,
) -> OrmResult<Report> {
    let runner = build_runner_from_dir(dir.as_ref(), Some(target_version))?;
    run(client, runner).await
}

/// Compute migration status for a directory.
pub async fn status(
    client: &mut tokio_postgres::Client,
    dir: impl AsRef<Path>,
) -> OrmResult<MigrationStatus> {
    let local = scan_migrations_dir(dir)?;
    let applied = fetch_applied(client, DEFAULT_MIGRATION_TABLE).await?;

    let applied_versions: HashSet<i64> = applied.iter().map(|m| m.version).collect();
    let local_versions: HashSet<i64> = local.iter().map(|m| m.version).collect();

    let pending = local
        .iter()
        .filter(|m| !applied_versions.contains(&m.version))
        .cloned()
        .collect();
    let missing_local = applied
        .iter()
        .filter(|m| !local_versions.contains(&m.version))
        .cloned()
        .collect();

    Ok(MigrationStatus {
        local,
        applied,
        pending,
        missing_local,
    })
}

/// Return the pending migration list for a directory.
pub async fn plan(
    client: &mut tokio_postgres::Client,
    dir: impl AsRef<Path>,
) -> OrmResult<Vec<DiskMigration>> {
    Ok(status(client, dir).await?.pending)
}

/// Build a SQL draft composed from pending up migrations.
pub async fn diff_pending_sql(
    client: &mut tokio_postgres::Client,
    dir: impl AsRef<Path>,
) -> OrmResult<String> {
    let st = status(client, &dir).await?;
    if st.pending.is_empty() {
        return Ok("-- no pending migrations\n".to_string());
    }

    let mut out = String::new();
    for m in st.pending {
        let sql = fs::read_to_string(&m.up_path).map_err(|e| {
            OrmError::Other(format!(
                "failed to read migration {}: {e}",
                m.up_path.display()
            ))
        })?;
        out.push_str(&format!("-- V{}__{}\n", m.version, m.name));
        out.push_str(sql.trim_end());
        out.push_str("\n\n");
    }
    Ok(out)
}

/// Roll back the latest `steps` migrations using `*.down.sql` files.
///
/// Returns the migrations rolled back, in rollback order (newest first).
pub async fn down_steps(
    client: &mut tokio_postgres::Client,
    dir: impl AsRef<Path>,
    steps: usize,
) -> OrmResult<Vec<AppliedMigration>> {
    if steps == 0 {
        return Ok(Vec::new());
    }

    let local = scan_migrations_dir(&dir)?;
    let local_by_version: HashMap<i64, DiskMigration> =
        local.into_iter().map(|m| (m.version, m)).collect();

    let applied = fetch_applied(client, DEFAULT_MIGRATION_TABLE).await?;
    if steps > applied.len() {
        return Err(OrmError::Other(format!(
            "cannot rollback {steps} step(s): only {} applied migration(s)",
            applied.len()
        )));
    }

    let to_rollback: Vec<AppliedMigration> = applied.iter().rev().take(steps).cloned().collect();
    let table_name = quote_table_name(DEFAULT_MIGRATION_TABLE)?;
    let delete_sql = format!("DELETE FROM {table_name} WHERE version = $1");

    for applied in &to_rollback {
        let Some(local) = local_by_version.get(&applied.version) else {
            return Err(OrmError::Other(format!(
                "cannot rollback V{}__{}: migration file not found in local dir",
                applied.version, applied.name
            )));
        };
        let Some(down_path) = &local.down_path else {
            return Err(OrmError::Other(format!(
                "cannot rollback V{}__{}: missing down migration (.down.sql)",
                local.version, local.name
            )));
        };

        let down_sql = fs::read_to_string(down_path).map_err(|e| {
            OrmError::Other(format!(
                "failed to read down migration {}: {e}",
                down_path.display()
            ))
        })?;

        let tx = client
            .transaction()
            .await
            .map_err(OrmError::from_db_error)?;
        tx.batch_execute(&down_sql)
            .await
            .map_err(OrmError::from_db_error)?;
        let affected = tx
            .execute(&delete_sql, &[&applied.version])
            .await
            .map_err(OrmError::from_db_error)?;
        if affected == 0 {
            return Err(OrmError::Other(format!(
                "failed to update migration history for version {}",
                applied.version
            )));
        }
        tx.commit().await.map_err(OrmError::from_db_error)?;
    }

    Ok(to_rollback)
}

/// Roll back migrations until `target_version` is the latest applied version.
pub async fn down_to(
    client: &mut tokio_postgres::Client,
    dir: impl AsRef<Path>,
    target_version: i64,
) -> OrmResult<Vec<AppliedMigration>> {
    let applied = fetch_applied(client, DEFAULT_MIGRATION_TABLE).await?;
    let steps = applied
        .iter()
        .filter(|m| m.version > target_version)
        .count();
    if steps == 0 {
        return Ok(Vec::new());
    }
    down_steps(client, dir, steps).await
}

/// Acquire a connection from a pool and run migrations on it.
#[cfg(feature = "pool")]
pub async fn run_pool(pool: &deadpool_postgres::Pool, runner: Runner) -> OrmResult<Report> {
    let mut client = pool.get().await?;
    run(&mut client, runner).await
}

/// Pool variant of [`up_dir`].
#[cfg(feature = "pool")]
pub async fn up_dir_pool(
    pool: &deadpool_postgres::Pool,
    dir: impl AsRef<Path>,
) -> OrmResult<Report> {
    let mut client = pool.get().await?;
    up_dir(&mut client, dir).await
}

/// Pool variant of [`run_to`].
#[cfg(feature = "pool")]
pub async fn run_to_pool(
    pool: &deadpool_postgres::Pool,
    dir: impl AsRef<Path>,
    target_version: i64,
) -> OrmResult<Report> {
    let mut client = pool.get().await?;
    run_to(&mut client, dir, target_version).await
}

/// Pool variant of [`status`].
#[cfg(feature = "pool")]
pub async fn status_pool(
    pool: &deadpool_postgres::Pool,
    dir: impl AsRef<Path>,
) -> OrmResult<MigrationStatus> {
    let mut client = pool.get().await?;
    status(&mut client, dir).await
}

/// Pool variant of [`plan`].
#[cfg(feature = "pool")]
pub async fn plan_pool(
    pool: &deadpool_postgres::Pool,
    dir: impl AsRef<Path>,
) -> OrmResult<Vec<DiskMigration>> {
    let mut client = pool.get().await?;
    plan(&mut client, dir).await
}

/// Pool variant of [`diff_pending_sql`].
#[cfg(feature = "pool")]
pub async fn diff_pending_sql_pool(
    pool: &deadpool_postgres::Pool,
    dir: impl AsRef<Path>,
) -> OrmResult<String> {
    let mut client = pool.get().await?;
    diff_pending_sql(&mut client, dir).await
}

/// Pool variant of [`down_steps`].
#[cfg(feature = "pool")]
pub async fn down_steps_pool(
    pool: &deadpool_postgres::Pool,
    dir: impl AsRef<Path>,
    steps: usize,
) -> OrmResult<Vec<AppliedMigration>> {
    let mut client = pool.get().await?;
    down_steps(&mut client, dir, steps).await
}

/// Pool variant of [`down_to`].
#[cfg(feature = "pool")]
pub async fn down_to_pool(
    pool: &deadpool_postgres::Pool,
    dir: impl AsRef<Path>,
    target_version: i64,
) -> OrmResult<Vec<AppliedMigration>> {
    let mut client = pool.get().await?;
    down_to(&mut client, dir, target_version).await
}

#[cfg(test)]
mod tests {
    use super::{MigrationFileKind, parse_migration_filename, scan_migrations_dir};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_dir() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pgorm-migrate-test-{nonce}"));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn parse_migration_filename_variants() {
        let a = parse_migration_filename("V1__init.sql").expect("parse");
        assert_eq!(a.0, 1);
        assert_eq!(a.1, "init");
        assert!(matches!(a.2, MigrationFileKind::Up));

        let b = parse_migration_filename("V2__users.up.sql").expect("parse");
        assert_eq!(b.0, 2);
        assert_eq!(b.1, "users");
        assert!(matches!(b.2, MigrationFileKind::Up));

        let c = parse_migration_filename("V2__users.down.sql").expect("parse");
        assert_eq!(c.0, 2);
        assert_eq!(c.1, "users");
        assert!(matches!(c.2, MigrationFileKind::Down));

        assert!(parse_migration_filename("not_migration.sql").is_none());
    }

    #[test]
    fn scan_migrations_dir_collects_up_down_pairs() {
        let dir = make_temp_dir();
        std::fs::write(dir.join("V1__init.sql"), "CREATE TABLE t1(id int);").expect("write");
        std::fs::write(dir.join("V2__users.up.sql"), "CREATE TABLE users(id int);").expect("write");
        std::fs::write(dir.join("V2__users.down.sql"), "DROP TABLE users;").expect("write");

        let migrations = scan_migrations_dir(&dir).expect("scan");
        assert_eq!(migrations.len(), 2);
        assert_eq!(migrations[0].version, 1);
        assert_eq!(migrations[1].version, 2);
        assert!(migrations[0].down_path.is_none());
        assert!(migrations[1].down_path.is_some());

        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn scan_migrations_dir_rejects_down_without_up() {
        let dir = make_temp_dir();
        std::fs::write(dir.join("V3__x.down.sql"), "DROP TABLE x;").expect("write");

        let err = scan_migrations_dir(&dir).expect_err("must fail");
        assert!(err.to_string().contains("no up.sql"));

        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}
