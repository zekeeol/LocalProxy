# LocalProxy

LocalProxy 是一个只面向 macOS 的本地代理桌面应用，使用 Electron + React/TypeScript 提供界面，Rust + Tokio 负责网络转发和生命周期管理。Electron 主进程通过 stdin/stdout NDJSON 私有通道驱动 Rust Core；应用不会开放外部控制 API。

首期支持：

- 固定上游的 HTTP/HTTPS 反向代理，支持 HTTP/1.1 流式请求/响应转发。
- Headers/Cookies 配置按名称覆盖客户端同名值，敏感值只写入本地私密文件。
- Wstunnel UDP/TCP over WebSocket（ws/wss），支持粘贴客户端命令快速导入。
- SSH 密码或私钥文件认证、`direct-tcpip` 本地端口转发。
- 应用级 HTTP/SOCKS5 出站代理；每个 HTTP/SSH profile 可单独选择是否经过它。
- SSH 主机密钥首次确认、指纹校验、keepalive 和指数退避自动重连。
- HTTP 和 SSH 转发端口均支持固定端口或系统随机端口。
- profile 和每条 SSH 转发规则支持 Tag，运行后界面显示并支持复制完整本地地址。

所有服务只监听 `127.0.0.1`。应用重启后会恢复配置，但不会自动连接。

## 工程结构

```text
.
├── electron/                 # Electron 主进程、preload 与 Rust sidecar 管理
├── src/                      # React/TypeScript UI
├── rust/src/api/             # Electron 可调用的类型化业务 API
├── rust/src/core/            # HTTP、SSH、私密值持久化与重连实现
└── assets/brand/             # LocalProxy 图标源文件
```

## 开发环境

- macOS
- Node.js 18 或更新版本
- Rust 1.94 或更新版本
- Xcode command line tools

安装前端依赖并构建 Rust Core：

```bash
npm install
npm run build:rust
```

启动桌面开发环境：

```bash
npm run dev:desktop
```

也可以分别启动 Vite 和 Electron：

```bash
npm run dev
npm run dev:electron
```

## 使用方式

打开“应用设置”，可以分别填写已有的 HTTP 代理和 SOCKS5 代理地址。它们是 LocalProxy 访问外部服务时使用的出站代理，不会在 LocalProxy 内部额外开启一个 SOCKS5 出口。

创建 HTTP profile 时填写上游基础 URL 和本地端口；端口填写 `0` 会由系统随机分配。Headers/Cookies 支持逐项添加，也支持批量粘贴浏览器复制的“名称、下一行值”格式或每行一个 `名称: 值` 的格式。

创建 SSH profile 时可以添加多条本地转发规则，例如 `3307 → db-readonly.example.com:3306`。远端主机支持域名、IPv4 和 IPv6，域名会作为 SSH `direct-tcpip` 的目标交给 SSH 服务器解析：

```bash
mysql --protocol=TCP -h 127.0.0.1 -P 3307 -u <mysql-user> -p
```

如果 SSH profile 勾选“经过应用级 SOCKS5”，连接路径是：

```text
客户端 → LocalProxy → 配置的 SOCKS5 → SSH 会话 → 目标地址
```

界面中的“复制 SSH 命令”会在启用该选项时生成兼容 OpenSSH `connect` 的命令片段：

```text
ssh -o 'ProxyCommand=connect -S 127.0.0.1:<socks-port> %h %p' ...
```

`connect` 命令由用户环境提供，LocalProxy 只负责在内部建立 SSH 会话和本地端口转发。

## Wstunnel 命令导入

点击“新建代理配置”，在“从命令快速导入”中粘贴并点击“解析并填入配置”：

```bash
wstunnel client --http-upgrade-path-prefix XXXX -L 'udp://50814:localhost:50814?timeout_sec=0' wss://18.123.135.89:443
```

检查名称、服务端、Upgrade 路径前缀和转发规则后保存，再点击启动。本例在 `127.0.0.1:50814` 监听 UDP，通过 WSS 转发到**服务端所在网络**的 `localhost:50814`。`timeout_sec=0` 禁用 UDP 会话空闲超时，省略时使用 30 秒。

- 支持多条 `-L` / `--local-to-remote`、`-P` / `--http-upgrade-path-prefix`、`--tls-verify-certificate`、引号、续行及示例中的 Markdown 转义。
- 支持 UDP/TCP、远端域名及 IPv4/IPv6；本地固定监听 IPv4 回环地址，端口范围 1–65535。
- 不支持的选项和非回环绑定会明确报错。命令只解析为配置，不执行 shell。
- TLS 证书校验遵循原命令：包含 `--tls-verify-certificate` 时开启，也可在编辑器中切换；不包含时允许自签名证书。
- “运行中”表示本地监听已就绪；收到本地流量时才连接远端。停止会关闭监听和该配置的所有连接，重启应用后不会自动连接。
- 内嵌 [官方 wstunnel Rust 库](https://github.com/erebe/wstunnel/tree/v10.5.4/wstunnel)，固定到 v10.5.4 的 Git 提交以兼容 Rust 1.94，无需安装外部 `wstunnel`。首次构建需要访问 GitHub 和 crates.io。
- Wstunnel 暂不使用应用级 HTTP/SOCKS5 代理。Upgrade 路径前缀随配置保存到本地 JSON。

## 构建和打包

构建 macOS arm64 DMG：

```bash
npm run build:mac
```

产物位于 `release/`，包含 `LocalProxy.app` 和 DMG。Rust sidecar 会被放入应用的 `Resources/bin/local_proxy_core`。发布版本仍需配置 Apple Developer ID 签名和 notarization。

## 配置和安全

非敏感配置写入：

```text
~/Library/Application Support/LocalProxy/profiles.json
```

文件带有 `schemaVersion`，通过临时文件同步后原子替换。HTTP header/cookie 值、SSH 密码/私钥口令与 WireGuard 私钥统一写入 `private.json`，文件权限为 `0600`；配置文件、UI 和脱敏日志不回显这些值。SSH 私钥文件本身不会复制到应用目录，只保存用户选择的路径。

HTTP 不支持 CONNECT、HTTP/2、HTTP/3、WebSocket 或 `Location` 重写；SSH 不支持远程端口转发、终端会话和 ssh-agent。HTTP 上游 TLS 默认严格校验证书；Wstunnel 的校验行为见上文。

## 验证

```bash
npm run lint
npm run build:electron
npm run build:rust
cargo test --manifest-path rust/Cargo.toml
cargo clippy --manifest-path rust/Cargo.toml -- -D warnings
```
