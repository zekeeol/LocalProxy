# LocalProxy

LocalProxy 是 macOS 上的原生本地代理管理器。界面使用 Rust、[GPUI Kit](https://crates.io/crates/gpui-kit) 构建，直接调用同进程 Rust Core；配置和网络转发无需 Node.js、浏览器运行时或独立的 Core 进程。

支持中文、English、日本語和 한국어。配置持久化后，应用重启不会自动连接。

- **HTTP/HTTPS 反向代理**：固定上游、流式 HTTP/1.1、Header 和 Cookie 注入，可使用应用级 HTTP 出站代理。
- **SSH 本地端口转发**：在应用设置中维护可复用的 SSH 连接，支持密码或私钥、首次主机密钥指纹确认、SOCKS5 出站代理、多个转发规则和自动重连。
- **Wstunnel**：UDP/TCP over WebSocket，支持从 `wstunnel client` 命令导入并编辑转发、TLS 校验和 Upgrade 前缀。命令只解析，不通过 shell 执行。
- **WireGuard**：导入和编辑 wg-quick 格式配置，显示接口、握手及收发状态。启动需要 macOS 管理员授权创建 utun、路由和 DNS；唤醒、解锁或窗口重新激活后会重新检查已有隧道。
- 配置列表、详情、本地端点复制、标签、运行日志、提示和语言切换。HTTP、SSH 和 Wstunnel 的本地监听限定为 `127.0.0.1`；WireGuard 创建系统隧道并按配置调整路由和 DNS。

## 开发与运行

需要 macOS、Xcode Command Line Tools 和 rustup。`gpui-app/rust-toolchain.toml` 与 `rust/rust-toolchain.toml` 分别固定 UI 和 Core 使用的 Rust 工具链；首次构建需要访问 crates.io 和 GitHub 上固定提交的 wstunnel 源码。

```bash
# 构建调试版 .app 并运行
./scripts/run-native.sh

# 只构建；产物为 release/LocalProxy.app
./scripts/build-native.sh debug
./scripts/build-native.sh release

# 构建本机架构的 release DMG，输出到 release/
./scripts/build-dmg.sh
```

构建脚本分别编译 GPUI 主程序和 `local_proxy_wireguard_helper`，将它们放到 `.app/Contents/MacOS/`，并对本地构建产物进行临时签名。helper 必须与运行中的主程序相邻；脚本也会将调试或发布版 helper 复制到 `gpui-app/target/<模式>/`，方便之后直接使用 `cargo run`。DMG 脚本需要 Python 3.10 或更新版本；首次执行时会在被忽略的构建目录创建虚拟环境并安装 `scripts/dmg-requirements.txt` 中的打包工具。它复用旧版的应用标识、卷名、图标和拖放安装布局，产物命名为 `LocalProxy-<版本>-<架构>.dmg`。正式分发仍需配置 Developer ID 签名和 notarization。

新版直接复用旧版 `~/Library/Application Support/LocalProxy/` 中的配置、SSH 主机密钥和私密值，无需复制或重新导入。运行时也可用 `LOCALPROXY_DATA_DIR` 指定独立的配置与私密数据目录进行开发或测试（WireGuard helper 和系统网络状态仍由本机共享）：

```bash
LOCALPROXY_DATA_DIR=/tmp/localproxy-test ./scripts/run-native.sh
```

## 使用

点击配置列表旁的“+”选择 HTTP、SSH、Wstunnel 或 WireGuard，再填写并保存配置；已有配置可在详情中编辑。配置类型选择、编辑和应用设置使用统一的圆角对话框，长表单可滚动，底部的保存和取消操作保持可见。

在“应用设置”中配置可选的 HTTP/SOCKS5 出站代理和 SSH 连接。创建 SSH 转发配置时选择已有连接并添加本地转发规则。随机端口填写 `0`，启动后在详情和侧栏显示实际地址。复制 SSH 命令时，若启用了 SOCKS5，会生成依赖用户本机 `connect` 命令的 OpenSSH `ProxyCommand`；应用内的 SSH 转发不依赖该命令。

HTTP Header/Cookie 支持逐项编辑，以及批量粘贴 `name: value`、交替的名称和值或 Cookie 字符串。已保存的敏感值不会回显，留空保留；删除配置项会清除对应值。

Wstunnel 导入示例：

```text
wstunnel client --http-upgrade-path-prefix XXXX -L 'udp://50814:localhost:50814?timeout_sec=0' wss://example.com:443
```

UDP 的 `timeout_sec=0` 表示不设置空闲超时，留空使用库默认值。支持多条 UDP/TCP 规则；本地监听就绪后才显示运行中，实际远端连接在收到本地流量时建立。Wstunnel 不使用应用级 HTTP/SOCKS5 代理。

WireGuard 编辑器接受标准的 `[Interface]` / `[Peer]` 文本。编辑已保存配置时不显示私钥；保留私钥注释或省略 `PrivateKey` 会沿用已有值，粘贴新的 `PrivateKey` 则替换。详细的 helper 生命周期见 [docs/wireguard-helper-lifecycle.md](docs/wireguard-helper-lifecycle.md)。

## 数据与限制

非敏感配置保存在 `profiles.json`；HTTP 敏感值、SSH 密码/私钥口令和 WireGuard 私钥保存在同目录的 `private.json`，文件权限为 `0600`。SSH 私钥文件本身不会复制，应用只保存路径。日志和配置详情不回显敏感值。语言设置保存在 `ui.json`。

HTTP 不支持 CONNECT、HTTP/2、HTTP/3、WebSocket 或 `Location` 重写；SSH 不支持远程端口转发、终端会话和 ssh-agent。WireGuard 的系统网络变更依赖随应用构建的特权 helper。

## 验证

```bash
(cd gpui-app && cargo test --locked)
(cd rust && cargo test --locked)
./scripts/build-native.sh debug
./scripts/build-dmg.sh
```

源码结构：`gpui-app/` 为原生界面和国际化，`rust/src/api/` 为类型化接口，`rust/src/core/` 为网络与持久化实现，`rust/src/bin/` 为 WireGuard helper，`assets/brand/` 为应用图标。
