use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::fs;
use tokio::time::{self, Duration};
use tracing::warn;

/// Statistics counters for UA-Mask
pub struct Stats {
    /// Current active connections
    pub active_connections: AtomicU64,
    /// Total HTTP requests processed
    pub http_requests: AtomicU64,
    /// Total successful UA modifications
    pub modified_requests: AtomicU64,
    /// Cache hits (modify)
    pub cache_hits: AtomicU64,
    /// Cache hits (pass-through, no modify)
    pub cache_hit_no_modify: AtomicU64,
}

impl Stats {
    /// Create a new Stats instance with all counters at zero
    pub fn new() -> Self {
        Stats {
            active_connections: AtomicU64::new(0),
            http_requests: AtomicU64::new(0),
            modified_requests: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_hit_no_modify: AtomicU64::new(0),
        }
    }

    /// Add value to active connections counter
    pub fn add_active_connections(&self, val: u64) {
        self.active_connections.fetch_add(val, Ordering::Relaxed);
    }

    /// Subtract value from active connections counter
    pub fn sub_active_connections(&self, val: u64) {
        self.active_connections.fetch_sub(val, Ordering::Relaxed);
    }

    /// Increment HTTP requests counter
    pub fn inc_http_requests(&self) {
        self.http_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment modified requests counter
    pub fn inc_modified_requests(&self) {
        self.modified_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment cache hits (modify) counter
    pub fn inc_cache_hits(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment cache hits (no modify) counter
    pub fn inc_cache_hit_no_modify(&self) {
        self.cache_hit_no_modify.fetch_add(1, Ordering::Relaxed);
    }

    /// Start periodic stats writer
    /// Writes stats to file every `interval` duration
    pub fn start_writer(self: Arc<Self>, file_path: String, interval: Duration) {
        tokio::spawn(async move {
            let mut ticker = time::interval(interval);
            let mut last_http_requests: u64 = 0;
            let mut last_check_time = Instant::now();

            loop {
                ticker.tick().await;

                let active_conn = self.active_connections.load(Ordering::Relaxed);
                let http_requests = self.http_requests.load(Ordering::Relaxed);
                let modified = self.modified_requests.load(Ordering::Relaxed);
                let cache_hit_modify = self.cache_hits.load(Ordering::Relaxed);
                let cache_hit_pass = self.cache_hit_no_modify.load(Ordering::Relaxed);

                let now = Instant::now();
                let interval_seconds = now.duration_since(last_check_time).as_secs_f64();
                let rps = if interval_seconds > 0.0 {
                    let requests_since_last = http_requests.saturating_sub(last_http_requests);
                    requests_since_last as f64 / interval_seconds
                } else {
                    0.0
                };

                last_http_requests = http_requests;
                last_check_time = now;

                let total_cache_hits = cache_hit_modify + cache_hit_pass;
                let rule_processing = http_requests.saturating_sub(total_cache_hits);
                let direct_pass = http_requests.saturating_sub(modified);

                let total_cache_ratio = if http_requests > 0 {
                    (total_cache_hits as f64 * 100.0) / http_requests as f64
                } else {
                    0.0
                };

                let content = format!(
                    "current_connections:{}\n\
                     total_requests:{}\n\
                     rps:{:.2}\n\
                     successful_modifications:{}\n\
                     direct_passthrough:{}\n\
                     rule_processing:{}\n\
                     cache_hit_modify:{}\n\
                     cache_hit_pass:{}\n\
                     total_cache_ratio:{:.2}\n",
                    active_conn,
                    http_requests,
                    rps,
                    modified,
                    direct_pass,
                    rule_processing,
                    cache_hit_modify,
                    cache_hit_pass,
                    total_cache_ratio,
                );

                if let Err(e) = fs::write(&file_path, content).await {
                    warn!("Failed to write stats file: {}", e);
                }
            }
        });
    }
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_new() {
        let stats = Stats::new();
        assert_eq!(stats.active_connections.load(Ordering::Relaxed), 0);
        assert_eq!(stats.http_requests.load(Ordering::Relaxed), 0);
        assert_eq!(stats.modified_requests.load(Ordering::Relaxed), 0);
        assert_eq!(stats.cache_hits.load(Ordering::Relaxed), 0);
        assert_eq!(stats.cache_hit_no_modify.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_stats_increment() {
        let stats = Stats::new();

        stats.add_active_connections(5);
        assert_eq!(stats.active_connections.load(Ordering::Relaxed), 5);

        stats.sub_active_connections(2);
        assert_eq!(stats.active_connections.load(Ordering::Relaxed), 3);

        stats.inc_http_requests();
        stats.inc_http_requests();
        assert_eq!(stats.http_requests.load(Ordering::Relaxed), 2);

        stats.inc_modified_requests();
        assert_eq!(stats.modified_requests.load(Ordering::Relaxed), 1);

        stats.inc_cache_hits();
        stats.inc_cache_hits();
        stats.inc_cache_hits();
        assert_eq!(stats.cache_hits.load(Ordering::Relaxed), 3);

        stats.inc_cache_hit_no_modify();
        assert_eq!(stats.cache_hit_no_modify.load(Ordering::Relaxed), 1);
    }
}
