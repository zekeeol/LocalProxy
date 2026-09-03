use std::net::IpAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use bytes::Bytes;
use russh::client::{self, Handler};
use russh::keys::{self, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{Channel, ChannelMsg, Disconnect};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, RwLock};
use tokio_util::sync::CancellationToken;

use crate::api::local_proxy::{Endpoint, EventKind, ServiceState, SshAuthMode};

use super::{
    emit_log, forward_endpoint, known_host, set_reconnecting, set_running, set_status, upgrade,
    AppController, PendingHostKey, StatusUpdate, StoredAppSettings, StoredForward,
    StoredSshProfile,
};

const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
const KEEPALIVE_MAX: usize = 3;
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

type SharedSession = Arc<RwLock<Option<Arc<client::Handle<ProxyClient>>>>>;

#[derive(Debug)]
struct ProxyClient {
    controller: Weak<AppController>,
    profile_id: String,
    host: String,
    port: u16,
    expected_fingerprint: Option<String>,
    key_error: Arc<Mutex<Option<String>>>,
}

#[derive(Debug)]
enum ConnectError {
    Retryable(String),
    Terminal(String),
}

#[derive(Debug)]
enum Socks5ConnectError {
    Retryable(String),
    Terminal(String),
}

pub(crate) async fn run(
    weak: Weak<AppController>,
    id: String,
    config: StoredSshProfile,
    settings: StoredAppSettings,
    mut stop_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let cancellation = CancellationToken::new();
    let result = run_loop(
        &weak,
        &id,
        &config,
        &settings,
        &mut stop_rx,
        cancellation.clone(),
    )
    .await;
    cancellation.cancel();
    result
}

async fn run_loop(
    weak: &Weak<AppController>,
    id: &str,
    config: &StoredSshProfile,
    settings: &StoredAppSettings,
    stop_rx: &mut oneshot::Receiver<()>,
    cancellation: CancellationToken,
) -> Result<(), String> {
    let controller = upgrade(weak)?;
    let mut listeners: Option<BoundListeners> = None;
    let mut shared_session: Option<SharedSession> = None;
    let mut reconnect_delay = Duration::from_secs(1);

    loop {
        let session_result = tokio::select! {
            result = connect_authenticated(&controller, id, config, settings) => result,
            _ = &mut *stop_rx => return Ok(()),
        };
        let session = match session_result {
            Ok(session) => session,
            Err(ConnectError::Terminal(error)) => return Err(error),
            Err(ConnectError::Retryable(error)) => {
                set_reconnecting(&controller, id, error);
                if wait_for_reconnect(stop_rx, reconnect_delay).await {
                    return Ok(());
                }
                reconnect_delay = next_delay(reconnect_delay);
                continue;
            }
        };

        if stop_requested(stop_rx) {
            let _ = session
                .disconnect(Disconnect::ByApplication, "stopped", "en")
                .await;
            return Ok(());
        }

        reconnect_delay = Duration::from_secs(1);
        if listeners.is_none() {
            let bound = bind_listeners(config).await?;
            if stop_requested(stop_rx) {
                let _ = session
                    .disconnect(Disconnect::ByApplication, "stopped", "en")
                    .await;
                return Ok(());
            }
            let endpoints = bound.endpoints.clone();
            let session_slot: SharedSession = Arc::new(RwLock::new(Some(session.clone())));
            spawn_listener_tasks(
                Arc::downgrade(&controller),
                id,
                bound,
                session_slot.clone(),
                cancellation.clone(),
            );
            listeners = Some(BoundListeners { endpoints });
            shared_session = Some(session_slot);
            let endpoints = listeners
                .as_ref()
                .map(|listeners| listeners.endpoints.clone())
                .unwrap_or_default();
            set_running(
                &controller,
                id,
                endpoints,
                Some(format!(
                    "SSH 已连接{}，本地转发已启动",
                    connection_route(config, settings)
                )),
            );
            emit_log(
                &controller,
                id,
                format!(
                    "SSH 已连接到 {}:{}{}",
                    config.host,
                    config.port,
                    connection_route(config, settings)
                ),
            );
        } else if let Some(slot) = &shared_session {
            *slot.write().await = Some(session.clone());
            let endpoints = listeners
                .as_ref()
                .map(|listeners| listeners.endpoints.clone())
                .unwrap_or_default();
            set_running(
                &controller,
                id,
                endpoints,
                Some("SSH 连接已恢复".to_string()),
            );
            emit_log(&controller, id, "SSH 自动重连成功");
        }

        let disconnected = match wait_for_session(&session, stop_rx).await {
            WaitResult::Stopped => return Ok(()),
            WaitResult::Disconnected => true,
        };
        if disconnected {
            if let Some(slot) = &shared_session {
                *slot.write().await = None;
            }
            if listeners.is_some() {
                set_reconnecting(&controller, id, "SSH 连接已断开，正在自动重连".to_string());
            }
        }
    }
}

struct BoundListeners {
    endpoints: Vec<Endpoint>,
}

struct BoundAddresses {
    endpoints: Vec<Endpoint>,
    forward_listeners: Vec<(StoredForward, TcpListener)>,
}

async fn bind_listeners(config: &StoredSshProfile) -> Result<BoundAddresses, String> {
    let mut endpoints = Vec::new();
    let mut forward_listeners = Vec::with_capacity(config.forwards.len());
    for forward in &config.forwards {
        let listener = TcpListener::bind(("127.0.0.1", forward.local_port))
            .await
            .map_err(|error| format!("本地转发端口 {} 监听失败: {error}", forward.local_port))?;
        let local_port = listener
            .local_addr()
            .map_err(|error| format!("读取本地转发端口失败: {error}"))?
            .port();
        endpoints.push(forward_endpoint(
            local_port,
            &forward.remote_host,
            forward.remote_port,
        ));
        forward_listeners.push((
            StoredForward {
                local_port,
                remote_host: forward.remote_host.clone(),
                remote_port: forward.remote_port,
                tag: forward.tag.clone(),
            },
            listener,
        ));
    }

    Ok(BoundAddresses {
        endpoints,
        forward_listeners,
    })
}

fn spawn_listener_tasks(
    controller: Weak<AppController>,
    profile_id: &str,
    bound: BoundAddresses,
    session: SharedSession,
    cancellation: CancellationToken,
) {
    for (forward, listener) in bound.forward_listeners {
        tokio::spawn(forward_listener(
            controller.clone(),
            profile_id.to_string(),
            listener,
            forward,
            session.clone(),
            cancellation.clone(),
        ));
    }
}

async fn forward_listener(
    controller: Weak<AppController>,
    profile_id: String,
    listener: TcpListener,
    forward: StoredForward,
    session: SharedSession,
    cancellation: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => return,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { return; };
                let current = session.read().await.clone();
                let Some(current) = current else { continue; };
                let host = forward.remote_host.clone();
                let port = forward.remote_port;
                let controller = controller.clone();
                let profile_id = profile_id.clone();
                tokio::spawn(async move {
                    // RFC 4254 carries the destination as a host string. The SSH server
                    // resolves DNS names from its own network, just like `ssh -L`.
                    match current
                        .channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", 0)
                        .await
                    {
                        Ok(channel) => {
                            if let Err(error) = bridge_channel(stream, channel).await {
                                if let Some(controller) = controller.upgrade() {
                                    emit_log(
                                        &controller,
                                        &profile_id,
                                        format!(
                                            "SSH 本地转发数据通道异常关闭（远端端口 {port}）：{error}"
                                        ),
                                    );
                                }
                            }
                        }
                        Err(error) => {
                            if let Some(controller) = controller.upgrade() {
                                emit_log(
                                    &controller,
                                    &profile_id,
                                    format!(
                                        "SSH 本地转发目标 channel 打开失败（远端端口 {port}）：{error:?}"
                                    ),
                                );
                            }
                        }
                    }
                });
            }
        }
    }
}

async fn bridge_channel(
    mut stream: TcpStream,
    mut channel: Channel<russh::client::Msg>,
) -> Result<(), String> {
    let mut local_closed = false;
    let mut buffer = vec![0_u8; 16 * 1024];
    loop {
        tokio::select! {
            result = stream.read(&mut buffer), if !local_closed => {
                match result {
                    Ok(0) => {
                        local_closed = true;
                        channel.eof().await.map_err(|error| format!("SSH channel EOF 失败: {error:?}"))?;
                    }
                    Ok(size) => channel
                        .data_bytes(Bytes::copy_from_slice(&buffer[..size]))
                        .await
                        .map_err(|error| format!("写入 SSH channel 失败: {error:?}"))?,
                    Err(error) => return Err(error.to_string()),
                }
            }
            message = channel.wait() => {
                match message {
                    Some(ChannelMsg::Data { data }) => stream.write_all(&data).await.map_err(|error| error.to_string())?,
                    Some(ChannelMsg::ExtendedData { data, .. }) => stream.write_all(&data).await.map_err(|error| error.to_string())?,
                    Some(ChannelMsg::Eof | ChannelMsg::Close) | None => break,
                    Some(ChannelMsg::OpenFailure(_)) => return Err("SSH channel 打开失败".to_string()),
                    Some(_) => {}
                }
            }
        }
    }
    let _ = channel.close().await;
    Ok(())
}

async fn connect_authenticated(
    controller: &Arc<AppController>,
    profile_id: &str,
    config: &StoredSshProfile,
    settings: &StoredAppSettings,
) -> Result<Arc<client::Handle<ProxyClient>>, ConnectError> {
    let key_error = Arc::new(Mutex::new(None));
    let handler = ProxyClient {
        controller: Arc::downgrade(controller),
        profile_id: profile_id.to_string(),
        host: config.host.clone(),
        port: config.port,
        expected_fingerprint: known_host(controller, &config.host, config.port),
        key_error: key_error.clone(),
    };
    let client_config = client::Config {
        nodelay: true,
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        keepalive_max: KEEPALIVE_MAX,
        ..Default::default()
    };
    let socket = connect_ssh_socket(config, settings).await?;
    if client_config.nodelay {
        if let Err(error) = socket.set_nodelay(true) {
            emit_log(
                controller,
                profile_id,
                format!("设置 SSH TCP_NODELAY 失败: {error}"),
            );
        }
    }
    let mut session = client::connect_stream(Arc::new(client_config), socket, handler)
        .await
        .map_err(|error| {
            if let Ok(key_error) = key_error.lock() {
                if let Some(message) = key_error.clone() {
                    return ConnectError::Terminal(message);
                }
            }
            ConnectError::Retryable(format!("SSH 连接失败: {error:?}"))
        })?;

    match config.auth_mode {
        SshAuthMode::Password => {
            let password = controller
                .secrets
                .get(profile_id, "ssh", "password")
                .map_err(ConnectError::Terminal)?;
            let result = session
                .authenticate_password(config.username.clone(), password)
                .await
                .map_err(|error| ConnectError::Terminal(format!("SSH 密码认证失败: {error:?}")))?;
            if !result.success() {
                return Err(ConnectError::Terminal(
                    "SSH 密码认证失败，请检查用户名或密码".to_string(),
                ));
            }
        }
        SshAuthMode::PrivateKey => {
            let passphrase = if config.passphrase_configured {
                Some(
                    controller
                        .secrets
                        .get(profile_id, "ssh", "passphrase")
                        .map_err(ConnectError::Terminal)?,
                )
            } else {
                None
            };
            let key =
                keys::load_secret_key(Path::new(&config.private_key_path), passphrase.as_deref())
                    .map_err(|error| ConnectError::Terminal(format!("读取私钥失败: {error:?}")))?;
            let hash_alg = session
                .best_supported_rsa_hash()
                .await
                .map_err(|error| {
                    ConnectError::Terminal(format!("协商 SSH 密钥算法失败: {error:?}"))
                })?
                .flatten();
            let result = session
                .authenticate_publickey(
                    config.username.clone(),
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
                )
                .await
                .map_err(|error| ConnectError::Terminal(format!("SSH 私钥认证失败: {error:?}")))?;
            if !result.success() {
                return Err(ConnectError::Terminal(
                    "SSH 私钥认证失败，请检查用户名或私钥".to_string(),
                ));
            }
        }
    }
    Ok(Arc::new(session))
}

async fn connect_ssh_socket(
    config: &StoredSshProfile,
    settings: &StoredAppSettings,
) -> Result<TcpStream, ConnectError> {
    if config.use_system_proxy {
        let proxy = settings.socks_proxy.as_ref().ok_or_else(|| {
            ConnectError::Terminal(
                "SSH 已启用系统 SOCKS5 代理，但应用设置中没有配置代理".to_string(),
            )
        })?;
        return connect_via_socks5(&proxy.host, proxy.port, &config.host, config.port)
            .await
            .map_err(|error| match error {
                Socks5ConnectError::Retryable(message) => ConnectError::Retryable(message),
                Socks5ConnectError::Terminal(message) => ConnectError::Terminal(message),
            });
    }

    TcpStream::connect((config.host.as_str(), config.port))
        .await
        .map_err(|error| ConnectError::Retryable(format!("SSH 连接失败: {error}")))
}

async fn connect_via_socks5(
    proxy_host: &str,
    proxy_port: u16,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream, Socks5ConnectError> {
    let proxy_address = format_proxy_address(proxy_host, proxy_port);
    let mut stream = TcpStream::connect((proxy_host, proxy_port))
        .await
        .map_err(|error| {
            Socks5ConnectError::Retryable(format!("SOCKS5 {proxy_address} 暂不可用: {error}"))
        })?;

    stream.write_all(&[5, 1, 0]).await.map_err(|error| {
        Socks5ConnectError::Retryable(format!("SOCKS5 {proxy_address} 握手写入失败: {error}"))
    })?;
    let mut method_reply = [0_u8; 2];
    stream
        .read_exact(&mut method_reply)
        .await
        .map_err(|error| {
            Socks5ConnectError::Retryable(format!("SOCKS5 {proxy_address} 握手读取失败: {error}"))
        })?;
    if method_reply[0] != 5 {
        return Err(Socks5ConnectError::Terminal(format!(
            "{proxy_address} 不是 SOCKS5 服务"
        )));
    }
    match method_reply[1] {
        0 => {}
        0xff => {
            return Err(Socks5ConnectError::Terminal(format!(
                "SOCKS5 {proxy_address} 不支持无认证模式"
            )));
        }
        _ => {
            return Err(Socks5ConnectError::Terminal(format!(
                "SOCKS5 {proxy_address} 要求认证，LocalProxy 仅支持无认证代理"
            )));
        }
    }

    let request = encode_socks5_connect_request(target_host, target_port).map_err(|error| {
        Socks5ConnectError::Terminal(format!("SSH 上游 SOCKS5 地址无效: {error}"))
    })?;
    stream.write_all(&request).await.map_err(|error| {
        Socks5ConnectError::Retryable(format!("SOCKS5 {proxy_address} 请求写入失败: {error}"))
    })?;

    let mut response = [0_u8; 4];
    stream.read_exact(&mut response).await.map_err(|error| {
        Socks5ConnectError::Retryable(format!("SOCKS5 {proxy_address} 响应读取失败: {error}"))
    })?;
    if response[0] != 5 {
        return Err(Socks5ConnectError::Terminal(format!(
            "SOCKS5 {proxy_address} 返回无效响应"
        )));
    }
    consume_socks5_address(&mut stream, response[3]).await?;
    if response[1] != 0 {
        return Err(Socks5ConnectError::Retryable(format!(
            "SOCKS5 {proxy_address} 无法连接 SSH 主机（错误码 {}）",
            response[1]
        )));
    }
    Ok(stream)
}

fn encode_socks5_connect_request(host: &str, port: u16) -> Result<Vec<u8>, String> {
    let mut request = vec![5, 1, 0];
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => {
            request.push(1);
            request.extend_from_slice(&address.octets());
        }
        Ok(IpAddr::V6(address)) => {
            request.push(4);
            request.extend_from_slice(&address.octets());
        }
        Err(_) => {
            let bytes = host.as_bytes();
            if bytes.is_empty() {
                return Err("主机名不能为空".to_string());
            }
            if bytes.len() > usize::from(u8::MAX) {
                return Err("主机名不能超过 255 字节".to_string());
            }
            request.push(3);
            request.push(bytes.len() as u8);
            request.extend_from_slice(bytes);
        }
    }
    request.extend_from_slice(&port.to_be_bytes());
    Ok(request)
}

async fn consume_socks5_address(
    stream: &mut TcpStream,
    address_type: u8,
) -> Result<(), Socks5ConnectError> {
    match address_type {
        1 => {
            let mut address = [0_u8; 4];
            stream.read_exact(&mut address).await.map_err(|error| {
                Socks5ConnectError::Retryable(format!("本机 SOCKS5 地址读取失败: {error}"))
            })?;
        }
        3 => {
            let length = stream.read_u8().await.map_err(|error| {
                Socks5ConnectError::Retryable(format!("本机 SOCKS5 地址读取失败: {error}"))
            })?;
            let mut address = vec![0_u8; usize::from(length)];
            stream.read_exact(&mut address).await.map_err(|error| {
                Socks5ConnectError::Retryable(format!("本机 SOCKS5 地址读取失败: {error}"))
            })?;
        }
        4 => {
            let mut address = [0_u8; 16];
            stream.read_exact(&mut address).await.map_err(|error| {
                Socks5ConnectError::Retryable(format!("本机 SOCKS5 地址读取失败: {error}"))
            })?;
        }
        _ => {
            return Err(Socks5ConnectError::Terminal(
                "本机 SOCKS5 返回不支持的地址类型".to_string(),
            ));
        }
    }
    let mut port = [0_u8; 2];
    stream.read_exact(&mut port).await.map_err(|error| {
        Socks5ConnectError::Retryable(format!("本机 SOCKS5 端口读取失败: {error}"))
    })?;
    Ok(())
}

fn connection_route(config: &StoredSshProfile, settings: &StoredAppSettings) -> String {
    if !config.use_system_proxy {
        return String::new();
    }
    settings
        .socks_proxy
        .as_ref()
        .map(|proxy| {
            format!(
                "（经应用 SOCKS5 {}）",
                format_proxy_address(&proxy.host, proxy.port)
            )
        })
        .unwrap_or_else(|| "（经应用 SOCKS5）".to_string())
}

fn format_proxy_address(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

impl Handler for ProxyClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let fingerprint = format!(
            "{}",
            server_public_key.public_key().fingerprint(HashAlg::Sha256)
        );
        if let Some(expected) = &self.expected_fingerprint {
            if expected == &fingerprint {
                return Ok(true);
            }
            self.set_key_error(format!(
                "SSH 主机密钥不匹配，预期 {expected}，实际 {fingerprint}"
            ));
            return Ok(false);
        }

        let Some(controller) = self.controller.upgrade() else {
            return Ok(false);
        };
        let (sender, receiver) = oneshot::channel();
        if let Ok(mut pending) = controller.pending_host_keys.lock() {
            if let Some(previous) = pending.insert(
                self.profile_id.clone(),
                PendingHostKey {
                    fingerprint: fingerprint.clone(),
                    response: sender,
                },
            ) {
                let _ = previous.response.send(false);
            }
        } else {
            return Ok(false);
        }
        set_status(
            &controller,
            &self.profile_id,
            StatusUpdate {
                state: ServiceState::AwaitingHostKey,
                endpoints: Vec::new(),
                error: None,
                kind: EventKind::HostKey,
                message: Some(format!(
                    "首次连接需要确认 {}:{} 的主机密钥",
                    self.host, self.port
                )),
                fingerprint: Some(fingerprint),
            },
        );
        let accepted = receiver.await.unwrap_or(false);
        if !accepted {
            self.set_key_error("用户拒绝了 SSH 主机密钥".to_string());
        }
        Ok(accepted)
    }
}

impl ProxyClient {
    fn set_key_error(&self, message: String) {
        if let Ok(mut key_error) = self.key_error.lock() {
            *key_error = Some(message);
        }
    }
}

enum WaitResult {
    Stopped,
    Disconnected,
}

async fn wait_for_session(
    session: &Arc<client::Handle<ProxyClient>>,
    stop_rx: &mut oneshot::Receiver<()>,
) -> WaitResult {
    loop {
        tokio::select! {
            _ = &mut *stop_rx => {
                let _ = session.disconnect(Disconnect::ByApplication, "stopped", "en").await;
                return WaitResult::Stopped;
            }
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if session.is_closed() { return WaitResult::Disconnected; }
            }
        }
    }
}

async fn wait_for_reconnect(stop_rx: &mut oneshot::Receiver<()>, delay: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(delay) => false,
        _ = &mut *stop_rx => true,
    }
}

fn stop_requested(stop_rx: &mut oneshot::Receiver<()>) -> bool {
    matches!(
        stop_rx.try_recv(),
        Ok(()) | Err(tokio::sync::oneshot::error::TryRecvError::Closed)
    )
}

fn next_delay(current: Duration) -> Duration {
    std::cmp::min(current.saturating_mul(2), MAX_RECONNECT_DELAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> StoredSshProfile {
        StoredSshProfile {
            host: "example.com".to_string(),
            port: 22,
            username: "tester".to_string(),
            auth_mode: SshAuthMode::Password,
            private_key_path: String::new(),
            password_configured: true,
            passphrase_configured: false,
            forwards: Vec::new(),
            upstream_socks_port: None,
            use_system_proxy: false,
        }
    }

    #[test]
    fn encodes_socks5_ipv4_ipv6_and_domain_targets() {
        assert_eq!(
            encode_socks5_connect_request("127.0.0.1", 22).unwrap(),
            vec![5, 1, 0, 1, 127, 0, 0, 1, 0, 22]
        );
        assert_eq!(
            encode_socks5_connect_request("::1", 443).unwrap(),
            vec![5, 1, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 187]
        );
        assert_eq!(
            encode_socks5_connect_request("example.com", 22).unwrap(),
            vec![
                5, 1, 0, 3, 11, b'e', b'x', b'a', b'm', b'p', b'l', b'e', b'.', b'c', b'o', b'm',
                0, 22
            ]
        );
    }

    #[tokio::test]
    async fn connects_to_ssh_target_through_local_socks5() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let proxy_address = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut greeting = [0_u8; 3];
            stream.read_exact(&mut greeting).await.unwrap();
            assert_eq!(greeting, [5, 1, 0]);
            stream.write_all(&[5, 0]).await.unwrap();

            let mut request_header = [0_u8; 4];
            stream.read_exact(&mut request_header).await.unwrap();
            assert_eq!(request_header, [5, 1, 0, 3]);
            let host_length = stream.read_u8().await.unwrap();
            let mut host = vec![0_u8; usize::from(host_length)];
            stream.read_exact(&mut host).await.unwrap();
            assert_eq!(host, b"example.com");
            let mut port = [0_u8; 2];
            stream.read_exact(&mut port).await.unwrap();
            assert_eq!(u16::from_be_bytes(port), 22);
            stream
                .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 22])
                .await
                .unwrap();

            let mut payload = [0_u8; 3];
            stream.read_exact(&mut payload).await.unwrap();
            assert_eq!(&payload, b"ssh");
        });

        let mut stream = connect_via_socks5("127.0.0.1", proxy_address.port(), "example.com", 22)
            .await
            .unwrap();
        stream.write_all(b"ssh").await.unwrap();
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn random_local_ports_are_bound_and_reported() {
        let mut config = test_config();
        config.forwards = vec![StoredForward {
            local_port: 0,
            remote_host: "192.0.2.10".to_string(),
            remote_port: 6379,
            tag: String::new(),
        }];
        let bound = bind_listeners(&config).await.unwrap();
        assert_eq!(bound.endpoints.len(), 1);
        assert!(bound.endpoints.iter().all(|endpoint| endpoint.port != 0));
    }

    #[tokio::test]
    async fn fixed_local_port_conflict_is_reported() {
        let occupied = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let occupied_port = occupied.local_addr().unwrap().port();
        let mut config = test_config();
        config.forwards = vec![StoredForward {
            local_port: occupied_port,
            remote_host: "192.0.2.10".to_string(),
            remote_port: 6379,
            tag: String::new(),
        }];

        assert!(bind_listeners(&config).await.is_err());
    }

    #[tokio::test]
    async fn fixed_local_port_is_released_when_listeners_are_dropped() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let mut config = test_config();
        config.forwards = vec![StoredForward {
            local_port: port,
            remote_host: "192.0.2.10".to_string(),
            remote_port: 6379,
            tag: String::new(),
        }];
        let bound = bind_listeners(&config).await.unwrap();
        drop(bound);
        assert!(TcpListener::bind(("127.0.0.1", port)).await.is_ok());
    }

    #[test]
    fn reconnect_delay_is_exponential_and_capped() {
        assert_eq!(next_delay(Duration::from_secs(1)), Duration::from_secs(2));
        assert_eq!(next_delay(Duration::from_secs(32)), Duration::from_secs(60));
        assert_eq!(next_delay(Duration::from_secs(60)), Duration::from_secs(60));
    }
}
