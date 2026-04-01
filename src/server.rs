use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::handler::HttpHandler;
use crate::stats::Stats;
use crate::tproxy::get_original_dst;

pub struct Server {
    config: Arc<Config>,
    stats: Arc<Stats>,
    handler: HttpHandler,
}

impl Server {
    pub fn new(config: Arc<Config>, stats: Arc<Stats>) -> Self {
        let handler = HttpHandler::new(config.clone());
        Server {
            config,
            stats,
            handler,
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let addr = format!("0.0.0.0:{}", self.config.port);
        let listener = TcpListener::bind(&addr).await?;
        info!("REDIRECT proxy server listening on {}", addr);

        if self.config.pool_size > 0 {
            self.run_worker_pool(listener).await
        } else {
            self.run_default(listener).await
        }
    }

    async fn run_worker_pool(
        &self,
        listener: TcpListener,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let pool_size = self.config.pool_size as usize;
        info!("Starting in Worker Pool Mode (size: {})", pool_size);

        let semaphore = Arc::new(Semaphore::new(pool_size));
        let config = Arc::clone(&self.config);
        let stats = Arc::clone(&self.stats);
        let handler = self.handler.clone();

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let permit = semaphore.clone().acquire_owned().await.unwrap();
                    let config = Arc::clone(&config);
                    let stats = Arc::clone(&stats);
                    let handler = handler.clone();

                    tokio::spawn(async move {
                        Self::handle_connection(stream, &config, stats, &handler).await;
                        drop(permit);
                    });
                }
                Err(e) => {
                    warn!("Accept error: {}; retrying...", e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
    }

    async fn run_default(
        &self,
        listener: TcpListener,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting in Default Mode (one task per connection)");

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let config = Arc::clone(&self.config);
                    let stats = Arc::clone(&self.stats);
                    let handler = self.handler.clone();
                    tokio::spawn(async move {
                        Self::handle_connection(stream, &config, stats, &handler).await;
                    });
                }
                Err(e) => {
                    warn!("Accept error: {}; retrying...", e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
    }

    async fn handle_connection(
        client_stream: TcpStream,
        _config: &Config,
        stats: Arc<Stats>,
        handler: &HttpHandler,
    ) {
        stats.add_active_connections(1);

        let original_dst = match get_original_dst(&client_stream).await {
            Ok(addr) => addr,
            Err(e) => {
                debug!("[server] Failed to get original destination: {}", e);
                stats.sub_active_connections(1);
                return;
            }
        };

        let client_addr: Option<std::net::SocketAddr> = client_stream.peer_addr().ok();
        debug!(
            "[server] Connection: {:?} -> {:?} (original: {})",
            client_addr,
            client_stream.local_addr(),
            original_dst
        );

        if let Err(e) = client_stream.set_nodelay(true) {
            debug!("[server] Failed to set nodelay: {}", e);
        }

        let server_stream = match TcpStream::connect(original_dst).await {
            Ok(s) => s,
            Err(e) => {
                debug!("[server] Failed to connect to {}: {}", original_dst, e);
                stats.sub_active_connections(1);
                return;
            }
        };

        if let Err(e) = server_stream.set_nodelay(true) {
            debug!("[server] Failed to set nodelay on upstream: {}", e);
        }

        let dest_addr = original_dst.to_string();

        handler
            .handle_connection(client_stream, server_stream, dest_addr, stats)
            .await;
    }
}