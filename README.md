# LocalProxy

LocalProxy 是一个只面向 macOS 的本地代理桌面应用，使用 Flutter 提供界面，Rust 负责网络服务和生命周期管理。

首期支持：

- 固定上游的 HTTP/HTTPS 反向代理：请求路径和 query 追加到上游基础 URL。
- HTTP/1.1 流式请求/响应转发，配置 headers/cookies 覆盖客户端同名值。
- SSH 密码或私钥文件认证。
- SSH 本地端口转发（`direct-tcpip`）。
- 应用级 HTTP/SOCKS5 出站代理；HTTP profile 和 SSH profile 分别选择是否启用。
- SSH 主机密钥首次确认、指纹校验、15 秒 keepalive 和指数退避自动重连。
- HTTP 和 SSH 转发端口均支持固定端口或系统随机端口。
- 每个代理配置支持多个地址标签；SSH 的每条本地端口转发还支持独立 Tag，运行后界面显示可直接使用的完整本地地址。

所有服务只监听 `127.0.0.1`，没有 CLI 或控制 API。应用重启后会恢复配置，但不会自动连接。

## 开发环境

- macOS
- Flutter 3.41 或更新版本
- Rust 1.94.0
- Xcode command line tools

首次准备依赖并生成桥接代码：

```bash
flutter pub get
flutter_rust_bridge_codegen generate
```

`rust/rust-toolchain.toml` 声明了 `aarch64-apple-darwin` 和 `x86_64-apple-darwin`，因此 native-assets 可以在 Apple Silicon 和 Intel macOS 上构建。

## 使用方式

启动开发应用：

```bash
flutter run -d macos
```

在界面中先打开右上角“应用设置”，分别填写 HTTP 代理和 SOCKS5 代理的主机、端口。然后创建 profile，填写可选的地址标签，按 profile 类型勾选对应的“使用应用级代理”，保存后点击启动。端点卡片会显示实际的完整本地地址并支持复制。SSH profile 可以配置多条本地端口转发。

让 LocalProxy 建立 SSH 连接时经过已有的 SOCKS5：

1. 先启动一个无认证 SOCKS5 服务，例如监听 `127.0.0.1:8099`。
2. 在“应用设置 → SOCKS5 代理”中填写 `127.0.0.1:8099` 并启用。
3. 编辑 SSH profile，勾选“使用应用级 SOCKS5 代理连接 SSH”，保存并启动。
4. LocalProxy 会先连接配置的 SOCKS5，再通过 SOCKS5 CONNECT 到 SSH 主机，在这条连接上建立 SSH 会话；实际链路为：`客户端 → LocalProxy → 127.0.0.1:8099 → SSH 会话 → 目标地址`。

HTTP profile 勾选“使用应用级 HTTP 代理”后，HTTP 上游请求会通过应用设置中的 HTTP 代理发送；HTTP 明文请求使用代理绝对 URI，HTTPS 上游先建立 CONNECT 隧道。代理认证暂不支持。

Headers 表格支持“批量粘贴”：可以粘贴浏览器复制的“header 名称、下一行值”格式，也可以粘贴每行一个 `名称: 值` 的格式。

本地端口转发支持 IPv4、IPv6 和域名。例如配置 `3307 → db-readonly.example.com:3306` 并启动后，使用 TCP 连接 `127.0.0.1:3307` 即可：

```bash
mysql --protocol=TCP -h 127.0.0.1 -P 3307 -u <mysql-user> -p
```

域名会作为 SSH `direct-tcpip` 的目标原样发送，由 SSH 服务器所在环境解析；LocalProxy 本机不需要能解析这个内网域名。因此 Redis 的 IP 转发正常而 MySQL 域名转发失败时，请在 SSH 服务器上检查 `getent hosts <域名>` 和 `nc -vz <域名> 3306`，并确认目标网络允许 SSH 服务器访问。MySQL 与 Redis 在本地转发层都是 TCP，不需要分别实现协议支持。每条规则的 Tag 用于在多个转发地址中快速识别用途。

macOS 产物名称为 `LocalProxy.app`，应用图标使用 `macos/Runner/Assets.xcassets/AppIcon.appiconset` 中的 LocalProxy 网络节点图标。

## 配置和安全

非敏感配置写入：

```text
~/Library/Application Support/LocalProxy/profiles.json
```

文件带有 `schemaVersion`，通过临时文件同步后原子替换。密码、私钥口令以及 HTTP header/cookie 值只写入 macOS Keychain；配置文件和 UI 只显示“已配置”状态，不回显值。私钥本身不会复制到应用目录，只保存用户选择的路径。

HTTP 不支持 CONNECT、HTTP/2、HTTP/3、WebSocket 或 `Location` 重写；SSH 不支持远程端口转发、终端会话和 ssh-agent。上游 TLS 默认严格校验证书。

## 验证

```bash
cd rust
cargo test
cargo clippy -- -D warnings
cd ..
flutter analyze
flutter test
flutter build macos
```

发布前还需要对 `.app` 做代码签名和 notarization。首期 macOS target 使用非 App Sandbox，以便读取用户选择的私钥文件。
