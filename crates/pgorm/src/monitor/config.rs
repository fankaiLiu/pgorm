use std::time::Duration;

/// Configuration for query monitoring and timeouts.
///
/// By default, monitoring is disabled and must be explicitly enabled.
#[derive(Debug, Clone, Default)]
pub struct MonitorConfig {
    /// Query timeout duration. `None` means no timeout (default).
    pub query_timeout: Option<Duration>,
    /// Slow query threshold for alerting.
    pub slow_query_threshold: Option<Duration>,
    /// Whether monitoring is enabled.
    pub monitoring_enabled: bool,
}

impl MonitorConfig {
    /// Create a new configuration with defaults (monitoring disabled, no timeout).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the query timeout duration.
    ///
    /// Queries exceeding this duration will be cancelled and return a timeout error.
    /// Default is `None` (no timeout).
    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.query_timeout = Some(timeout);
        self
    }

    /// Set the slow query threshold.
    ///
    /// Queries exceeding this duration will trigger `on_slow_query` callbacks.
    pub fn with_slow_query_threshold(mut self, threshold: Duration) -> Self {
        self.slow_query_threshold = Some(threshold);
        self
    }

    /// Enable monitoring.
    ///
    /// Monitoring must be explicitly enabled for monitors to receive events.
    pub fn enable_monitoring(mut self) -> Self {
        self.monitoring_enabled = true;
        self
    }

    /// Disable monitoring.
    pub fn disable_monitoring(mut self) -> Self {
        self.monitoring_enabled = false;
        self
    }
}
