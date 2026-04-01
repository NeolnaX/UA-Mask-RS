# Learnings

## Project Initialization (Task 1)

### Dependencies Selected
- tokio 1.x with "full" feature for async runtime
- httparse 1.x for HTTP parsing (lightweight, zero-copy)
- lru 0.12 for LRU cache (matches Go golang-lru)
- clap 4.x with "derive" feature for CLI parsing
- tracing + tracing-subscriber 0.3 for structured logging (replaces logrus)
- tracing-appender 0.2 for log rotation (replaces lumberjack)
- nix 0.27 with socket/net/ioctl features for Linux syscalls (replaces golang.org/x/sys)
- regex 1.x for pattern matching
- bytes 1.x for buffer management

### Build Configuration
- Release profile: opt-level 3, LTO enabled, codegen-units 1, strip symbols
- Added release-lto profile for fat LTO (slower build, smaller binary)

### Cross-Compilation Setup
- build.rs detects cross-compilation via TARGET/HOST env vars
- Sets cfg flags for OpenWrt targets: target_openwrt_mips, target_openwrt_arm, target_openwrt_aarch64

### Verification
- cargo build completed successfully in ~9.44s
- All dependencies resolved without conflicts
