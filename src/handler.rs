use std::sync::Arc;

use bytes::BytesMut;
use httparse::{Request, Status};
use lru::LruCache;
use regex::Regex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::debug;

use crate::config::Config;
use crate::stats::Stats;

/// HTTP methods for detection (matching Go version)
const HTTP_METHODS: &[&str] = &[
    "GET", "POST", "HEAD", "PUT", "DELETE", "OPTIONS", "TRACE", "CONNECT", "PATCH",
];

/// HTTP handler for UA modification
pub struct HttpHandler {
    config: Arc<Config>,
    cache: Arc<Mutex<LruCache<String, String>>>,
}

impl HttpHandler {
    /// Create a new HTTP handler with LRU cache
    pub fn new(config: Arc<Config>) -> Self {
        let cache_size = config.cache_size.max(1) as usize;
        HttpHandler {
            config,
            cache: Arc::new(Mutex::new(LruCache::new(
                std::num::NonZeroUsize::new(cache_size).unwrap(),
            ))),
        }
    }

    /// Check if data starts with an HTTP method (peek first 7 bytes)
    /// Returns true if HTTP traffic detected
    fn is_http(data: &[u8]) -> bool {
        if data.len() < 7 {
            return false;
        }
        let hint = &data[..7];
        HTTP_METHODS
            .iter()
            .any(|method| hint.starts_with(method.as_bytes()))
    }

    /// Build new User-Agent string
    /// If partial replace is enabled with regex, use regex replacement
    /// Otherwise, return full replacement
    fn build_new_ua(origin_ua: &str, replacement_ua: &str, ua_regexp: &Option<Regex>, enable_partial_replace: bool) -> String {
        if enable_partial_replace {
            if let Some(re) = ua_regexp {
                return re.replace_all(origin_ua, replacement_ua).to_string();
            }
        }
        replacement_ua.to_string()
    }

    /// Check if UA should be replaced based on config mode
    /// Returns (should_replace, match_reason)
    fn should_replace_ua(&self, ua: &str) -> (bool, &'static str) {
        for whitelist_ua in &self.config.whitelist {
            if whitelist_ua == ua {
                return (false, "Hit User-Agent Whitelist");
            }
        }

        if self.config.force_replace {
            return (true, "Force Replace Mode");
        }

        if self.config.enable_regex {
            if let Some(re) = &self.config.ua_regexp {
                if re.is_match(ua) {
                    return (true, "Hit User-Agent Pattern");
                }
            }
            return (false, "Not Hit User-Agent Pattern");
        }

        for keyword in &self.config.keywords_list {
            if ua.contains(keyword) {
                return (true, "Hit User-Agent Keyword");
            }
        }

        (false, "Not Hit User-Agent Keywords")
    }

    /// Process a single HTTP request buffer
    /// Returns modified request bytes if UA was changed, or original bytes if not
    async fn process_request(&self, buf: &[u8], dest_addr: &str) -> Option<Vec<u8>> {
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = Request::new(&mut headers);

        let header_len = match req.parse(buf) {
            Ok(Status::Complete(len)) => len,
            Ok(Status::Partial) => {
                debug!("[Handler] [{}] Incomplete HTTP request", dest_addr);
                return None;
            }
            Err(e) => {
                debug!("[Handler] [{}] HTTP parse error: {}", dest_addr, e);
                return None;
            }
        };

        let ua_index = req
            .headers
            .iter()
            .position(|h| h.name.eq_ignore_ascii_case("User-Agent"));

        let ua_index = match ua_index {
            Some(idx) => idx,
            None => {
                debug!("[Handler] [{}] No User-Agent header found", dest_addr);
                return None;
            }
        };

        let original_ua = std::str::from_utf8(req.headers[ua_index].value)
            .ok()
            .map(|s| s.to_string());

        let original_ua = match original_ua {
            Some(ua) if !ua.is_empty() => ua,
            _ => {
                debug!("[Handler] [{}] Empty User-Agent header", dest_addr);
                return None;
            }
        };

        // Check cache first
        {
            let mut cache = self.cache.lock().await;
            if let Some(cached_ua) = cache.get(&original_ua) {
                if cached_ua != &original_ua {
                    debug!(
                        "[Handler] [{}] UA modified (cached): {} -> {}",
                        dest_addr, original_ua, cached_ua
                    );
                    return Some(self.rebuild_request(buf, header_len, ua_index, cached_ua));
                } else {
                    debug!(
                        "[Handler] [{}] UA not modified (cached): {}",
                        dest_addr, original_ua
                    );
                    return None;
                }
            }
        }

        let (should_replace, match_reason) = self.should_replace_ua(&original_ua);

        if !should_replace {
            debug!(
                "[Handler] [{}] {}: {}",
                dest_addr, match_reason, original_ua
            );
            let mut cache = self.cache.lock().await;
            cache.put(original_ua.clone(), original_ua);
            return None;
        }

        let new_ua = Self::build_new_ua(
            &original_ua,
            &self.config.user_agent,
            &self.config.ua_regexp,
            self.config.enable_partial_replace,
        );

        debug!(
            "[Handler] [{}] {}: {} -> {}",
            dest_addr, match_reason, original_ua, new_ua
        );

        {
            let mut cache = self.cache.lock().await;
            cache.put(original_ua, new_ua.clone());
        }

        Some(self.rebuild_request(buf, header_len, ua_index, &new_ua))
    }

    /// Rebuild HTTP request with modified User-Agent header
    fn rebuild_request(&self, original: &[u8], header_len: usize, ua_index: usize, new_ua: &str) -> Vec<u8> {
        let mut result = Vec::with_capacity(original.len() + new_ua.len());

        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = Request::new(&mut headers);
        let _ = req.parse(original);

        if let (Some(method), Some(path)) = (req.method, req.path) {
            result.extend_from_slice(method.as_bytes());
            result.push(b' ');
            result.extend_from_slice(path.as_bytes());
            if let Some(version) = req.version {
                result.extend_from_slice(b" HTTP/1.");
                result.push(if version == 1 { b'1' } else { b'0' });
            }
            result.extend_from_slice(b"\r\n");
        }

        for (i, header) in req.headers.iter().enumerate() {
            if header.name.is_empty() {
                break;
            }
            result.extend_from_slice(header.name.as_bytes());
            result.extend_from_slice(b": ");
            if i == ua_index {
                result.extend_from_slice(new_ua.as_bytes());
            } else {
                result.extend_from_slice(header.value);
            }
            result.extend_from_slice(b"\r\n");
        }

        result.extend_from_slice(b"\r\n");

        if header_len < original.len() {
            result.extend_from_slice(&original[header_len..]);
        }

        result
    }

    /// Handle connection: detect HTTP, parse, modify UA, forward
    /// This is the main entry point for connection handling
    pub async fn handle_connection(
        &self,
        client_stream: TcpStream,
        server_stream: TcpStream,
        dest_addr: String,
        stats: std::sync::Arc<Stats>,
    ) {
        let (mut client_read, mut client_write) = tokio::io::split(client_stream);
        let (mut server_read, mut server_write) = tokio::io::split(server_stream);

        let config_c2s = self.config.clone();
        let config_s2c = self.config.clone();
        let handler = Arc::new(self.clone());

        let dest_addr_clone = dest_addr.clone();
        let handler_clone = handler.clone();
        let stats_c2s = Arc::clone(&stats);
        let stats_final = Arc::clone(&stats);

        let c2s = async move {
            let mut buf = BytesMut::with_capacity(config_c2s.buffer_size as usize);
            let mut peek_buf = [0u8; 7];

            loop {
                match client_read.read(&mut peek_buf).await {
                    Ok(0) => {
                        debug!("[Handler] [{}] Connection closed by client", dest_addr_clone);
                        return;
                    }
                    Ok(n) => {
                        buf.extend_from_slice(&peek_buf[..n]);

                        if !Self::is_http(&buf) {
                            debug!(
                                "[Handler] [{}] Non-HTTP traffic detected, forwarding raw",
                                dest_addr_clone
                            );
                            if let Err(e) = server_write.write_all(&buf).await {
                                debug!("[Handler] [{}] Write error: {}", dest_addr_clone, e);
                                return;
                            }
                            let mut remaining = vec![0u8; config_c2s.buffer_size as usize];
                            loop {
                                match client_read.read(&mut remaining).await {
                                    Ok(0) => return,
                                    Ok(n) => {
                                        if let Err(e) = server_write.write_all(&remaining[..n]).await
                                        {
                                            debug!(
                                                "[Handler] [{}] Write error: {}",
                                                dest_addr_clone, e
                                            );
                                            return;
                                        }
                                    }
                                    Err(e) => {
                                        debug!(
                                            "[Handler] [{}] Read error: {}",
                                            dest_addr_clone, e
                                        );
                                        return;
                                    }
                                }
                            }
                        }

                        loop {
                            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                            let mut tmp = [0u8; 4096];
                            match client_read.read(&mut tmp).await {
                                Ok(0) => break,
                                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                                Err(e) => {
                                    debug!(
                                        "[Handler] [{}] Read error: {}",
                                        dest_addr_clone, e
                                    );
                                    return;
                                }
                            }
                        }

                        stats_c2s.inc_http_requests();

                        if let Some(modified) =
                            handler_clone.process_request(&buf, &dest_addr_clone).await
                        {
                            stats_c2s.inc_modified_requests();
                            if let Err(e) = server_write.write_all(&modified).await {
                                debug!("[Handler] [{}] Write error: {}", dest_addr_clone, e);
                                return;
                            }
                        } else {
                            stats_c2s.inc_cache_hit_no_modify();
                            if let Err(e) = server_write.write_all(&buf).await {
                                debug!("[Handler] [{}] Write error: {}", dest_addr_clone, e);
                                return;
                            }
                        }

                        buf.clear();
                    }
                    Err(e) => {
                        debug!("[Handler] [{}] Read error: {}", dest_addr_clone, e);
                        return;
                    }
                }
            }
        };

        let s2c = async move {
            let mut buf = vec![0u8; config_s2c.buffer_size as usize];
            loop {
                match server_read.read(&mut buf).await {
                    Ok(0) => return,
                    Ok(n) => {
                        if let Err(e) = client_write.write_all(&buf[..n]).await {
                            debug!("[Handler] [{}] Write error: {}", dest_addr, e);
                            return;
                        }
                    }
                    Err(e) => {
                        debug!("[Handler] [{}] Read error: {}", dest_addr, e);
                        return;
                    }
                }
            }
        };

        tokio::join!(c2s, s2c);
        stats_final.sub_active_connections(1);
    }
}

impl Clone for HttpHandler {
    fn clone(&self) -> Self {
        HttpHandler {
            config: self.config.clone(),
            cache: self.cache.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_http() {
        assert!(HttpHandler::is_http(b"GET / HTTP/1.1"));
        assert!(HttpHandler::is_http(b"POST /api HTTP/1.1"));
        assert!(HttpHandler::is_http(b"HEAD / HTTP/1.1"));
        assert!(HttpHandler::is_http(b"PUT / HTTP/1.1"));
        assert!(HttpHandler::is_http(b"DELETE / HTTP/1.1"));
        assert!(HttpHandler::is_http(b"OPTIONS / HTTP/1.1"));
        assert!(HttpHandler::is_http(b"PATCH / HTTP/1.1"));
        assert!(HttpHandler::is_http(b"TRACE / HTTP/1.1"));
        assert!(HttpHandler::is_http(b"CONNECT example.com:443 HTTP/1.1"));

        assert!(!HttpHandler::is_http(b"HTTP/1.1 200 OK"));
        assert!(!HttpHandler::is_http(b"\x16\x03\x01"));
        assert!(!HttpHandler::is_http(b"GET"));
        assert!(!HttpHandler::is_http(b""));
    }

    #[test]
    fn test_build_new_ua_full_replace() {
        let result = HttpHandler::build_new_ua(
            "Mozilla/5.0 (Windows NT 10.0)",
            "CustomUA/1.0",
            &None,
            false,
        );
        assert_eq!(result, "CustomUA/1.0");
    }

    #[test]
    fn test_build_new_ua_partial_replace() {
        let re = Regex::new(r"(?i)(iPhone|iPad|Android)").unwrap();
        let result = HttpHandler::build_new_ua(
            "Mozilla/5.0 (iPhone; CPU iPhone OS)",
            "CustomDevice",
            &Some(re),
            true,
        );
        assert_eq!(result, "Mozilla/5.0 (CustomDevice; CPU CustomDevice OS)");
    }

    #[test]
    fn test_should_replace_ua_force() {
        let config = Config {
            force_replace: true,
            ..Default::default()
        };
        let handler = HttpHandler::new(Arc::new(config));
        let (should, reason) = handler.should_replace_ua("Mozilla/5.0");
        assert!(should);
        assert_eq!(reason, "Force Replace Mode");
    }

    #[test]
    fn test_should_replace_ua_whitelist() {
        let config = Config {
            whitelist: vec!["Mozilla/5.0".to_string()],
            ..Default::default()
        };
        let handler = HttpHandler::new(Arc::new(config));
        let (should, reason) = handler.should_replace_ua("Mozilla/5.0");
        assert!(!should);
        assert_eq!(reason, "Hit User-Agent Whitelist");
    }

    #[test]
    fn test_should_replace_ua_regex() {
        let re = Regex::new(r"(?i)(iPhone|iPad|Android)").unwrap();
        let config = Config {
            enable_regex: true,
            ua_regexp: Some(re),
            ..Default::default()
        };
        let handler = HttpHandler::new(Arc::new(config));

        let (should, _) = handler.should_replace_ua("Mozilla/5.0 (iPhone)");
        assert!(should);

        let (should, _) = handler.should_replace_ua("Bot/1.0");
        assert!(!should);
    }

    #[test]
    fn test_should_replace_ua_keywords() {
        let config = Config {
            keywords_list: vec!["iPhone".to_string(), "Android".to_string()],
            ..Default::default()
        };
        let handler = HttpHandler::new(Arc::new(config));

        let (should, _) = handler.should_replace_ua("Mozilla/5.0 (iPhone)");
        assert!(should);

        let (should, _) = handler.should_replace_ua("Bot/1.0");
        assert!(!should);
    }
}
