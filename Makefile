include $(TOPDIR)/rules.mk

# ==============================================
# UA-Mask (Rust) OpenWrt Makefile
# ==============================================
# 使用方法:
# 1. 将整个文件夹复制到 OpenWrt 编译目录的 package/luci 下
# 2. 确保已安装 Rust 工具链: make toolchain/rust/install
# 3. 或者使用预编译二进制 (见下文)
#
# 预编译二进制方式:
#   1. 在 Linux 环境交叉编译: cargo build --release --target x86_64-unknown-linux-musl
#   2. 将二进制重命名并放入 bin/ 目录，命名格式: UAmask-{arch}.ipk
#   3. 或者直接放入 files/ 目录，Makefile 会自动使用
# ==============================================

PKG_NAME:=UAmask
PKG_VERSION:=0.5.0
PKG_RELEASE:=1
PKG_ARCH:=$(shell uname -m)

PKG_MAINTAINER:=Zesuy <hongri580@gmail.com>
PKG_LICENSE:=MIT
PKG_LICENSE_FILES:=LICENSE

PKG_BUILD_DEPENDS:=+libc
PKG_BUILD_FLAGS:=no-mips16

include $(INCLUDE_DIR)/package.mk

# ==============================================
# Package definition
# ==============================================
define Package/UAmask
	SECTION:=net
	CATEGORY:=Network
	SUBMENU:=Web Servers/Proxies
	TITLE:=User-Agent modification proxy (Rust)
	URL:=https://github.com/Zesuy/UA-Mask
	DEPENDS:=+libc +luci-compat
	CONFLICTS:=ua3f-tproxy ua3f-tproxy-ipt UAmask-ipt
endef

define Package/UAmask/description
	High-performance transparent proxy for modifying HTTP User-Agent.
	Rust rewrite for better performance and lower memory usage.
endef

# ==============================================
# Build prepare
# ==============================================
define Build/Prepare
	$(CP) $(CURDIR)/LICENSE $(PKG_BUILD_DIR)/
endef

# ==============================================
# Build compile (使用 Rust 交叉编译)
# ==============================================
define Build/Compile
	# 检查是否有预编译二进制
	@if [ -f "$(CURDIR)/files/UAmask-bin" ]; then \
		echo "使用预编译二进制..."; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "使用系统 Rust 编译..."; \
		cd $(PKG_BUILD_DIR); \
		cargo build --release || { echo "Rust 编译失败，请使用预编译二进制"; exit 1; }; \
	else \
		echo "========================================"; \
		echo "错误: 未找到 Rust 和预编译二进制"; \
		echo ""; \
		echo "请选择以下方式之一:"; \
		echo "1. 安装 Rust: make toolchain/rust/install"; \
		echo "2. 提供预编译二进制到 files/UAmask-bin"; \
		echo "3. 手动交叉编译后放入 files/ 目录"; \
		echo ""; \
		echo "交叉编译示例 (Linux x86_64):"; \
		echo "  rustup target add x86_64-unknown-linux-musl"; \
		echo "  cargo build --release --target x86_64-unknown-linux-musl"; \
		echo "  cp target/x86_64-unknown-linux-musl/release/UAmask \\"; \
		echo "        package/luci/UAmask/files/UAmask-bin"; \
		echo "========================================"; \
		exit 1; \
	fi
endef

# ==============================================
# Package conffiles
# ==============================================
define Package/UAmask/conffiles
/etc/config/UAmask
endef

# ==============================================
# Package install
# ==============================================
define Package/UAmask/install
	# 安装二进制
	$(INSTALL_DIR) $(1)/usr/bin/
	$(INSTALL_BIN) $(PKG_BUILD_DIR)/target/$(RUSTC_TARGET_ARCH)/release/UAmask $(1)/usr/bin/UAmask 2>/dev/null || \
	$(INSTALL_BIN) ./files/UAmask-bin $(1)/usr/bin/UAmask 2>/dev/null || \
	$(INSTALL_BIN) ./files/UAmask $(1)/usr/bin/UAmask

	# 安装 init 脚本
	$(INSTALL_DIR) $(1)/etc/init.d/
	$(INSTALL_BIN) ./files/UAmask.init $(1)/etc/init.d/UAmask
	chmod +x $(1)/etc/init.d/UAmask

	# 安装 UCI 配置
	$(INSTALL_DIR) $(1)/etc/config/
	$(INSTALL_CONF) ./files/UAmask.uci $(1)/etc/config/UAmask

	# 安装 LuCI 界面
	$(INSTALL_DIR) $(1)/usr/lib/lua/luci/model/cbi/
	$(INSTALL_CONF) ./files/luci/cbi.lua $(1)/usr/lib/lua/luci/model/cbi/UAmask.lua

	$(INSTALL_DIR) $(1)/usr/lib/lua/luci/controller/
	$(INSTALL_CONF) ./files/luci/controller.lua $(1)/usr/lib/lua/luci/controller/UAmask.lua
endef

$(eval $(call BuildPackage,UAmask))