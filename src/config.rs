use clap::Parser;
use regex::Regex;

/// UA-Mask CLI arguments
#[derive(Parser, Debug, Clone)]
#[command(name = "ua-mask")]
#[command(version = "0.5.0")]
#[command(about = "High-performance User-Agent modification proxy for OpenWrt")]
pub struct Args {
    /// User-Agent string to replace with
    #[arg(short = 'u', long, default_value = "FFF")]
    pub user_agent: String,

    /// TPROXY listen port
    #[arg(long, default_value_t = 12032)]
    pub port: u16,

    /// Log level (debug, info, warn, error)
    #[arg(long, default_value = "info")]
    pub loglevel: String,

    /// Show version
    #[arg(short = 'v', long, default_value_t = false)]
    pub version: bool,

    /// Log file path
    #[arg(long, default_value = "")]
    pub log: String,

    /// Comma-separated User-Agent whitelist
    #[arg(short = 'w', long, default_value = "")]
    pub whitelist: String,

    /// Force replace User-Agent
    #[arg(long, default_value_t = false)]
    pub force: bool,

    /// Enable Regex matching mode
    #[arg(long, default_value_t = false)]
    pub enable_regex: bool,

    /// Comma-separated User-Agent keywords
    #[arg(long, default_value = "iPhone,iPad,Android,Macintosh,Windows")]
    pub keywords: String,

    /// UA-Pattern (Regex)
    #[arg(
        short = 'r',
        long,
        default_value = "(iPhone|iPad|Android|Macintosh|Windows|Linux|Apple|Mac OS X|Mobile)"
    )]
    pub pattern: String,

    /// Enable Regex Partial Replace
    #[arg(short = 's', long, default_value_t = false)]
    pub partial_replace: bool,

    /// LRU cache size
    #[arg(long, default_value_t = 1000)]
    pub cache_size: i32,

    /// I/O buffer size (bytes)
    #[arg(long, default_value_t = 8192)]
    pub buffer_size: i32,

    /// Worker pool size
    #[arg(short = 'p', long, default_value_t = 0)]
    pub pool_size: i32,
}

/// Application configuration
#[derive(Debug, Clone)]
pub struct Config {
    pub user_agent: String,
    pub port: u16,
    pub log_level: String,
    pub show_version: bool,
    pub log_file: String,
    pub whitelist: Vec<String>,
    pub force_replace: bool,
    pub enable_regex: bool,
    pub enable_partial_replace: bool,
    pub keywords_list: Vec<String>,
    pub ua_pattern: String,
    pub ua_regexp: Option<Regex>,
    pub cache_size: i32,
    pub buffer_size: i32,
    pub pool_size: i32,
}

impl Config {
    pub fn new() -> Result<Self, String> {
        let args = Args::parse();
        Self::from_args(args)
    }

    pub fn from_args(args: Args) -> Result<Self, String> {
        let whitelist = parse_comma_list(&args.whitelist);

        let mut cfg = Config {
            user_agent: args.user_agent,
            port: args.port,
            log_level: args.loglevel,
            show_version: args.version,
            log_file: args.log,
            whitelist,
            force_replace: args.force,
            enable_regex: args.enable_regex,
            enable_partial_replace: args.partial_replace,
            keywords_list: Vec::new(),
            ua_pattern: String::new(),
            ua_regexp: None,
            cache_size: args.cache_size,
            buffer_size: args.buffer_size,
            pool_size: args.pool_size,
        };

        cfg.validate()?;
        cfg.process_matching_mode(&args.keywords, &args.pattern)?;

        Ok(cfg)
    }

    fn validate(&self) -> Result<(), String> {
        if self.port == 0 {
            return Err(format!("invalid port: {}", self.port));
        }
        if self.buffer_size < 1024 || self.buffer_size > 65536 {
            return Err(format!("invalid buffer size: {}", self.buffer_size));
        }
        if self.cache_size < 0 {
            return Err(format!("invalid cache size: {}", self.cache_size));
        }
        Ok(())
    }

    fn process_matching_mode(&mut self, keywords: &str, pattern: &str) -> Result<(), String> {
        if self.enable_regex {
            self.ua_pattern = format!("(?i){}", pattern);
            self.ua_regexp = Some(
                Regex::new(&self.ua_pattern)
                    .map_err(|e| format!("invalid User-Agent Regex Pattern: {}", e))?,
            );
        } else if !self.force_replace {
            self.keywords_list = parse_comma_list(keywords);
        }
        Ok(())
    }

    pub fn log_config(&self, version: &str) {
        tracing::info!("UA-MASK v{}", version);
        tracing::info!("Port: {}", self.port);
        tracing::info!("User-Agent: {}", self.user_agent);
        tracing::info!("Log level: {}", self.log_level);
        tracing::info!("User-Agent Whitelist: {:?}", self.whitelist);
        tracing::info!("Cache Size: {}", self.cache_size);
        tracing::info!("Buffer Size: {}", self.buffer_size);
        tracing::info!("Worker Pool Size: {}", self.pool_size);

        if self.force_replace {
            tracing::info!("Mode: Force Replace (All)");
        } else if self.enable_regex {
            tracing::info!(
                "Mode: Regex | Pattern: {} | Partial Replace: {}",
                self.ua_pattern,
                self.enable_partial_replace
            );
        } else {
            tracing::info!("Mode: Keywords | Keywords: {:?}", self.keywords_list);
        }
    }
}

fn parse_comma_list(input: &str) -> Vec<String> {
    if input.is_empty() {
        return Vec::new();
    }
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_comma_list() {
        assert_eq!(parse_comma_list(""), Vec::<String>::new());
        assert_eq!(
            parse_comma_list("a,b,c"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert_eq!(
            parse_comma_list("a, b , c"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn test_validate_port() {
        let cfg = Config {
            port: 0,
            buffer_size: 8192,
            cache_size: 1000,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());

        let cfg = Config {
            port: 8080,
            buffer_size: 8192,
            cache_size: 1000,
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_validate_buffer_size() {
        let cfg = Config {
            port: 8080,
            buffer_size: 512,
            cache_size: 1000,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());

        let cfg = Config {
            port: 8080,
            buffer_size: 100000,
            cache_size: 1000,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());

        let cfg = Config {
            port: 8080,
            buffer_size: 8192,
            cache_size: 1000,
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_validate_cache_size() {
        let cfg = Config {
            port: 8080,
            buffer_size: 8192,
            cache_size: -1,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());

        let cfg = Config {
            port: 8080,
            buffer_size: 8192,
            cache_size: 0,
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            user_agent: String::from("FFF"),
            port: 12032,
            log_level: String::from("info"),
            show_version: false,
            log_file: String::new(),
            whitelist: Vec::new(),
            force_replace: false,
            enable_regex: false,
            enable_partial_replace: false,
            keywords_list: Vec::new(),
            ua_pattern: String::new(),
            ua_regexp: None,
            cache_size: 1000,
            buffer_size: 8192,
            pool_size: 0,
        }
    }
}
