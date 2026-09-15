//! Typed command import and embedded upstream wstunnel client.
use std::{collections::HashSet, future::Future, sync::Weak, time::Duration};

use tokio::{sync::oneshot, task::AbortHandle};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use wstunnel::tunnel::listeners::{TcpTunnelListener, UdpTunnelListener};
use wstunnel::{config::Client as ClientCreationRequest, executor::TokioExecutorRef};

use super::{format_forward_target, normalize_forward_host, AppController};
use crate::api::local_proxy::{Endpoint, WstunnelConfig, WstunnelForward};

// Parse shell quoting as data only. Shell expansion and execution are never used.
fn words(command: &str) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && quote != Some('\'') {
            let next = chars.next().ok_or("命令末尾的转义字符不完整")?;
            if next != '\n' {
                word.push(next);
                started = true;
            }
        } else if Some(c) == quote {
            quote = None;
        } else if quote.is_none() && matches!(c, '\'' | '"') {
            quote = Some(c);
            started = true;
        } else if quote.is_none() && c.is_whitespace() {
            if started {
                result.push(std::mem::take(&mut word));
                started = false;
            }
        } else if quote.is_none() && matches!(c, ';' | '|' | '&' | '>' | '<' | '`' | '$') {
            return Err("只支持单条 wstunnel client 命令，不支持 shell 操作符或变量".into());
        } else {
            word.push(c);
            started = true;
        }
    }
    if quote.is_some() {
        return Err("命令引号未闭合".into());
    }
    if started {
        result.push(word);
    }
    Ok(result)
}

pub(crate) fn parse_command(command: &str) -> Result<WstunnelConfig, String> {
    if command.len() > 65536 {
        return Err("命令过长".into());
    }
    let args = words(command)?;
    if args.first().and_then(|s| s.rsplit('/').next()) != Some("wstunnel")
        || args.get(1).map(String::as_str) != Some("client")
    {
        return Err("请输入以 wstunnel client 开头的命令".into());
    }
    let mut config = WstunnelConfig {
        server_url: String::new(),
        http_upgrade_path_prefix: "v1".into(),
        forwards: vec![],
        tls_verify_certificate: false,
    };
    let mut index = 2;
    let mut prefix_seen = false;
    while index < args.len() {
        let arg = &args[index];
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        match flag {
            "-L" | "--local-to-remote" | "-P" | "--http-upgrade-path-prefix" => {
                let value = if let Some(value) = inline { value } else {
                    index += 1;
                    args.get(index).ok_or_else(|| format!("{flag} 缺少参数"))?.as_str()
                };
                if matches!(flag, "-L" | "--local-to-remote") {
                    config.forwards.push(parse_forward(value)?);
                } else {
                    if prefix_seen { return Err("路径前缀不能重复指定".into()); }
                    config.http_upgrade_path_prefix = value.into(); prefix_seen = true;
                }
            }
            "--tls-verify-certificate" if inline.is_none() => config.tls_verify_certificate = true,
            _ if arg.starts_with("ws://") || arg.starts_with("wss://") => {
                if !config.server_url.is_empty() { return Err("只能指定一个 Wstunnel 服务端".into()); }
                config.server_url = arg.clone();
            }
            _ => return Err("命令包含不支持的参数；支持 -L、-P / --http-upgrade-path-prefix、--tls-verify-certificate 和 ws(s) 服务端".into()),
        }
        index += 1;
    }
    validate(&config)?;
    Ok(config)
}

fn parse_forward(raw: &str) -> Result<WstunnelForward, String> {
    // Also accept Markdown-escaped examples inside single quotes, as pasted by users.
    let raw = raw.replace("\\:", ":").replace("\\_", "_");
    let (protocol, rest) = raw
        .split_once("://")
        .ok_or("转发规则应为 udp://端口:主机:端口")?;
    let (address, query) = rest
        .split_once('?')
        .map_or((rest, None), |(a, q)| (a, Some(q)));
    let address = address
        .strip_prefix("127.0.0.1:")
        .or_else(|| address.strip_prefix("localhost:"))
        .unwrap_or(address);
    let (local, target) = address.split_once(':').ok_or("缺少本地端口或目标地址")?;
    let (host, remote) = target.rsplit_once(':').ok_or("缺少目标端口")?;
    let mut timeout_sec = None;
    if let Some(query) = query {
        let value = query
            .strip_prefix("timeout_sec=")
            .ok_or("只支持 timeout_sec 查询参数")?;
        timeout_sec = Some(
            value
                .parse::<u64>()
                .map_err(|_| "timeout_sec 必须是非负整数")?,
        );
    }
    Ok(WstunnelForward {
        protocol: protocol.into(),
        local_port: local
            .parse()
            .map_err(|_| "本地端口必须为 1–65535；仅支持 IPv4 回环监听")?,
        remote_host: normalize_forward_host(host)?,
        remote_port: remote.parse().map_err(|_| "目标端口必须为 1–65535")?,
        timeout_sec,
    })
}

pub(crate) fn validate(config: &WstunnelConfig) -> Result<(), String> {
    let url = url::Url::parse(&config.server_url).map_err(|_| "Wstunnel 服务端 URL 无效")?;
    if !matches!(url.scheme(), "ws" | "wss")
        || url.host_str().is_none()
        || url.port() == Some(0)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err("服务端必须是 ws:// 或 wss://主机[:端口]；路径请使用 Upgrade 前缀配置".into());
    }
    if config.http_upgrade_path_prefix.is_empty()
        || config.http_upgrade_path_prefix.len() > 1024
        || config
            .http_upgrade_path_prefix
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_' | '.' | '~' | '/'))
    {
        return Err("Upgrade 路径前缀不能为空，只支持字母、数字和 - _ . ~ /".into());
    }
    if config.forwards.is_empty() || config.forwards.len() > 64 {
        return Err("需要 1–64 条本地转发规则".into());
    }
    let mut ports = HashSet::new();
    for f in &config.forwards {
        if !matches!(f.protocol.as_str(), "udp" | "tcp") {
            return Err("仅支持 UDP 和 TCP 本地转发".into());
        }
        if f.local_port == 0 || f.remote_port == 0 {
            return Err("Wstunnel 端口必须为 1–65535".into());
        }
        normalize_forward_host(&f.remote_host)?;
        if f.protocol == "tcp" && f.timeout_sec.is_some() {
            return Err("timeout_sec 仅适用于 UDP".into());
        }
        if f.timeout_sec.is_some_and(|s| s > u32::MAX as u64) {
            return Err("timeout_sec 超出支持范围".into());
        }
        if !ports.insert((&f.protocol, f.local_port)) {
            return Err("同一协议的本地端口不能重复".into());
        }
    }
    Ok(())
}

pub(crate) fn endpoints(config: &WstunnelConfig) -> Vec<Endpoint> {
    config
        .forwards
        .iter()
        .map(|f| Endpoint {
            kind: f.protocol.clone(),
            host: "127.0.0.1".into(),
            port: f.local_port,
            description: format!(
                "{} → {}",
                f.protocol.to_uppercase(),
                format_forward_target(&f.remote_host, f.remote_port)
            ),
        })
        .collect()
}

// Every connection task belongs to this profile and is drained before stop completes.
#[derive(Clone, Default)]
struct ProfileExecutor {
    cancel: CancellationToken,
    tasks: TaskTracker,
}
impl TokioExecutorRef for ProfileExecutor {
    fn spawn<F>(&self, future: F) -> AbortHandle
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let cancel = self.cancel.clone();
        self.tasks
            .spawn(async move {
                tokio::select! { biased; _ = cancel.cancelled() => {}, _ = future => {} }
            })
            .abort_handle()
    }
}

fn client_request(config: &WstunnelConfig) -> ClientCreationRequest {
    ClientCreationRequest {
        local_to_remote: vec![],
        remote_to_local: vec![],
        socket_so_mark: None,
        connection_min_idle: 0,
        connection_retry_max_backoff: Duration::from_secs(300),
        reverse_tunnel_connection_retry_max_backoff: Duration::from_secs(1),
        tls_sni_override: None,
        tls_sni_disable: false,
        tls_ech_enable: false,
        tls_verify_certificate: config.tls_verify_certificate,
        http_proxy: None,
        http_proxy_login: None,
        http_proxy_password: None,
        http_upgrade_path_prefix: config.http_upgrade_path_prefix.clone(),
        http_upgrade_credentials: None,
        websocket_ping_frequency: Some(Duration::from_secs(30)),
        websocket_mask_frame: false,
        http_headers: vec![],
        http_headers_file: None,
        remote_addr: url::Url::parse(&config.server_url).expect("validated URL"),
        tls_certificate: None,
        tls_private_key: None,
        dns_resolver: vec![],
        dns_resolver_prefer_ipv4: false,
    }
}

pub(crate) async fn run(
    controller: Weak<AppController>,
    id: String,
    config: WstunnelConfig,
    mut stop: oneshot::Receiver<()>,
) -> Result<(), String> {
    validate(&config)?;
    let executor = ProfileExecutor::default();
    let mut listeners = tokio::task::JoinSet::new();
    let _cancel_on_drop = executor.cancel.clone().drop_guard();
    let service = async {
        let client = wstunnel::create_client(client_request(&config), executor.clone())
            .await
            .map_err(|_| "Wstunnel 客户端初始化失败".to_string())?;
        for f in &config.forwards {
            let bind = ([127, 0, 0, 1], f.local_port).into();
            let host = normalize_forward_host(&f.remote_host)?;
            let host = if host.contains(':') {
                format!("[{host}]")
            } else {
                host
            };
            let dest = (
                url::Host::parse(&host).map_err(|_| "目标主机无效")?,
                f.remote_port,
            );
            let client = client.clone();
            let error = || {
                format!(
                    "无法监听 {} 127.0.0.1:{}，请检查端口是否被占用",
                    f.protocol, f.local_port
                )
            };
            if f.protocol == "udp" {
                let timeout = match f.timeout_sec {
                    Some(0) => None,
                    value => Some(Duration::from_secs(value.unwrap_or(30))),
                };
                let listener = UdpTunnelListener::new(bind, dest, timeout)
                    .await
                    .map_err(|_| error())?;
                listeners.spawn(async move { client.run_tunnel(listener).await });
            } else {
                let listener = TcpTunnelListener::new(bind, dest, false)
                    .await
                    .map_err(|_| error())?;
                listeners.spawn(async move { client.run_tunnel(listener).await });
            }
        }
        if let Some(controller) = controller.upgrade() {
            super::set_running(
                &controller,
                &id,
                endpoints(&config),
                Some("本地 Wstunnel 监听已就绪；收到流量时连接服务端".into()),
            );
        }
        let _ = listeners.join_next().await;
        Err("Wstunnel 监听意外结束".into())
    };
    let result = tokio::select! { biased; _ = &mut stop => Ok(()), result = service => result };
    listeners.abort_all();
    while listeners.join_next().await.is_some() {}
    executor.cancel.cancel();
    executor.tasks.close();
    executor.tasks.wait().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream, UdpSocket};
    use wstunnel::{config::Server as ServerCreationRequest, executor::TokioExecutor};

    impl TokioExecutor for ProfileExecutor {
        type Ref = Self;
        fn ref_clone(&self) -> Self {
            self.clone()
        }
    }

    #[test]
    fn imports_exact_example_and_markdown_escapes() {
        let normal = parse_command("wstunnel client --http-upgrade-path-prefix XXXX -L 'udp://50814:localhost:50814?timeout_sec=0' wss://18.123.135.89:443").unwrap();
        let escaped = parse_command(r"wstunnel client --http-upgrade-path-prefix XXXX -L 'udp\://50814:localhost:50814?timeout\_sec=0' wss\://18.123.135.89:443").unwrap();
        assert_eq!(normal, escaped);
        assert_eq!(normal.forwards[0].timeout_sec, Some(0));
        assert_eq!(normal.http_upgrade_path_prefix, "XXXX");
        assert_eq!(normal.forwards[0].remote_host, "localhost");
        assert_eq!(
            normal,
            serde_json::from_str(&serde_json::to_string(&normal).unwrap()).unwrap()
        );
    }

    #[test]
    fn imports_multiple_rules_ipv6_and_tls_option() {
        let config = parse_command("/opt/homebrew/bin/wstunnel client -P=test \\\n          --local-to-remote='udp://127.0.0.1:5000:[::1]:53' -L \"tcp://5000:db.example.com:5432\" --tls-verify-certificate wss://[::1]:443").unwrap();
        assert_eq!(config.forwards.len(), 2);
        assert_eq!(config.forwards[0].remote_host, "::1");
        assert_eq!(config.forwards[0].timeout_sec, None);
        assert!(config.tls_verify_certificate);
    }

    #[test]
    fn rejects_incomplete_unsupported_and_unsafe_commands() {
        for tail in [
            "",
            "-L",
            "-L 'udp://1:localhost:2",
            "-R udp://1:localhost:2 ws://localhost",
            "-L udp://0:localhost:2 ws://localhost",
            "-L udp://1:localhost:2?timeout_sec=-1 ws://localhost",
            "-L tcp://1:localhost:2?timeout_sec=0 ws://localhost",
            "-L udp://0.0.0.0:1234:localhost:2 ws://localhost",
            "-L udp://1:localhost:2 ws://localhost ; touch /tmp/unwanted",
            "-L udp://1:localhost:2 ws://localhost -L udp://1:localhost:3",
            "-L udp://1:localhost:2 ws://localhost --unknown",
            "-L udp://1:localhost:2 ws://localhost ws://other",
        ] {
            assert!(
                parse_command(&format!("wstunnel client {tail}")).is_err(),
                "accepted {tail}"
            );
        }
    }

    fn config(port: u16, target: u16, server: u16) -> WstunnelConfig {
        parse_command(&format!("wstunnel client -P test -L 'udp://{port}:127.0.0.1:{target}?timeout_sec=0' wss://127.0.0.1:{server}")).unwrap()
    }

    #[tokio::test]
    async fn reports_port_conflict_and_releases_earlier_listeners() {
        let occupied = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let available = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = available.local_addr().unwrap().port();
        drop(available);
        let mut config = config(port, 53, 443);
        config.forwards.push(WstunnelForward {
            local_port: occupied.local_addr().unwrap().port(),
            ..config.forwards[0].clone()
        });
        let (_tx, rx) = oneshot::channel();
        let result = run(Weak::new(), "test".into(), config, rx).await;
        assert!(result.unwrap_err().contains("端口是否被占用"));
        UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn forwards_udp_and_tcp_over_tls_and_releases_ports_on_stop() {
        let echo = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let target = echo.local_addr().unwrap().port();
        let echo_task = tokio::spawn(async move {
            let mut buf = [0; 2048];
            loop {
                let (n, peer) = echo.recv_from(&mut buf).await.unwrap();
                echo.send_to(&buf[..n], peer).await.unwrap();
            }
        });
        let tcp_echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp_target = tcp_echo.local_addr().unwrap().port();
        let tcp_echo_task = tokio::spawn(async move {
            let (mut stream, _) = tcp_echo.accept().await.unwrap();
            let (mut reader, mut writer) = stream.split();
            let _ = tokio::io::copy(&mut reader, &mut writer).await;
        });
        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_port = reservation.local_addr().unwrap().port();
        drop(reservation);
        let executor = ProfileExecutor::default();
        let request = ServerCreationRequest {
            remote_addr: format!("wss://127.0.0.1:{server_port}").parse().unwrap(),
            socket_so_mark: None,
            websocket_ping_frequency: None,
            websocket_mask_frame: false,
            dns_resolver: vec![],
            dns_resolver_prefer_ipv4: false,
            restrict_to: None,
            restrict_http_upgrade_path_prefix: Some(vec!["test".into()]),
            restrict_config: None,
            tls_certificate: None,
            tls_private_key: None,
            tls_client_ca_certs: None,
            http_proxy: None,
            http_proxy_login: None,
            http_proxy_password: None,
            remote_to_local_server_idle_timeout: Duration::from_secs(180),
        };
        let server_exec = executor.clone();
        let server = tokio::spawn(async move { wstunnel::run_server(request, server_exec).await });
        let available = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = available.local_addr().unwrap().port();
        drop(available);
        let mut config = config(port, target, server_port);
        let available_tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp_port = available_tcp.local_addr().unwrap().port();
        drop(available_tcp);
        config.forwards.push(WstunnelForward {
            protocol: "tcp".into(),
            local_port: tcp_port,
            remote_host: "127.0.0.1".into(),
            remote_port: tcp_target,
            timeout_sec: None,
        });
        let (tx, rx) = oneshot::channel();
        let client = tokio::spawn(run(Weak::new(), "test".into(), config, rx));
        let local = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let received = tokio::time::timeout(Duration::from_secs(15), async {
            let mut buf = [0; 2048];
            loop {
                local
                    .send_to(
                        b"localproxy-udp-test",
                        (std::net::Ipv4Addr::LOCALHOST, port),
                    )
                    .await
                    .unwrap();
                if let Ok(Ok((n, _))) =
                    tokio::time::timeout(Duration::from_millis(200), local.recv_from(&mut buf))
                        .await
                {
                    break buf[..n].to_vec();
                }
            }
        })
        .await;
        let tcp_received = tokio::time::timeout(Duration::from_secs(5), async {
            let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, tcp_port))
                .await
                .unwrap();
            stream.write_all(b"tcp-test").await.unwrap();
            let mut buf = [0; 8];
            stream.read_exact(&mut buf).await.unwrap();
            buf
        })
        .await;
        tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), client)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let rebound = UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, port)).await;
        server.abort();
        let _ = server.await;
        executor.cancel.cancel();
        executor.tasks.close();
        executor.tasks.wait().await;
        tcp_echo_task.abort();
        let _ = tcp_echo_task.await;
        echo_task.abort();
        let _ = echo_task.await;
        assert_eq!(received.unwrap(), b"localproxy-udp-test");
        rebound.unwrap();
        assert_eq!(tcp_received.unwrap(), *b"tcp-test");
        TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, tcp_port))
            .await
            .unwrap();
    }
}
