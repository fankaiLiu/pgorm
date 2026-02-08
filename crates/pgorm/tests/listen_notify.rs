use pgorm::{OrmError, OrmResult, PgListener, PgListenerState};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_postgres::NoTls;

#[tokio::test]
async fn listen_notify_roundtrip() -> OrmResult<()> {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("DATABASE_URL is not set; skipping listen_notify_roundtrip");
            return Ok(());
        }
    };

    let mut listener = PgListener::connect(&database_url).await?;
    assert_eq!(listener.state(), PgListenerState::Connected);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before UNIX_EPOCH")
        .as_nanos();
    let channel = format!("pgorm_test_listen_{}_{}", std::process::id(), nanos);
    let payload = format!("payload-{nanos}");

    listener.listen(&channel).await?;

    let (notify_client, notify_connection) = tokio_postgres::connect(&database_url, NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = notify_connection.await;
    });

    notify_client
        .query("SELECT pg_notify($1, $2)", &[&channel, &payload])
        .await
        .map_err(OrmError::from_db_error)?;

    let msg = tokio::time::timeout(Duration::from_secs(5), listener.next())
        .await
        .map_err(|_| OrmError::Timeout(Duration::from_secs(5)))?
        .ok_or_else(|| OrmError::Connection("listener closed".to_string()))??;

    assert_eq!(msg.channel, channel);
    assert_eq!(msg.payload, payload);
    assert_eq!(listener.state(), PgListenerState::Connected);
    let stats = listener.stats();
    assert_eq!(stats.reconnect_count, 0);

    listener.unlisten(&channel).await?;
    listener.close().await?;
    Ok(())
}
