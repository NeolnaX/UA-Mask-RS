# UA-Mask Rust 重构计划

## 项目概述

将 UA-Mask 从 Go 语言重写为 Rust 语言，保持与 OpenWrt 的完全兼容。

## 目标

- 保持与现有 Go 版本相同的功能
- 提升性能 (目标: 超过 Go 版本)
- 保持 OpenWrt 兼容性 (procd, UCI, LuCI)

---

## TODOs

### 1. 项目初始化

- [x] 1.1 创建 Rust 项目结构 (`Cargo.toml`, `src/main.rs`)
- [x] 1.2 添加核心依赖 (tokio, httparse, lru, clap, tracing, nix, libc)
- [x] 1.3 配置 `build.rs` for OpenWrt 交叉编译
- [x] 1.4 创建基础项目框架，验证编译

### 2. 配置系统 (Config)

- [x] 2.1 使用 clap 定义 CLI 参数 (与 Go 版本对齐)
- [x] 2.2 实现 Config 结构体
- [x] 2.3 实现参数验证 (port, buffer size, cache size)
- [x] 2.4 实现三种 UA 匹配模式配置 (force/regex/keywords)

### 3. 核心代理 (Core Proxy)

- [x] 3.1 实现 TCP 服务器 (支持 worker pool 模式)
- [x] 3.2 实现 SO_ORIGINAL_DST 获取 (使用 nix/libc)
- [x] 3.3 实现双向流量转发
- [x] 3.4 实现 Keep-Alive 连接处理

### 4. HTTP 解析与 UA 修改

- [x] 4.1 使用 httparse 解析 HTTP 请求
- [x] 4.2 实现 HTTP 检测 (peek 7 bytes)
- [x] 4.3 实现 UA 匹配逻辑 (keywords/regex/force)
- [x] 4.4 实现 UA 修改 (完整替换/部分替换)
- [x] 4.5 实现 bufio 池化管理

### 5. LRU 缓存

- [x] 5.1 集成 lru crate
- [x] 5.2 实现 UA 缓存逻辑
- [x] 5.3 实现缓存命中统计

### 6. 防火墙管理器 (Firewall Manager)

- [x] 6.1 实现 ipset 批量添加 (shell out)
- [x] 6.2 实现 nftables 批量添加 (shell out)
- [x] 6.3 实现端口画像 (port profiling)
- [x] 6.4 实现流量卸载决策引擎
- [x] 6.5 实现非 HTTP 事件累积和 HTTP 事件重置

### 7. 统计模块 (Stats)

- [x] 7.1 实现原子计数器 (active connections, requests, modified, cache hits)
- [x] 7.2 实现周期性文件写入 (`/tmp/UAmask.stats`)
- [x] 7.3 实现派生指标计算 (RPS, cache ratio)

### 8. 日志系统

- [x] 8.1 集成 tracing + tracing-subscriber
- [x] 8.2 实现日志级别配置 (已在 main.rs)

### 9. 集成测试

- [x] 9.1 单元测试: Config 验证
- [x] 9.2 单元测试: UA 匹配逻辑
- [ ] 9.3 集成测试: 代理流量修改
- [x] 9.4 验证编译通过

### 10. OpenWrt 集成 (可选)

- [ ] 10.1 更新 Makefile 支持 Rust 编译
- [ ] 10.2 更新 init 脚本路径
- [ ] 10.3 验证 UCI 配置兼容

---

## 验收标准

1. Rust 版本编译通过，无警告
2. 功能测试: UA 修改正确工作
3. 性能测试: 吞吐量不低于 Go 版本
4. OpenWrt 兼容性: 可在路由器上运行

## 依赖映射

| Go 依赖 | Rust Crate |
|---------|------------|
| logrus | tracing + tracing-subscriber |
| golang-lru | lru |
| lumberjack | tracing-appender |
| golang.org/x/sys | nix / libc |
| net/http | httparse |
| flag | clap |
| sync.Pool | bytes + 对象池 |

## 文件映射

| Go 文件 | Rust 文件 |
|---------|-----------|
| main.rs | src/main.rs |
| config.rs | src/config.rs |
| handler.rs | src/handler.rs |
| server.rs | src/server.rs |
| manager.rs | src/manager.rs |
| stats.rs | src/stats.rs |
| tproxy.rs | src/tproxy.rs |