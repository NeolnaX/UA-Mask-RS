mod config;
mod handler;
mod server;
mod stats;
mod tproxy;

use std::sync::Arc;

use config::Config;
use server::Server;
use stats::Stats;
use tokio::time::Duration;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::new()?;

    let level = match cfg.log_level.as_str() {
        "debug" => tracing::Level::DEBUG,
        "trace" => tracing::Level::TRACE,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    };

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(level)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    if cfg.show_version {
        println!("UA-Mask v0.5.0");
        return Ok(());
    }

    cfg.log_config("0.5.0");

    info!("UA-Mask v0.5.0 starting on port {}", cfg.port);

    let stats = Arc::new(Stats::new());
    stats.clone().start_writer("/tmp/UAmask.stats".to_string(), Duration::from_secs(10));

    let config = Arc::new(cfg);
    let server = Server::new(config, stats);
    server.run().await?;

    Ok(())
}
