//! Example demonstrating PostgreSQL LISTEN/NOTIFY with pgorm.
//!
//! Run with:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example \
//!   cargo run --example listen_notify -p pgorm

use pgorm::{OrmError, OrmResult, PgListener, PgListenerConfig, PgListenerQueuePolicy};
use std::env;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_postgres::NoTls;

#[tokio::main]
async fn main() -> OrmResult<()> {
    let database_url = env::var("DATABASE_URL")
        .map_err(|_| OrmError::Connection("DATABASE_URL is not set".to_string()))?;

    let cfg = PgListenerConfig::new()
        .queue_capacity(256)
        .queue_policy(PgListenerQueuePolicy::DropNewest)
        .reconnect(true)
        .reconnect_backoff(Duration::from_millis(250), Duration::from_secs(5));

    let mut listener = PgListener::connect_with_no_tls_config(&database_url, cfg).await?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before UNIX_EPOCH")
        .as_nanos();
    let channel = format!("pgorm_demo_events_{}_{}", std::process::id(), nanos);
    listener.listen(&channel).await?;

    let (notify_client, notify_connection) = tokio_postgres::connect(&database_url, NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = notify_connection.await;
    });

    notify_client
        .query("SELECT pg_notify($1, $2)", &[&channel, &"hello from pgorm"])
        .await
        .map_err(OrmError::from_db_error)?;

    let msg = tokio::time::timeout(Duration::from_secs(5), listener.next())
        .await
        .map_err(|_| OrmError::Timeout(Duration::from_secs(5)))?
        .ok_or_else(|| OrmError::Connection("listener closed".to_string()))??;

    println!(
        "received channel={} payload={} pid={}",
        msg.channel, msg.payload, msg.process_id
    );
    println!(
        "listener_state={:?} reconnects={} dropped={}",
        listener.state(),
        listener.stats().reconnect_count,
        listener.stats().dropped_notifications
    );

    listener.unlisten(&channel).await?;
    listener.close().await?;
    Ok(())
}
