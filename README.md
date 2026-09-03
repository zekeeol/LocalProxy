# LocalProxy

LocalProxy 是一个只面向 macOS 的本地代理桌面应用，使用 Electron + React/TypeScript 提供界面，Rust + Tokio 负责网络转发和生命周期管理。Electron 主进程通过 stdin/stdout NDJSON 私有通道驱动 Rust Core；应用不会开放外部控制 API。

首期支持：

- 固定上游的 HTTP/HTTPS 反向代理，支持 HTTP/1.1 流式请求/响应转发。
- Headers/Cookies 配置按名称覆盖客户端同名值，敏感值只写入 macOS Keychain。
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
├── rust/src/core/            # HTTP、SSH、Keychain、持久化与重连实现
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

文件带有 `schemaVersion`，通过临时文件同步后原子替换。密码、私钥口令以及 HTTP header/cookie 值只写入 macOS Keychain；配置文件、UI 和脱敏日志不回显这些值。私钥本身不会复制到应用目录，只保存用户选择的路径。

HTTP 不支持 CONNECT、HTTP/2、HTTP/3、WebSocket 或 `Location` 重写；SSH 不支持远程端口转发、终端会话和 ssh-agent。上游 TLS 默认严格校验证书。

## 验证

```bash
npm run lint
npm run build:electron
npm run build:rust
cargo test --manifest-path rust/Cargo.toml
cargo clippy --manifest-path rust/Cargo.toml -- -D warnings
```
