use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use regex::Regex;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::{interval, sleep, timeout};
use tracing::{debug, info, warn};

use crate::config::Config;

/// Item to be added to firewall set
#[derive(Debug, Clone)]
struct FirewallAddItem {
    ip: String,
    port: u16,
    set_name: String,
    fw_type: String,
    timeout: u32,
}

/// Port profile for tracking non-HTTP traffic per IP
#[derive(Debug)]
struct PortProfile {
    /// Non-HTTP event score
    non_http_score: u32,
    /// HTTP lock expiration time (cooldown period)
    http_lock_expires: Option<Instant>,
    /// Last event time
    last_event: Instant,
    /// Decision timer handle (None if not pending)
    decision_pending: bool,
}

/// Report event for port profiling
#[derive(Debug, Clone)]
struct ReportEvent {
    ip: String,
    port: u16,
}

/// Firewall manager for ipset/nftables operations and traffic offload decisions
pub struct FirewallManager {
    config: Arc<Config>,
    queue_tx: mpsc::Sender<FirewallAddItem>,
    queue_rx: Mutex<Option<mpsc::Receiver<FirewallAddItem>>>,
    non_http_tx: mpsc::Sender<ReportEvent>,
    non_http_rx: Mutex<Option<mpsc::Receiver<ReportEvent>>>,
    http_tx: mpsc::Sender<ReportEvent>,
    http_rx: Mutex<Option<mpsc::Receiver<ReportEvent>>>,
    port_profiles: Arc<Mutex<HashMap<String, PortProfile>>>,
    max_batch_size: usize,
    max_batch_wait: Duration,
    profile_cleanup_interval: Duration,
}

impl FirewallManager {
    /// Create a new FirewallManager
    pub fn new(config: Arc<Config>, queue_size: usize) -> Self {
        let (queue_tx, queue_rx) = mpsc::channel(queue_size);
        let (non_http_tx, non_http_rx) = mpsc::channel(queue_size);
        let (http_tx, http_rx) = mpsc::channel(queue_size);

        FirewallManager {
            config,
            queue_tx,
            queue_rx: Mutex::new(Some(queue_rx)),
            non_http_tx,
            non_http_rx: Mutex::new(Some(non_http_rx)),
            http_tx,
            http_rx: Mutex::new(Some(http_rx)),
            port_profiles: Arc::new(Mutex::new(HashMap::new())),
            max_batch_size: 200,
            max_batch_wait: Duration::from_millis(100),
            profile_cleanup_interval: Duration::from_secs(600),
        }
    }

    /// Report HTTP event (one-vote veto)
    pub async fn report_http_event(&self, ip: &str, port: u16) {
        let event = ReportEvent {
            ip: ip.to_string(),
            port,
        };
        if let Err(_) = self.http_tx.try_send(event) {
            warn!("[Manager] HTTP event channel full, dropping event for {}:{}", ip, port);
        }
    }

    /// Report non-HTTP event (accumulate score)
    pub async fn report_non_http_event(&self, ip: &str, port: u16) {
        let event = ReportEvent {
            ip: ip.to_string(),
            port,
        };
        if let Err(_) = self.non_http_tx.try_send(event) {
            warn!("[Manager] Non-HTTP event channel full, dropping event for {}:{}", ip, port);
        }
    }

    /// Add IP to firewall set
    pub async fn add(&self, ip: &str, port: u16, set_name: &str, fw_type: &str, timeout: u32) {
        if ip.is_empty() || set_name.is_empty() {
            return;
        }

        // Validate IP address
        if std::net::IpAddr::from_str(ip).is_err() {
            warn!("[Manager] Invalid IP address: {}", ip);
            return;
        }

        // Validate set name (alphanumeric + underscore only)
        let set_name_regex = Regex::new(r"^[a-zA-Z0-9_]+$").unwrap();
        if !set_name_regex.is_match(set_name) {
            warn!("[Manager] Invalid set name: {}", set_name);
            return;
        }

        let item = FirewallAddItem {
            ip: ip.to_string(),
            port,
            set_name: set_name.to_string(),
            fw_type: fw_type.to_string(),
            timeout,
        };

        if let Err(_) = tokio::time::timeout(Duration::from_millis(50), self.queue_tx.send(item)).await {
            warn!("[Manager] Firewall add queue is full. Dropping item for {}", ip);
        }
    }

    /// Start the manager worker
    pub async fn start(&self) {
        let queue_rx = self.queue_rx.lock().await.take();
        let non_http_rx = self.non_http_rx.lock().await.take();
        let http_rx = self.http_rx.lock().await.take();

        let (queue_rx, non_http_rx, http_rx) = match (queue_rx, non_http_rx, http_rx) {
            (Some(q), Some(nh), Some(h)) => (q, nh, h),
            _ => {
                warn!("[Manager] Manager already started or receivers taken");
                return;
            }
        };

        info!("[Manager] FirewallManager worker started");

        let manager = Arc::new(self.clone_without_channels());
        let manager_clone = manager.clone();

        // Spawn the main worker task
        tokio::spawn(async move {
            manager_clone.worker(queue_rx, non_http_rx, http_rx).await;
        });
    }

    /// Clone manager without channels (for worker task)
    fn clone_without_channels(&self) -> Self {
        FirewallManager {
            config: self.config.clone(),
            queue_tx: self.queue_tx.clone(),
            queue_rx: Mutex::new(None),
            non_http_tx: self.non_http_tx.clone(),
            non_http_rx: Mutex::new(None),
            http_tx: self.http_tx.clone(),
            http_rx: Mutex::new(None),
            port_profiles: Arc::new(Mutex::new(HashMap::new())),
            max_batch_size: self.max_batch_size,
            max_batch_wait: self.max_batch_wait,
            profile_cleanup_interval: self.profile_cleanup_interval,
        }
    }

    /// Main worker loop
    async fn worker(
        &self,
        mut queue_rx: mpsc::Receiver<FirewallAddItem>,
        mut non_http_rx: mpsc::Receiver<ReportEvent>,
        mut http_rx: mpsc::Receiver<ReportEvent>,
    ) {
        let mut batches: HashMap<String, HashMap<String, FirewallAddItem>> = HashMap::new();
        let mut batch_timer = tokio::time::interval(self.max_batch_wait);
        batch_timer.tick().await; // Skip first immediate tick

        let mut cleanup_timer = interval(self.profile_cleanup_interval);

        loop {
            tokio::select! {
                // Process firewall add queue
                item = queue_rx.recv() => {
                    match item {
                        Some(item) => {
                            let key = format!("{}:{}", item.fw_type, item.set_name);
                            let dedup_key = format!("{}:{}", item.ip, item.port);

                            batches
                                .entry(key)
                                .or_insert_with(HashMap::new)
                                .insert(dedup_key, item);

                            // Check if batch is full
                            if let Some(batch) = batches.values().next() {
                                if batch.len() >= self.max_batch_size {
                                    self.execute_batches(&batches).await;
                                    batches.clear();
                                }
                            }
                        }
                        None => {
                            // Channel closed, execute remaining batches and exit
                            self.execute_batches(&batches).await;
                            break;
                        }
                    }
                }

                // Process batch timer
                _ = batch_timer.tick() => {
                    if !batches.is_empty() {
                        self.execute_batches(&batches).await;
                        batches.clear();
                    }
                }

                // Process HTTP events
                event = http_rx.recv() => {
                    if let Some(event) = event {
                        self.handle_http_event(&event.ip, event.port).await;
                    }
                }

                // Process non-HTTP events
                event = non_http_rx.recv() => {
                    if let Some(event) = event {
                        self.handle_non_http_event(&event.ip, event.port).await;
                    }
                }

                // Cleanup stale profiles
                _ = cleanup_timer.tick() => {
                    self.cleanup_profiles().await;
                }
            }
        }

        info!("[Manager] FirewallManager worker stopped");
    }

    /// Execute batch firewall operations
    async fn execute_batches(&self, batches: &HashMap<String, HashMap<String, FirewallAddItem>>) {
        if batches.is_empty() {
            return;
        }

        debug!("[Manager] Executing {} batches...", batches.len());

        for (_key, items_map) in batches {
            if items_map.is_empty() {
                continue;
            }

            let items: Vec<&FirewallAddItem> = items_map.values().collect();
            let first_item = items[0];
            let fw_type = &first_item.fw_type;
            let set_name = &first_item.set_name;
            let item_count = items.len();

            let result = if fw_type == "nft" {
                self.execute_nft_batch(set_name, &items).await
            } else {
                self.execute_ipset_batch(set_name, &items).await
            };

            match result {
                Ok(_) => {
                    debug!(
                        "[Manager] Successfully added {} unique IPs to firewall set {} ({})",
                        item_count, set_name, fw_type
                    );
                }
                Err(e) => {
                    warn!(
                        "[Manager] Failed to execute batch for set {} ({}): {}",
                        set_name, fw_type, e
                    );
                }
            }
        }
    }

    /// Execute nftables batch add
    async fn execute_nft_batch(&self, set_name: &str, items: &[&FirewallAddItem]) -> Result<(), String> {
        let mut elements = Vec::new();
        for item in items {
            let mut element = format!("{} . {}", item.ip, item.port);
            if item.timeout > 0 {
                element.push_str(&format!(" timeout {}s", item.timeout));
            }
            elements.push(element);
        }

        let elements_str = elements.join(", ");
        let args = vec![
            "add".to_string(),
            "element".to_string(),
            "inet".to_string(),
            "fw4".to_string(),
            set_name.to_string(),
            "{".to_string(),
            elements_str,
            "}".to_string(),
        ];

        debug!("[Manager] Executing nft {:?}", args);

        let output = timeout(Duration::from_secs(10), async {
            Command::new("nft")
                .args(&args)
                .output()
        })
        .await
        .map_err(|_| "nft command timed out".to_string())?
        .map_err(|e| format!("Failed to execute nft: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("nft failed: {}", stderr));
        }

        Ok(())
    }

    /// Execute ipset batch add using restore
    async fn execute_ipset_batch(&self, set_name: &str, items: &[&FirewallAddItem]) -> Result<(), String> {
        let mut stdin = String::new();
        for item in items {
            if item.timeout > 0 {
                stdin.push_str(&format!(
                    "add {} {},{} timeout {} -exist\n",
                    set_name, item.ip, item.port, item.timeout
                ));
            } else {
                stdin.push_str(&format!(
                    "add {} {},{} -exist\n",
                    set_name, item.ip, item.port
                ));
            }
        }

        debug!("[Manager] Executing ipset restore with {} entries", items.len());

        let output = timeout(Duration::from_secs(10), async {
            Command::new("ipset")
                .arg("restore")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    if let Some(child_stdin) = child.stdin.as_mut() {
                        child_stdin.write_all(stdin.as_bytes())?;
                    }
                    child.wait_with_output()
                })
        })
        .await
        .map_err(|_| "ipset restore timed out".to_string())?
        .map_err(|e| format!("Failed to execute ipset restore: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("ipset restore failed: {}", stderr));
        }

        Ok(())
    }

    /// Handle HTTP event (one-vote veto)
    async fn handle_http_event(&self, ip: &str, port: u16) {
        let key = format!("{}:{}", ip, port);
        let mut profiles = self.port_profiles.lock().await;

        let profile = profiles.entry(key.clone()).or_insert(PortProfile {
            non_http_score: 0,
            http_lock_expires: None,
            last_event: Instant::now(),
            decision_pending: false,
        });

        // If in cooldown period, return
        if let Some(expires) = profile.http_lock_expires {
            if Instant::now() < expires {
                return;
            }
        }

        debug!("[Manager] HTTP event for {}, resetting score and setting cooldown.", key);

        // Reset non-HTTP score and set new cooldown
        profile.non_http_score = 0;
        profile.http_lock_expires = Some(Instant::now() + self.config.firewall_http_cooldown_period);
        profile.last_event = Instant::now();

        // Cancel pending decision if exists
        if profile.decision_pending {
            profile.decision_pending = false;
            info!("[Manager] Cancelled firewall add for {} due to new HTTP activity.", key);
        }
    }

    /// Handle non-HTTP event (accumulate score)
    async fn handle_non_http_event(&self, ip: &str, port: u16) {
        let key = format!("{}:{}", ip, port);
        let mut profiles = self.port_profiles.lock().await;

        let profile = profiles.entry(key.clone()).or_insert(PortProfile {
            non_http_score: 0,
            http_lock_expires: None,
            last_event: Instant::now(),
            decision_pending: false,
        });

        // If in HTTP cooldown, ignore
        if let Some(expires) = profile.http_lock_expires {
            if Instant::now() < expires {
                debug!("[Manager] Ignored non-HTTP event for {} during HTTP cooldown.", key);
                return;
            }
        }

        // Accumulate non-HTTP score
        profile.non_http_score += 1;
        profile.last_event = Instant::now();
        debug!("[Manager] Non-HTTP event for {}, score is now {}.", key, profile.non_http_score);

        // Check if threshold reached
        if profile.non_http_score >= self.config.firewall_nonhttp_threshold as u32 {
            if !profile.decision_pending {
                info!(
                    "[Manager] Threshold reached for {}. Starting decision timer ({:?}).",
                    key, self.config.firewall_decision_delay
                );
                profile.decision_pending = true;

                // Spawn decision timer
                let ip_owned = ip.to_string();
                let port_owned = port;
                let manager = self.clone_for_timer();
                tokio::spawn(async move {
                    sleep(manager.config.firewall_decision_delay).await;
                    manager.finalize_decision(&ip_owned, port_owned).await;
                });
            }
        }
    }

    /// Clone manager for timer task (minimal clone)
    fn clone_for_timer(&self) -> Self {
        FirewallManager {
            config: self.config.clone(),
            queue_tx: self.queue_tx.clone(),
            queue_rx: Mutex::new(None),
            non_http_tx: self.non_http_tx.clone(),
            non_http_rx: Mutex::new(None),
            http_tx: self.http_tx.clone(),
            http_rx: Mutex::new(None),
            port_profiles: self.port_profiles.clone(),
            max_batch_size: self.max_batch_size,
            max_batch_wait: self.max_batch_wait,
            profile_cleanup_interval: self.profile_cleanup_interval,
        }
    }

    /// Finalize decision after delay
    async fn finalize_decision(&self, ip: &str, port: u16) {
        let key = format!("{}:{}", ip, port);
        let mut profiles = self.port_profiles.lock().await;

        if let Some(profile) = profiles.get_mut(&key) {
            // Re-check conditions
            if profile.non_http_score < self.config.firewall_nonhttp_threshold as u32 {
                info!("[Manager] Final decision for {} aborted (conditions no longer met).", key);
                profile.decision_pending = false;
                return;
            }

            // Check if in HTTP cooldown
            if let Some(expires) = profile.http_lock_expires {
                if Instant::now() < expires {
                    info!("[Manager] Final decision for {} aborted (in HTTP cooldown).", key);
                    profile.decision_pending = false;
                    return;
                }
            }

            info!("[Manager] Decision final for {}. Adding to firewall.", key);

            self.add(
                ip,
                port,
                &self.config.firewall_ipset_name,
                &self.config.firewall_type,
                self.config.firewall_timeout as u32,
            )
            .await;

            profiles.remove(&key);
        }
    }

    /// Cleanup stale profiles
    async fn cleanup_profiles(&self) {
        let mut profiles = self.port_profiles.lock().await;
        let now = Instant::now();
        let mut cleaned_count = 0;

        profiles.retain(|_key, profile| {
            // Keep if decision is pending
            if profile.decision_pending {
                return true;
            }

            // Keep if in HTTP cooldown
            if let Some(expires) = profile.http_lock_expires {
                if now < expires {
                    return true;
                }
            }

            if now.duration_since(profile.last_event) > self.profile_cleanup_interval {
                cleaned_count += 1;
                false
            } else {
                true
            }
        });

        if cleaned_count > 0 {
            debug!("[Manager] Cleaned up {} stale port profiles.", cleaned_count);
        }
    }

    /// Create ipset set
    pub async fn create_ipset_set(&self, set_name: &str) -> Result<(), String> {
        let output = Command::new("ipset")
            .args(["create", set_name, "hash:ip,port", "timeout", &self.config.firewall_timeout.to_string(), "-exist"])
            .output()
            .map_err(|e| format!("Failed to create ipset: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("ipset create failed: {}", stderr));
        }

        info!("[Manager] Created ipset set: {}", set_name);
        Ok(())
    }

    /// Destroy ipset set
    pub async fn destroy_ipset_set(&self, set_name: &str) -> Result<(), String> {
        let output = Command::new("ipset")
            .args(["destroy", set_name, "-exist"])
            .output()
            .map_err(|e| format!("Failed to destroy ipset: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("ipset destroy failed: {}", stderr));
        }

        info!("[Manager] Destroyed ipset set: {}", set_name);
        Ok(())
    }

    /// Create nftables set
    pub async fn create_nft_set(&self, set_name: &str) -> Result<(), String> {
        let output = Command::new("nft")
            .args([
                "add",
                "set",
                "inet",
                "fw4",
                set_name,
                "{",
                "type",
                "ipv4_addr . inet_service",
                ";",
                "timeout",
                &format!("{}s", self.config.firewall_timeout),
                ";",
                "}",
            ])
            .output()
            .map_err(|e| format!("Failed to create nft set: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("nft add set failed: {}", stderr));
        }

        info!("[Manager] Created nftables set: {}", set_name);
        Ok(())
    }

    /// Destroy nftables set
    pub async fn destroy_nft_set(&self, set_name: &str) -> Result<(), String> {
        let output = Command::new("nft")
            .args(["delete", "set", "inet", "fw4", set_name])
            .output()
            .map_err(|e| format!("Failed to delete nft set: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("nft delete set failed: {}", stderr));
        }

        info!("[Manager] Destroyed nftables set: {}", set_name);
        Ok(())
    }
}

use std::str::FromStr;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_port_profile_scoring() {
        let config = Arc::new(Config::default());
        let manager = FirewallManager::new(config, 100);

        manager.handle_non_http_event("192.168.1.1", 8080).await;
        manager.handle_non_http_event("192.168.1.1", 8080).await;
        manager.handle_non_http_event("192.168.1.1", 8080).await;

        let profiles = manager.port_profiles.lock().await;
        let key = "192.168.1.1:8080";
        assert!(profiles.contains_key(key));
        let profile = profiles.get(key).unwrap();
        assert_eq!(profile.non_http_score, 3);
    }

    #[tokio::test]
    async fn test_http_event_resets_score() {
        let config = Arc::new(Config::default());
        let manager = FirewallManager::new(config, 100);

        manager.handle_non_http_event("192.168.1.1", 8080).await;
        manager.handle_non_http_event("192.168.1.1", 8080).await;

        manager.handle_http_event("192.168.1.1", 8080).await;

        let profiles = manager.port_profiles.lock().await;
        let key = "192.168.1.1:8080";
        let profile = profiles.get(key).unwrap();
        assert_eq!(profile.non_http_score, 0);
        assert!(profile.http_lock_expires.is_some());
    }

    #[tokio::test]
    async fn test_cooldown_prevents_scoring() {
        let config = Arc::new(Config {
            firewall_http_cooldown_period: Duration::from_secs(3600),
            ..Default::default()
        });
        let manager = FirewallManager::new(config, 100);

        manager.handle_http_event("192.168.1.1", 8080).await;

        manager.handle_non_http_event("192.168.1.1", 8080).await;
        manager.handle_non_http_event("192.168.1.1", 8080).await;

        let profiles = manager.port_profiles.lock().await;
        let key = "192.168.1.1:8080";
        let profile = profiles.get(key).unwrap();
        assert_eq!(profile.non_http_score, 0);
    }
}
