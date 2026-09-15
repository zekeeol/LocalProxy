//! wg-quick import and macOS privileged helper client.
use std::collections::HashSet;
use std::net::IpAddr;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{oneshot, Mutex};
use tokio::time::{sleep, timeout};

use super::AppController;
use crate::api::local_proxy::{
    SecretPatch, SecretPatchKind, WireGuardConfig, WireGuardPeer, WireGuardSetupResult,
    WireGuardStatus,
};

const HELPER_SOCKET: &str = "localproxy-wireguard.sock";
const HELPER_PROTOCOL_VERSION: u32 = 2;
static HELPER_START: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WireGuardCommand {
    pub token: String,
    #[serde(flatten)]
    pub action: WireGuardAction,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub(crate) enum WireGuardAction {
    Setup {
        id: String,
        config: WireGuardConfig,
        private_key: String,
    },
    Stop {
        id: String,
    },
    Status {
        id: String,
    },
    Ping,
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum WireGuardResponse {
    Error { error: String },
    Ok { result: serde_json::Value },
}

pub(crate) fn parse_config(raw: &str) -> Result<WireGuardConfig, String> {
    if raw.len() > 131072 {
        return Err("WireGuard 配置过长".into());
    }

    let mut config = WireGuardConfig {
        addresses: Vec::new(),
        dns: Vec::new(),
        mtu: None,
        listen_port: None,
        private_key: SecretPatch {
            name: "private_key".into(),
            value: None,
            kind: SecretPatchKind::Clear,
        },
        peers: Vec::new(),
    };
    let mut current_peer: Option<WireGuardPeer> = None;
    let mut saw_interface = false;

    for (line_number, original) in raw.lines().enumerate() {
        let line = original.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if let Some(peer) = current_peer.take() {
                config.peers.push(peer);
            }
            let section = line
                .trim_matches(|c| c == '[' || c == ']')
                .trim()
                .to_ascii_lowercase();
            match section.as_str() {
                "interface" => saw_interface = true,
                "peer" => {
                    current_peer = Some(WireGuardPeer {
                        public_key: String::new(),
                        endpoint_host: String::new(),
                        endpoint_port: 0,
                        allowed_ips: Vec::new(),
                        persistent_keepalive: None,
                    });
                }
                other => {
                    return Err(format!(
                        "第 {} 行的配置段 [{}] 不支持",
                        line_number + 1,
                        other
                    ))
                }
            }
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("第 {} 行缺少 '=' 分隔符", line_number + 1));
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if !saw_interface {
            return Err("配置必须以 [Interface] 开始".into());
        }

        if let Some(peer) = current_peer.as_mut() {
            match key.as_str() {
                "publickey" => peer.public_key = value.to_string(),
                "endpoint" => parse_endpoint(value, peer)?,
                "allowedips" => peer
                    .allowed_ips
                    .extend(split_values(value, |value| parse_cidr(value))?),
                "persistentkeepalive" => {
                    let seconds = value.parse::<u16>().map_err(|_| {
                        format!(
                            "第 {} 行的 PersistentKeepalive 必须是 0–65535 秒",
                            line_number + 1
                        )
                    })?;
                    peer.persistent_keepalive = Some(seconds);
                }
                "presharedkey" | "allowedips_v4" | "allowedips_v6" => {}
                _ => {}
            }
            continue;
        }

        match key.as_str() {
            "privatekey" => {
                if value.is_empty() {
                    return Err("PrivateKey 不能为空".into());
                }
                config.private_key = SecretPatch {
                    name: "private_key".into(),
                    value: Some(value.to_string()),
                    kind: SecretPatchKind::Set,
                };
            }
            "address" => config
                .addresses
                .extend(split_values(value, |value| parse_cidr(value))?),
            "dns" => config.dns.extend(split_dns(value)?),
            "mtu" => {
                let mtu = value
                    .parse::<u16>()
                    .map_err(|_| format!("第 {} 行的 MTU 必须是 1–65535", line_number + 1))?;
                if mtu < 576 {
                    return Err("MTU 不能小于 576".into());
                }
                config.mtu = Some(mtu);
            }
            "listenport" => {
                let port = value.parse::<u16>().map_err(|_| {
                    format!("第 {} 行的 ListenPort 必须是 1–65535", line_number + 1)
                })?;
                if port == 0 {
                    return Err("ListenPort 必须大于 0".into());
                }
                config.listen_port = Some(port);
            }
            _ => {}
        }
    }

    if let Some(peer) = current_peer.take() {
        config.peers.push(peer);
    }
    validate(&config)?;
    Ok(config)
}

fn split_values<T>(raw: &str, parse: impl Fn(&str) -> Result<T, String>) -> Result<Vec<T>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(parse)
        .collect()
}

fn split_dns(raw: &str) -> Result<Vec<String>, String> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|value| {
            if value.len() > 253
                || value
                    .chars()
                    .any(|c| c.is_whitespace() || matches!(c, '/' | '\\' | '@' | '?'))
            {
                Err(format!("DNS 地址无效: {value}"))
            } else {
                Ok(value.to_string())
            }
        })
        .collect()
}

fn parse_cidr(raw: &str) -> Result<String, String> {
    let (address, prefix) = raw
        .rsplit_once('/')
        .ok_or_else(|| format!("地址必须包含 CIDR 前缀: {raw}"))?;
    let address: IpAddr = address
        .parse()
        .map_err(|_| format!("地址格式无效: {raw}"))?;
    let prefix: u8 = prefix
        .parse()
        .map_err(|_| format!("CIDR 前缀无效: {raw}"))?;
    let max = if address.is_ipv4() { 32 } else { 128 };
    if prefix > max {
        return Err(format!("CIDR 前缀超出范围: {raw}"));
    }
    Ok(format!("{address}/{prefix}"))
}

fn parse_endpoint(raw: &str, peer: &mut WireGuardPeer) -> Result<(), String> {
    let raw = raw.trim();
    let (host, port) = if let Some(stripped) = raw.strip_prefix('[') {
        let Some((host, port)) = stripped.rsplit_once("]:") else {
            return Err("IPv6 Endpoint 必须写作 [地址]:端口".into());
        };
        (host.to_string(), port)
    } else {
        let Some((host, port)) = raw.rsplit_once(':') else {
            return Err("Endpoint 必须写作 主机:端口".into());
        };
        (host.trim().to_string(), port)
    };
    if host.is_empty() || port.is_empty() {
        return Err("Endpoint 主机或端口不能为空".into());
    }
    peer.endpoint_host = host;
    peer.endpoint_port = port
        .parse()
        .map_err(|_| format!("Endpoint 端口无效: {raw}"))?;
    Ok(())
}

pub(crate) fn validate(config: &WireGuardConfig) -> Result<(), String> {
    if config.addresses.is_empty() || config.addresses.len() > 8 {
        return Err("需要 1–8 个接口 Address".into());
    }
    for address in &config.addresses {
        parse_cidr(address)?;
    }
    if config.dns.len() > 8 {
        return Err("最多支持 8 个 DNS 服务器".into());
    }
    for dns in &config.dns {
        split_dns(dns)?;
    }
    match (
        &config.private_key.kind,
        config.private_key.value.as_deref(),
    ) {
        (SecretPatchKind::Keep, None) => {}
        (SecretPatchKind::Set, Some(value)) if !value.trim().is_empty() => {
            decode_key(value).map_err(|_| "PrivateKey 必须是 32 字节 Base64 密钥".to_string())?;
        }
        _ => return Err("PrivateKey 不能为空".into()),
    }
    if config.peers.is_empty() || config.peers.len() > 64 {
        return Err("需要 1–64 个 [Peer] 配置".into());
    }
    let mut public_keys = HashSet::new();
    for peer in &config.peers {
        decode_key(&peer.public_key).map_err(|_| {
            format!(
                "Peer PublicKey 必须是 32 字节 Base64 密钥: {}",
                peer.public_key
            )
        })?;
        if !public_keys.insert(peer.public_key.clone()) {
            return Err("Peer PublicKey 不能重复".into());
        }
        if peer.endpoint_host.is_empty() || peer.endpoint_port == 0 {
            return Err("每个 Peer 都需要 Endpoint".into());
        }
        if peer.allowed_ips.is_empty() {
            return Err("每个 Peer 都需要 AllowedIPs".into());
        }
        for allowed_ip in &peer.allowed_ips {
            parse_cidr(allowed_ip)?;
            if allowed_ip == "0.0.0.0/0" || allowed_ip == "::/0" {
                return Err("当前版本不支持默认路由，请配置具体 AllowedIPs".into());
            }
        }
    }
    Ok(())
}

pub(crate) fn decode_key(value: &str) -> Result<[u8; 32], String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|_| "Base64 密钥格式无效".to_string())?;
    bytes
        .try_into()
        .map_err(|_| "密钥长度必须是 32 字节".to_string())
}

pub(crate) async fn run(
    controller: std::sync::Weak<AppController>,
    id: String,
    config: WireGuardConfig,
    private_key: String,
    stop: oneshot::Receiver<()>,
) -> Result<(), String> {
    validate(&config)?;
    let mut helper_config = config.clone();
    helper_config.private_key = SecretPatch {
        name: "private_key".into(),
        value: Some(private_key.clone()),
        kind: SecretPatchKind::Set,
    };
    validate(&helper_config)?;
    let setup = call_helper(WireGuardAction::Setup {
        id: id.clone(),
        config: helper_config,
        private_key,
    })
    .await
    .and_then(|value| {
        serde_json::from_value::<WireGuardSetupResult>(value).map_err(|e| e.to_string())
    })?;
    if let Some(controller) = controller.upgrade() {
        super::set_running(
            &controller,
            &id,
            endpoints(&setup, &config),
            Some("WireGuard utun 已就绪".into()),
        );
    }
    let mut stopped = stop;
    loop {
        tokio::select! {
            biased;
            _ = &mut stopped => break,
            _ = sleep(Duration::from_secs(5)) => {
                match call_helper(WireGuardAction::Status { id: id.clone() }).await {
                    Ok(value) => {
                        let status: Option<WireGuardStatus> =
                            serde_json::from_value(value).map_err(|e| e.to_string())?;
                        if status.is_none() {
                            return Err("WireGuard helper 已失去该隧道".into());
                        }
                    }
                    Err(error) => return Err(format!("WireGuard helper 通信失败: {error}")),
                }
            }
        }
    }
    let _ = call_helper(WireGuardAction::Stop { id: id.clone() }).await;
    Ok(())
}

pub(crate) fn endpoints(
    setup: &WireGuardSetupResult,
    config: &WireGuardConfig,
) -> Vec<crate::api::local_proxy::Endpoint> {
    vec![crate::api::local_proxy::Endpoint {
        kind: "wireguard".into(),
        host: setup.interface_name.clone(),
        port: 0,
        description: format!(
            "{} → {}",
            config.addresses.join(", "),
            config
                .peers
                .iter()
                .map(|peer| format!("{}:{}", peer.endpoint_host, peer.endpoint_port))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }]
}

pub(crate) async fn status(id: &str) -> Result<Option<WireGuardStatus>, String> {
    let token = helper_token()?;
    let value = call_existing(WireGuardAction::Status { id: id.into() }, token).await?;
    serde_json::from_value(value).map_err(|error| format!("WireGuard 状态解析失败: {error}"))
}

async fn call_helper(action: WireGuardAction) -> Result<serde_json::Value, String> {
    ensure_helper().await?;
    timeout(Duration::from_secs(120), async move {
        let token = helper_token()?;
        call_with_token(action, token).await
    })
    .await
    .map_err(|_| "WireGuard helper 响应超时".to_string())?
}

async fn call_with_token(
    action: WireGuardAction,
    token: String,
) -> Result<serde_json::Value, String> {
    let socket = helper_socket_path();
    let stream = UnixStream::connect(&socket).await.map_err(|error| {
        format!("WireGuard helper 未运行。启动隧道时需要授权创建 utun 设备: {error}")
    })?;
    let command = WireGuardCommand { token, action };
    let (read_half, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let payload =
        serde_json::to_vec(&command).map_err(|e| format!("WireGuard 指令序列化失败: {e}"))?;
    writer
        .write_all(&payload)
        .await
        .map_err(|e| format!("发送 WireGuard 指令失败: {e}"))?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|e| format!("发送 WireGuard 指令失败: {e}"))?;
    writer
        .shutdown()
        .await
        .map_err(|e| format!("关闭 WireGuard 指令通道失败: {e}"))?;
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .map_err(|e| format!("读取 WireGuard 结果失败: {e}"))?;
    let response: WireGuardResponse =
        serde_json::from_str(line.trim()).map_err(|e| format!("WireGuard 结果无效: {e}"))?;
    match response {
        WireGuardResponse::Ok { result } => Ok(result),
        WireGuardResponse::Error { error } => Err(error),
    }
}

fn helper_socket_path() -> std::path::PathBuf {
    // macOS marks the per-user temp directory with sunlnk, so chmod on it
    // returns EPERM. Put the root helper socket in our own subdirectory.
    std::env::temp_dir()
        .join("localproxy-wireguard")
        .join(HELPER_SOCKET)
}

fn helper_token() -> Result<String, String> {
    let path = helper_token_path()?;
    if let Ok(token) = std::fs::read_to_string(&path) {
        let token = token.trim();
        if token.len() == 32 && token.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(token.to_owned());
        }
    }

    let token = uuid::Uuid::new_v4().simple().to_string();
    let parent = path.parent().ok_or("helper token 路径无效")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("创建 helper token 目录失败: {e}"))?;
    std::fs::write(&path, &token).map_err(|e| format!("保存 helper token 失败: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("设置 helper token 权限失败: {e}"))?;
    }
    Ok(token)
}

fn helper_token_path() -> Result<std::path::PathBuf, String> {
    dirs::data_dir()
        .map(|base| base.join("LocalProxy").join("wireguard-helper-token"))
        .ok_or_else(|| "无法确定 helper token 目录".to_string())
}

async fn ensure_helper() -> Result<(), String> {
    let _start_guard = HELPER_START.lock().await;
    if helper_is_current().await {
        return Ok(());
    }
    // Ask any older helper to exit before replacing it. Shutdown exists in
    // previous protocol versions, so this works even when Ping does not.
    let _ = timeout(
        Duration::from_millis(500),
        call_existing(WireGuardAction::Shutdown, helper_token()?),
    )
    .await;
    let token = helper_token()?;
    launch_helper(&token).await
}

async fn helper_is_current() -> bool {
    let Ok(token) = helper_token() else {
        return false;
    };
    match timeout(
        Duration::from_millis(350),
        call_existing(WireGuardAction::Ping, token),
    )
    .await
    {
        Ok(Ok(value)) => {
            value.get("version").and_then(Value::as_u64) == Some(HELPER_PROTOCOL_VERSION.into())
        }
        _ => false,
    }
}

async fn call_existing(
    action: WireGuardAction,
    token: String,
) -> Result<serde_json::Value, String> {
    call_with_token(action, token).await
}

async fn launch_helper(token: &str) -> Result<(), String> {
    let helper = helper_binary_path()?;
    let socket = helper_socket_path();
    let parent = socket.parent().ok_or("WireGuard socket 路径无效")?;
    std::fs::create_dir_all(&parent).map_err(|e| format!("创建 helper 目录失败: {e}"))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("设置 helper 目录权限失败: {e}"))?;
    let _ = std::fs::remove_file(&socket);
    let socket_pattern = shell_quote(&format!(
        "local_proxy_wireguard_helper --socket {}",
        socket.to_string_lossy()
    ));
    let script = format!(
        "do shell script \"pkill -f {} >/dev/null 2>&1 || true; {} --socket {} --token {} >/dev/null 2>&1 &\" with administrator privileges",
        socket_pattern,
        shell_quote(&helper.to_string_lossy()),
        shell_quote(&socket.to_string_lossy()),
        token,
    );
    let output = tokio::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .await
        .map_err(|e| format!("无法请求管理员权限: {e}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "WireGuard helper 启动失败: {}",
            if message.is_empty() {
                "用户拒绝授权".into()
            } else {
                message
            }
        ));
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if socket.exists() {
            return Ok(());
        }
        sleep(Duration::from_millis(80)).await;
    }
    Err("WireGuard helper 启动超时".into())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn helper_binary_path() -> Result<std::path::PathBuf, String> {
    let current = std::env::current_exe().map_err(|e| format!("读取核心程序路径失败: {e}"))?;
    let parent = current.parent().ok_or("核心程序路径无效")?;
    let helper = parent.join("local_proxy_wireguard_helper");
    if helper.exists() {
        Ok(helper)
    } else {
        Err(format!("找不到 WireGuard helper: {}", helper.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[Interface]
PrivateKey = TYjv11EHGusEY8Y/8Lt4kTMEnmIxUsGLI0ZBqhzmi6A=
Address = 192.168.1.6/32
MTU = 1420
DNS = 223.5.5.5
DNS = 172.31.0.2

[Peer]
PublicKey = kwJxAs348vRB9EV3TFbeto7YKJxM3Em25lhyozDDNh4=
Endpoint = localhost:50814
AllowedIPs = 172.16.110.0/24,172.31.0.2/32
PersistentKeepalive = 25
"#;

    #[test]
    fn imports_sample_wg_quick() {
        let config = parse_config(SAMPLE).unwrap();
        assert_eq!(config.addresses, vec!["192.168.1.6/32".to_string()]);
        assert_eq!(
            config.dns,
            vec!["223.5.5.5".to_string(), "172.31.0.2".to_string()]
        );
        assert_eq!(config.mtu, Some(1420));
        assert_eq!(config.peers.len(), 1);
        assert_eq!(config.peers[0].endpoint_host, "localhost");
        assert_eq!(config.peers[0].endpoint_port, 50814);
        assert_eq!(
            config.peers[0].allowed_ips,
            vec!["172.16.110.0/24".to_string(), "172.31.0.2/32".to_string()]
        );
        assert_eq!(config.peers[0].persistent_keepalive, Some(25));
    }

    #[test]
    fn rejects_missing_fields_or_unsafe_route() {
        assert!(parse_config("").is_err());
        assert!(parse_config("[Interface]\nAddress=192.168.1.6/32\n").is_err());
        let bad = SAMPLE.replace("172.16.110.0/24,172.31.0.2/32", "0.0.0.0/0");
        assert!(parse_config(&bad).is_err());
    }
}
