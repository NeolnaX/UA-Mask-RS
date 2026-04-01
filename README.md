# UA-Mask (Rust)

[![Build Status](https://github.com/Zesuy/UA-Mask-Rust/actions/workflows/build.yml/badge.svg)](https://github.com/Zesuy/UA-Mask-Rust/actions)
[![Version](https://img.shields.io/github/v/release/Zesuy/UA-Mask-Rust)](https://github.com/Zesuy/UA-Mask-Rust/releases)
[![License](https://img.shields.io/github/license/Zesuy/UA-Mask-Rust)](LICENSE)

High-performance User-Agent modification proxy for OpenWrt, written in Rust.

## Features

- **Rust Performance**: Near-native performance with minimal memory footprint
- **Transparent Proxy**: TPROXY-based architecture, no application-layer proxy needed
- **LRU Cache**: Efficient caching to reduce repeated matching overhead
- **Multiple Matching Modes**: Keywords, regex, or force replace
- **LuCI Interface**: Complete Web configuration interface

## Installation

### Pre-built Binaries

Download pre-built binaries from [Releases](https://github.com/Zesuy/UA-Mask-Rust/releases):

| Architecture | Binary |
|--------------|--------|
| x86_64 | UAmask-x86_64 |
| aarch64 | UAmask-aarch64 |
| armv7 | UAmask-armv7 |
| mips | UAmask-mips |
| mipsel | UAmask-mipsel |

### Build from Source

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build for your architecture
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

### OpenWrt Package

1. Clone to OpenWrt package directory:
```bash
cp -r UA-Mask-Rust package/luci/
make package/luci/UAmask/compile
```

2. Or use pre-built binary:
```bash
# Put binary in files/ directory
cp UAmask-x86_64 package/luci/UAmask/files/UAmask-bin
make package/luci/UAmask/compile
```

## Usage

```bash
# Basic usage
UAmask -u "CustomUA/1.0" -port 12032

# Keyword mode (default)
UAmask -keywords "iPhone,Android,Windows"

# Regex mode
UAmask -enable-regex -r "(iPhone|iPad|Android)"

# Force replace all
UAmask -force
```

### CLI Options

| Option | Description | Default |
|--------|-------------|---------|
| `-u` | User-Agent to replace with | FFF |
| `-port` | Listen port | 12032 |
| `-loglevel` | Log level (debug/info/warn/error) | info |
| `-w` | User-Agent whitelist | - |
| `-keywords` | Keywords for matching | iPhone,iPad,Android... |
| `-enable-regex` | Enable regex mode | false |
| `-r` | Regex pattern | - |
| `-force` | Replace all User-Agents | false |
| `-cache-size` | LRU cache size | 1000 |
| `-buffer-size` | I/O buffer size | 8192 |
| `-p` | Worker pool size | 0 |

## Configuration

### UCI

```bash
config UAmask 'enabled'
    option enabled '1'

config UAmask 'main'
    option port '12032'
    option ua 'FFF'
    option match_mode 'keywords'
    option keywords 'iPhone,Android,Windows'
```

### LuCI

Access Web UI: Services → UA-Mask

## Performance

| Metric | Go Version | Rust Version |
|--------|------------|--------------|
| Memory | ~10MB | ~3MB |
| Startup | Slower | Faster |

## License

MIT License - see [LICENSE](LICENSE)

## Credits

- Original Go version: [UA-Mask](https://github.com/Zesuy/UA-Mask)
- Based on [UA3F](https://github.com/SunBK201/UA3F)