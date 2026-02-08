//! PostgreSQL LISTEN/NOTIFY support.
//!
//! This module provides a dedicated listener connection for consuming
//! asynchronous notifications from PostgreSQL channels.

use crate::error::{OrmError, OrmResult};
use futures_core::Stream;
use std::collections::HashSet;
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_postgres::tls::{MakeTlsConnect, TlsConnect};
use tokio_postgres::{AsyncMessage, Socket};

const DEFAULT_QUEUE_CAPACITY: usize = 256;
const DEFAULT_RECONNECT_BACKOFF_MIN: Duration = Duration::from_millis(250);
const DEFAULT_RECONNECT_BACKOFF_MAX: Duration = Duration::from_secs(8);

/// A PostgreSQL notification message.
#[derive(Debug, Clone)]
pub struct PgNotification {
    /// Backend process ID that raised the notification.
    pub process_id: i32,
    /// Channel name.
    pub channel: String,
    /// Payload text.
    pub payload: String,
    /// Local receive timestamp.
    pub received_at: SystemTime,
}

impl PgNotification {
    fn from_tokio(n: tokio_postgres::Notification) -> Self {
        Self {
            process_id: n.process_id(),
            channel: n.channel().to_string(),
            payload: n.payload().to_string(),
            received_at: SystemTime::now(),
        }
    }
}

/// Queue policy when listener's internal channel is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgListenerQueuePolicy {
    /// Drop new incoming notifications.
    DropNewest,
    /// Apply backpressure and wait for the consumer.
    Block,
}

/// Connection state of a listener worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgListenerState {
    Connecting,
    Connected,
    Reconnecting,
    Closed,
}

impl PgListenerState {
    fn as_u8(self) -> u8 {
        match self {
            Self::Connecting => 0,
            Self::Connected => 1,
            Self::Reconnecting => 2,
            Self::Closed => 3,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Connecting,
            1 => Self::Connected,
            2 => Self::Reconnecting,
            _ => Self::Closed,
        }
    }
}

/// Runtime statistics for a listener worker.
#[derive(Debug, Clone, Copy, Default)]
pub struct PgListenerStats {
    /// Number of successful reconnects.
    pub reconnect_count: u64,
    /// Number of dropped notifications due to queue policy.
    pub dropped_notifications: u64,
}

/// Configuration for a [`PgListener`].
#[derive(Debug, Clone)]
pub struct PgListenerConfig {
    /// Internal notification queue capacity.
    pub queue_capacity: usize,
    /// Queue overflow policy.
    pub queue_policy: PgListenerQueuePolicy,
    /// Whether to reconnect automatically after connection loss.
    pub reconnect: bool,
    /// Minimum reconnect backoff.
    pub reconnect_backoff_min: Duration,
    /// Maximum reconnect backoff.
    pub reconnect_backoff_max: Duration,
}

impl Default for PgListenerConfig {
    fn default() -> Self {
        Self {
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            queue_policy: PgListenerQueuePolicy::DropNewest,
            reconnect: true,
            reconnect_backoff_min: DEFAULT_RECONNECT_BACKOFF_MIN,
            reconnect_backoff_max: DEFAULT_RECONNECT_BACKOFF_MAX,
        }
    }
}

impl PgListenerConfig {
    /// Create config with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set queue capacity.
    pub fn queue_capacity(mut self, capacity: usize) -> Self {
        self.queue_capacity = capacity;
        self
    }

    /// Set queue policy.
    pub fn queue_policy(mut self, policy: PgListenerQueuePolicy) -> Self {
        self.queue_policy = policy;
        self
    }

    /// Enable or disable reconnect.
    pub fn reconnect(mut self, reconnect: bool) -> Self {
        self.reconnect = reconnect;
        self
    }

    /// Set reconnect backoff range.
    pub fn reconnect_backoff(mut self, min: Duration, max: Duration) -> Self {
        self.reconnect_backoff_min = min;
        self.reconnect_backoff_max = max;
        self
    }
}

struct ListenerShared {
    state: AtomicU8,
    reconnect_count: AtomicU64,
    dropped_notifications: AtomicU64,
}

impl ListenerShared {
    fn new(initial_state: PgListenerState) -> Self {
        Self {
            state: AtomicU8::new(initial_state.as_u8()),
            reconnect_count: AtomicU64::new(0),
            dropped_notifications: AtomicU64::new(0),
        }
    }

    fn set_state(&self, state: PgListenerState) {
        self.state.store(state.as_u8(), Ordering::Relaxed);
    }

    fn state(&self) -> PgListenerState {
        PgListenerState::from_u8(self.state.load(Ordering::Relaxed))
    }

    fn inc_reconnect(&self) {
        self.reconnect_count.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_dropped_notifications(&self) {
        self.dropped_notifications.fetch_add(1, Ordering::Relaxed);
    }

    fn stats(&self) -> PgListenerStats {
        PgListenerStats {
            reconnect_count: self.reconnect_count.load(Ordering::Relaxed),
            dropped_notifications: self.dropped_notifications.load(Ordering::Relaxed),
        }
    }
}

enum ListenerCommand {
    Listen {
        channel: String,
        resp: oneshot::Sender<OrmResult<()>>,
    },
    Unlisten {
        channel: String,
        resp: oneshot::Sender<OrmResult<()>>,
    },
    UnlistenAll {
        resp: oneshot::Sender<OrmResult<()>>,
    },
    Close {
        resp: oneshot::Sender<OrmResult<()>>,
    },
}

/// Dedicated PostgreSQL LISTEN client.
///
/// A listener uses a dedicated connection and should not share business query
/// connections from your pool.
pub struct PgListener {
    cmd_tx: mpsc::Sender<ListenerCommand>,
    notif_rx: mpsc::Receiver<OrmResult<PgNotification>>,
    worker: Option<JoinHandle<()>>,
    shared: Arc<ListenerShared>,
}

impl PgListener {
    /// Connect a listener with `NoTls` using a PostgreSQL connection URL.
    pub async fn connect(database_url: &str) -> OrmResult<Self> {
        Self::connect_with_config(
            database_url,
            tokio_postgres::NoTls,
            PgListenerConfig::default(),
        )
        .await
    }

    /// Connect with custom config using `NoTls`.
    pub async fn connect_with_no_tls_config(
        database_url: &str,
        config: PgListenerConfig,
    ) -> OrmResult<Self> {
        Self::connect_with_config(database_url, tokio_postgres::NoTls, config).await
    }

    /// Connect with custom TLS connector and config.
    pub async fn connect_with_config<T>(
        database_url: &str,
        tls: T,
        config: PgListenerConfig,
    ) -> OrmResult<Self>
    where
        T: MakeTlsConnect<Socket> + Clone + Sync + Send + 'static,
        T::Stream: Sync + Send,
        T::TlsConnect: Sync + Send,
        <T::TlsConnect as TlsConnect<Socket>>::Future: Send,
    {
        let pg_config: tokio_postgres::Config = database_url
            .parse()
            .map_err(|e: tokio_postgres::Error| OrmError::Connection(e.to_string()))?;

        let shared = Arc::new(ListenerShared::new(PgListenerState::Connecting));

        let (client, connection) = pg_config
            .connect(tls.clone())
            .await
            .map_err(OrmError::from_db_error)?;

        shared.set_state(PgListenerState::Connected);

        let queue_capacity = config.queue_capacity.max(1);
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let (notif_tx, notif_rx) = mpsc::channel(queue_capacity);

        let worker = tokio::spawn(run_listener_loop(
            pg_config,
            tls,
            config,
            ActiveConnection { client, connection },
            cmd_rx,
            notif_tx,
            shared.clone(),
        ));

        Ok(Self {
            cmd_tx,
            notif_rx,
            worker: Some(worker),
            shared,
        })
    }

    /// Execute `LISTEN <channel>`.
    pub async fn listen(&self, channel: &str) -> OrmResult<()> {
        send_command(&self.cmd_tx, |resp| ListenerCommand::Listen {
            channel: channel.to_string(),
            resp,
        })
        .await
    }

    /// Execute `UNLISTEN <channel>`.
    pub async fn unlisten(&self, channel: &str) -> OrmResult<()> {
        send_command(&self.cmd_tx, |resp| ListenerCommand::Unlisten {
            channel: channel.to_string(),
            resp,
        })
        .await
    }

    /// Execute `UNLISTEN *`.
    pub async fn unlisten_all(&self) -> OrmResult<()> {
        send_command(&self.cmd_tx, |resp| ListenerCommand::UnlistenAll { resp }).await
    }

    /// Receive the next notification result.
    ///
    /// Returns `None` when the listener is closed or terminated.
    pub async fn next(&mut self) -> Option<OrmResult<PgNotification>> {
        self.notif_rx.recv().await
    }

    /// Current worker state.
    pub fn state(&self) -> PgListenerState {
        self.shared.state()
    }

    /// Current runtime stats.
    pub fn stats(&self) -> PgListenerStats {
        self.shared.stats()
    }

    /// Convert into a stream of notifications.
    pub fn into_stream(self) -> PgNotificationStream {
        let Self {
            cmd_tx,
            notif_rx,
            worker,
            shared,
        } = self;
        PgNotificationStream {
            cmd_tx,
            inner: notif_rx,
            worker,
            shared,
        }
    }

    /// Gracefully close the listener.
    pub async fn close(mut self) -> OrmResult<()> {
        close_worker(&self.cmd_tx, &mut self.worker).await
    }
}

/// A stream of PostgreSQL notifications from [`PgListener::into_stream`].
#[must_use]
pub struct PgNotificationStream {
    cmd_tx: mpsc::Sender<ListenerCommand>,
    inner: mpsc::Receiver<OrmResult<PgNotification>>,
    worker: Option<JoinHandle<()>>,
    shared: Arc<ListenerShared>,
}

impl PgNotificationStream {
    /// Current worker state.
    pub fn state(&self) -> PgListenerState {
        self.shared.state()
    }

    /// Current runtime stats.
    pub fn stats(&self) -> PgListenerStats {
        self.shared.stats()
    }

    /// Gracefully close the underlying listener worker.
    pub async fn close(mut self) -> OrmResult<()> {
        close_worker(&self.cmd_tx, &mut self.worker).await
    }
}

impl Drop for PgNotificationStream {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}

impl Stream for PgNotificationStream {
    type Item = OrmResult<PgNotification>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.poll_recv(cx)
    }
}

struct ActiveConnection<S, TStream> {
    client: tokio_postgres::Client,
    connection: tokio_postgres::Connection<S, TStream>,
}

enum LoopOutcome {
    CloseRequested,
    Disconnected,
}

enum CommandOutcome {
    Continue,
    CloseRequested,
    Disconnected,
}

async fn run_listener_loop<T>(
    pg_config: tokio_postgres::Config,
    tls: T,
    config: PgListenerConfig,
    mut active: ActiveConnection<Socket, T::Stream>,
    mut cmd_rx: mpsc::Receiver<ListenerCommand>,
    notif_tx: mpsc::Sender<OrmResult<PgNotification>>,
    shared: Arc<ListenerShared>,
) where
    T: MakeTlsConnect<Socket> + Clone + Sync + Send + 'static,
    T::Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    T::TlsConnect: Sync + Send,
    <T::TlsConnect as TlsConnect<Socket>>::Future: Send,
{
    let mut desired_channels = HashSet::<String>::new();

    let min_backoff = config.reconnect_backoff_min.max(Duration::from_millis(1));
    let max_backoff = config.reconnect_backoff_max.max(min_backoff);
    let mut backoff = min_backoff;

    loop {
        shared.set_state(PgListenerState::Connected);

        match run_connected_loop(
            &mut active,
            &mut desired_channels,
            &mut cmd_rx,
            &notif_tx,
            config.queue_policy,
            &shared,
        )
        .await
        {
            LoopOutcome::CloseRequested => {
                shared.set_state(PgListenerState::Closed);
                break;
            }
            LoopOutcome::Disconnected => {
                if !config.reconnect {
                    let _ = notif_tx
                        .send(Err(OrmError::Connection(
                            "listener connection closed".to_string(),
                        )))
                        .await;
                    shared.set_state(PgListenerState::Closed);
                    break;
                }

                shared.set_state(PgListenerState::Reconnecting);

                'reconnect: loop {
                    tokio::select! {
                        cmd = cmd_rx.recv() => {
                            match cmd {
                                Some(cmd) => {
                                    if handle_disconnected_command(cmd, &mut desired_channels) {
                                        shared.set_state(PgListenerState::Closed);
                                        return;
                                    }
                                }
                                None => {
                                    shared.set_state(PgListenerState::Closed);
                                    return;
                                }
                            }
                        }
                        _ = tokio::time::sleep(backoff) => {
                            if let Ok((client, mut connection)) = pg_config.connect(tls.clone()).await {
                                let mut reconnect_ok = true;
                                for channel in desired_channels.iter() {
                                    let channel = match quote_ident(channel) {
                                        Ok(v) => v,
                                        Err(_) => {
                                            reconnect_ok = false;
                                            break;
                                        }
                                    };
                                    let sql = format!("LISTEN {channel}");
                                    if execute_sql_with_polling(
                                        &client,
                                        &mut connection,
                                        &sql,
                                        &notif_tx,
                                        config.queue_policy,
                                        &shared,
                                    )
                                    .await
                                    .is_err()
                                    {
                                        reconnect_ok = false;
                                        break;
                                    }
                                }

                                if reconnect_ok {
                                    active = ActiveConnection { client, connection };
                                    shared.inc_reconnect();
                                    backoff = min_backoff;
                                    break 'reconnect;
                                }
                            }

                            backoff = next_backoff(backoff, max_backoff);
                        }
                    }
                }
            }
        }
    }
}

async fn run_connected_loop<S, TStream>(
    active: &mut ActiveConnection<S, TStream>,
    desired_channels: &mut HashSet<String>,
    cmd_rx: &mut mpsc::Receiver<ListenerCommand>,
    notif_tx: &mpsc::Sender<OrmResult<PgNotification>>,
    queue_policy: PgListenerQueuePolicy,
    shared: &ListenerShared,
) -> LoopOutcome
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    return LoopOutcome::CloseRequested;
                };

                match handle_connected_command(
                    cmd,
                    active,
                    desired_channels,
                    notif_tx,
                    queue_policy,
                    shared,
                ).await {
                    CommandOutcome::Continue => {}
                    CommandOutcome::CloseRequested => return LoopOutcome::CloseRequested,
                    CommandOutcome::Disconnected => return LoopOutcome::Disconnected,
                }
            }
            msg = poll_fn(|cx| active.connection.poll_message(cx)) => {
                match msg {
                    Some(Ok(AsyncMessage::Notification(n))) => {
                        if !dispatch_notification(
                            notif_tx,
                            PgNotification::from_tokio(n),
                            queue_policy,
                            shared,
                        ).await {
                            return LoopOutcome::CloseRequested;
                        }
                    }
                    Some(Ok(AsyncMessage::Notice(_))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(_)) => return LoopOutcome::Disconnected,
                    None => return LoopOutcome::Disconnected,
                }
            }
        }
    }
}

async fn handle_connected_command<S, TStream>(
    cmd: ListenerCommand,
    active: &mut ActiveConnection<S, TStream>,
    desired_channels: &mut HashSet<String>,
    notif_tx: &mpsc::Sender<OrmResult<PgNotification>>,
    queue_policy: PgListenerQueuePolicy,
    shared: &ListenerShared,
) -> CommandOutcome
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    match cmd {
        ListenerCommand::Listen { channel, resp } => {
            let quoted = match quote_ident(&channel) {
                Ok(v) => v,
                Err(e) => {
                    let _ = resp.send(Err(e));
                    return CommandOutcome::Continue;
                }
            };

            desired_channels.insert(channel.clone());
            let sql = format!("LISTEN {quoted}");
            let result = execute_sql_with_polling(
                &active.client,
                &mut active.connection,
                &sql,
                notif_tx,
                queue_policy,
                shared,
            )
            .await;
            let disconnected = result
                .as_ref()
                .err()
                .map(is_disconnect_error)
                .unwrap_or(false);
            let _ = resp.send(result);
            if disconnected {
                return CommandOutcome::Disconnected;
            }
            CommandOutcome::Continue
        }
        ListenerCommand::Unlisten { channel, resp } => {
            let quoted = match quote_ident(&channel) {
                Ok(v) => v,
                Err(e) => {
                    let _ = resp.send(Err(e));
                    return CommandOutcome::Continue;
                }
            };

            desired_channels.remove(&channel);
            let sql = format!("UNLISTEN {quoted}");
            let result = execute_sql_with_polling(
                &active.client,
                &mut active.connection,
                &sql,
                notif_tx,
                queue_policy,
                shared,
            )
            .await;
            let disconnected = result
                .as_ref()
                .err()
                .map(is_disconnect_error)
                .unwrap_or(false);
            let _ = resp.send(result);
            if disconnected {
                return CommandOutcome::Disconnected;
            }
            CommandOutcome::Continue
        }
        ListenerCommand::UnlistenAll { resp } => {
            desired_channels.clear();
            let result = execute_sql_with_polling(
                &active.client,
                &mut active.connection,
                "UNLISTEN *",
                notif_tx,
                queue_policy,
                shared,
            )
            .await;
            let disconnected = result
                .as_ref()
                .err()
                .map(is_disconnect_error)
                .unwrap_or(false);
            let _ = resp.send(result);
            if disconnected {
                return CommandOutcome::Disconnected;
            }
            CommandOutcome::Continue
        }
        ListenerCommand::Close { resp } => {
            desired_channels.clear();
            let result = execute_sql_with_polling(
                &active.client,
                &mut active.connection,
                "UNLISTEN *",
                notif_tx,
                queue_policy,
                shared,
            )
            .await;
            let _ = resp.send(result);
            CommandOutcome::CloseRequested
        }
    }
}

fn handle_disconnected_command(
    cmd: ListenerCommand,
    desired_channels: &mut HashSet<String>,
) -> bool {
    match cmd {
        ListenerCommand::Listen { channel, resp } => {
            let result = quote_ident(&channel).map(|_| {
                desired_channels.insert(channel);
            });
            let _ = resp.send(result);
            false
        }
        ListenerCommand::Unlisten { channel, resp } => {
            let result = quote_ident(&channel).map(|_| {
                desired_channels.remove(&channel);
            });
            let _ = resp.send(result);
            false
        }
        ListenerCommand::UnlistenAll { resp } => {
            desired_channels.clear();
            let _ = resp.send(Ok(()));
            false
        }
        ListenerCommand::Close { resp } => {
            let _ = resp.send(Ok(()));
            true
        }
    }
}

async fn execute_sql_with_polling<S, TStream>(
    client: &tokio_postgres::Client,
    connection: &mut tokio_postgres::Connection<S, TStream>,
    sql: &str,
    notif_tx: &mpsc::Sender<OrmResult<PgNotification>>,
    queue_policy: PgListenerQueuePolicy,
    shared: &ListenerShared,
) -> OrmResult<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let execute = client.batch_execute(sql);
    tokio::pin!(execute);

    loop {
        tokio::select! {
            result = &mut execute => {
                return result.map_err(OrmError::from_db_error);
            }
            msg = poll_fn(|cx| connection.poll_message(cx)) => {
                match msg {
                    Some(Ok(AsyncMessage::Notification(n))) => {
                        if !dispatch_notification(
                            notif_tx,
                            PgNotification::from_tokio(n),
                            queue_policy,
                            shared,
                        ).await {
                            return Err(OrmError::Connection("listener consumer closed".to_string()));
                        }
                    }
                    Some(Ok(AsyncMessage::Notice(_))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(OrmError::from_db_error(e)),
                    None => return Err(OrmError::Connection("listener connection closed".to_string())),
                }
            }
        }
    }
}

async fn dispatch_notification(
    notif_tx: &mpsc::Sender<OrmResult<PgNotification>>,
    notif: PgNotification,
    queue_policy: PgListenerQueuePolicy,
    shared: &ListenerShared,
) -> bool {
    match queue_policy {
        PgListenerQueuePolicy::DropNewest => match notif_tx.try_send(Ok(notif)) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                shared.inc_dropped_notifications();
                true
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        },
        PgListenerQueuePolicy::Block => notif_tx.send(Ok(notif)).await.is_ok(),
    }
}

fn is_disconnect_error(err: &OrmError) -> bool {
    match err {
        OrmError::Connection(_) => true,
        OrmError::Query(e) => e.is_closed(),
        _ => false,
    }
}

fn next_backoff(current: Duration, max: Duration) -> Duration {
    let doubled = current.saturating_mul(2);
    if doubled > max { max } else { doubled }
}

async fn send_command<F>(cmd_tx: &mpsc::Sender<ListenerCommand>, build: F) -> OrmResult<()>
where
    F: FnOnce(oneshot::Sender<OrmResult<()>>) -> ListenerCommand,
{
    let (resp_tx, resp_rx) = oneshot::channel();
    cmd_tx
        .send(build(resp_tx))
        .await
        .map_err(|_| OrmError::Connection("listener command channel is closed".to_string()))?;
    resp_rx.await.map_err(|_| {
        OrmError::Connection("listener worker closed before command reply".to_string())
    })?
}

async fn join_worker(worker: &mut Option<JoinHandle<()>>) -> OrmResult<()> {
    if let Some(worker) = worker.take() {
        worker
            .await
            .map_err(|e| OrmError::Other(format!("listener worker join error: {e}")))?;
    }
    Ok(())
}

async fn close_worker(
    cmd_tx: &mpsc::Sender<ListenerCommand>,
    worker: &mut Option<JoinHandle<()>>,
) -> OrmResult<()> {
    let close_result = send_command(cmd_tx, |resp| ListenerCommand::Close { resp }).await;
    let join_result = join_worker(worker).await;
    close_result?;
    join_result
}

fn quote_ident(input: &str) -> OrmResult<String> {
    if input.trim().is_empty() {
        return Err(OrmError::validation("channel name cannot be empty"));
    }
    if input.as_bytes().contains(&0) {
        return Err(OrmError::validation("channel name cannot contain NUL byte"));
    }
    Ok(format!("\"{}\"", input.replace('"', "\"\"")))
}

#[cfg(test)]
mod tests {
    use super::{
        PgListenerState, PgListenerState::Closed, PgListenerState::Connected,
        PgListenerState::Connecting, PgListenerState::Reconnecting, PgListenerStats, next_backoff,
        quote_ident,
    };
    use std::time::Duration;

    #[test]
    fn quote_ident_wraps_and_escapes() {
        assert_eq!(quote_ident("orders").unwrap(), "\"orders\"");
        assert_eq!(quote_ident("a\"b").unwrap(), "\"a\"\"b\"");
    }

    #[test]
    fn quote_ident_rejects_empty() {
        assert!(quote_ident("").is_err());
        assert!(quote_ident("   ").is_err());
    }

    #[test]
    fn state_roundtrip_u8() {
        assert_eq!(PgListenerState::from_u8(Connecting.as_u8()), Connecting);
        assert_eq!(PgListenerState::from_u8(Connected.as_u8()), Connected);
        assert_eq!(PgListenerState::from_u8(Reconnecting.as_u8()), Reconnecting);
        assert_eq!(PgListenerState::from_u8(Closed.as_u8()), Closed);
        assert_eq!(PgListenerState::from_u8(255), Closed);
    }

    #[test]
    fn backoff_caps_at_max() {
        let min = Duration::from_millis(100);
        let max = Duration::from_millis(700);

        let b1 = next_backoff(min, max);
        let b2 = next_backoff(b1, max);
        let b3 = next_backoff(b2, max);
        let b4 = next_backoff(b3, max);

        assert_eq!(b1, Duration::from_millis(200));
        assert_eq!(b2, Duration::from_millis(400));
        assert_eq!(b3, Duration::from_millis(700));
        assert_eq!(b4, Duration::from_millis(700));
    }

    #[test]
    fn stats_default() {
        let stats = PgListenerStats::default();
        assert_eq!(stats.reconnect_count, 0);
        assert_eq!(stats.dropped_notifications, 0);
    }
}
