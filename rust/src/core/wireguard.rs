//! wg-quick import and macOS privileged helper client.
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Mutex as StdMutex, OnceLock};
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
    SecretPatch, SecretPatchKind, WireGuardConfig, WireGuardHealthWarning, WireGuardPeer,
    WireGuardSetupResult, WireGuardStatus,
};
use crate::helper_paths;

/// Serializes helper starts inside this process.
static HELPER_START: Mutex<()> = Mutex::const_new(());
/// Orders wake recovery and user-initiated stop in the desktop core. The
/// helper has its own mutation lock for requests from any client.
pub(crate) static TUNNEL_LIFECYCLE: Mutex<()> = Mutex::const_new(());
/// Health probe budget: three short attempts instead of one, so a helper that
/// is busy running `ifconfig`/`route`/`networksetup` is not mistaken for a dead
/// one (a false negative used to kill a working helper and re-prompt the user
/// for the administrator password).
const PROBE_ATTEMPTS: u32 = 3;
const PROBE_TIMEOUT: Duration = Duration::from_millis(900);
const PROBE_GAP: Duration = Duration::from_millis(150);
/// Minimum stale-handshake threshold for peers with persistent keepalive.
const HEALTHY_HANDSHAKE_AGE_SEC: u64 = 180;
/// Consecutive 5 s supervisor ticks without a handshake before recovery.
const UNHEALTHY_TICKS_BEFORE_RECOVERY: u32 = 3;
/// Minimum time between two automatic recovery attempts.
const RECOVERY_COOLDOWN: Duration = Duration::from_secs(60);
const REFRESH_GRACE: Duration = Duration::from_secs(20);
const SLOW_RECOVERY_COOLDOWN: Duration = Duration::from_secs(300);
const CARRIER_CHECK_AFTER_REBUILDS: u32 = 3;
static HEALTH_WARNINGS: OnceLock<StdMutex<HashMap<String, WireGuardHealthWarning>>> =
    OnceLock::new();
static RECENT_REFRESHES: OnceLock<StdMutex<HashMap<String, std::time::Instant>>> = OnceLock::new();
/// Consecutive helper communication failures before the tunnel is stopped.
const MAX_STATUS_FAILURES: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HelperMode {
    /// May start the privileged helper, which costs the user an administrator
    /// password prompt. Only used for explicit user actions.
    Explicit,
    /// Reuses a running helper only. Never starts a new one.
    Existing,
}

/// Append one line to the helper lifecycle record. Logging never fails the
/// caller: the tunnel state matters more than the log file.
pub(crate) fn log_event(scope: &str, message: &str) {
    let Some(path) = helper_paths::log_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 1_048_576) {
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
    let line = format!("{} [{scope}] {message}\n", timestamp());
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// UTC timestamp without pulling in a date-time dependency.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let days = (secs / 86_400) as i64;
    let seconds_of_day = secs % 86_400;
    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_pivot = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_pivot + 2) / 5 + 1;
    let month = month_pivot + if month_pivot < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds_of_day / 3600,
        (seconds_of_day % 3600) / 60,
        seconds_of_day % 60
    )
}

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
    Refresh {
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

#[derive(Debug, Deserialize)]
struct RefreshResult {
    needs_rebuild: bool,
}

fn warning_store() -> &'static StdMutex<HashMap<String, WireGuardHealthWarning>> {
    HEALTH_WARNINGS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn set_health_warning(id: &str, warning: Option<WireGuardHealthWarning>) {
    if let Ok(mut warnings) = warning_store().lock() {
        match warning {
            Some(warning) => {
                warnings.insert(id.to_string(), warning);
            }
            None => {
                warnings.remove(id);
            }
        }
    }
}

fn health_warning(id: &str) -> Option<WireGuardHealthWarning> {
    warning_store().lock().ok()?.get(id).cloned()
}

fn refresh_store() -> &'static StdMutex<HashMap<String, std::time::Instant>> {
    RECENT_REFRESHES.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn recent_refresh(id: &str) -> Option<std::time::Instant> {
    refresh_store().lock().ok()?.get(id).copied()
}

fn clear_recent_refresh(id: &str) {
    if let Ok(mut refreshes) = refresh_store().lock() {
        refreshes.remove(id);
    }
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
                "allowedips" => peer.allowed_ips.extend(split_values(value, parse_cidr)?),
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
            "address" => config.addresses.extend(split_values(value, parse_cidr)?),
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
    set_health_warning(&id, None);
    clear_recent_refresh(&id);
    let helper_config = helper_config(&config, &private_key)?;
    // Starting the profile is the only path allowed to *start* the privileged
    // helper, i.e. the only path that can cost the user an admin password.
    // Request authorization before taking the tunnel lifecycle lock so a
    // pending password dialog cannot delay stopping another active profile.
    ensure_helper(HelperMode::Explicit).await?;
    let setup = {
        let _lifecycle = TUNNEL_LIFECYCLE.lock().await;
        setup_tunnel(&id, &helper_config, &private_key, HelperMode::Explicit).await?
    };
    log_event(
        "tunnel",
        &format!(
            "{id}: 已就绪 {} → {:?}",
            setup.interface_name,
            describe_peers(&config)
        ),
    );
    if let Some(controller) = controller.upgrade() {
        super::set_running(
            &controller,
            &id,
            endpoints(&setup, &config),
            Some("WireGuard utun 已就绪".into()),
        );
    }

    let mut stopped = stop;
    let started_at = std::time::Instant::now();
    let mut status_failures = 0u32;
    let mut unhealthy_ticks = 0u32;
    let mut recovery_attempts = 0u32;
    let mut last_refresh: Option<std::time::Instant> = None;
    let mut last_recovery = std::time::Instant::now() - RECOVERY_COOLDOWN;

    loop {
        tokio::select! {
            biased;
            _ = &mut stopped => break,
            _ = sleep(Duration::from_secs(5)) => {}
        }
        // Read-only probe: this loop must never restart the helper, otherwise
        // one slow `ifconfig` would kill a healthy tunnel and ask for the
        // administrator password again.
        match call_helper(
            WireGuardAction::Status { id: id.clone() },
            HelperMode::Existing,
        )
        .await
        {
            Ok(value) => {
                status_failures = 0;
                let status: Option<WireGuardStatus> =
                    serde_json::from_value(value).map_err(|e| e.to_string())?;
                let Some(status) = status else {
                    // The helper is alive but no longer owns this tunnel. The
                    // helper is already running, so re-applying the config
                    // needs no new authorization.
                    log_event(
                        "tunnel",
                        &format!("{id}: helper 已失去该隧道，重新下发配置"),
                    );
                    {
                        let _lifecycle = TUNNEL_LIFECYCLE.lock().await;
                        setup_tunnel(&id, &helper_config, &private_key, HelperMode::Existing)
                            .await
                            .map_err(|error| {
                                format!("WireGuard helper 已失去该隧道，且重新下发失败: {error}")
                            })?;
                    }
                    unhealthy_ticks = 0;
                    continue;
                };

                if tunnel_has_handshake(&status) {
                    unhealthy_ticks = 0;
                    recovery_attempts = 0;
                    last_refresh = None;
                    clear_recent_refresh(&id);
                    set_health_warning(&id, None);
                    continue;
                }

                unhealthy_ticks += 1;
                if unhealthy_ticks < UNHEALTHY_TICKS_BEFORE_RECOVERY {
                    continue;
                }
                if started_at.elapsed() < Duration::from_secs(30) {
                    // A fresh tunnel may legitimately have no handshake yet.
                    continue;
                }
                let cooldown = if recovery_attempts >= 5 {
                    SLOW_RECOVERY_COOLDOWN
                } else {
                    RECOVERY_COOLDOWN
                };
                if last_recovery.elapsed() < cooldown {
                    continue;
                }
                if let Some(shared) = recent_refresh(&id) {
                    if last_refresh.is_none_or(|local| shared > local) {
                        last_refresh = Some(shared);
                    }
                }
                if let Some(refreshed_at) = last_refresh {
                    if refreshed_at.elapsed() < REFRESH_GRACE {
                        continue;
                    }
                } else {
                    if recovery_attempts >= CARRIER_CHECK_AFTER_REBUILDS {
                        if let Some(endpoint) = check_local_carrier(&config).await {
                            let warning = format!("本地 UDP 承载端口 {endpoint} 未监听，请检查 wstunnel/SSH 等承载隧道");
                            set_health_warning(
                                &id,
                                Some(WireGuardHealthWarning {
                                    code: "carrier_udp_missing".into(),
                                    endpoint,
                                }),
                            );
                            log_event("health", &format!("{id}: {warning}"));
                            if let Some(controller) = controller.upgrade() {
                                super::emit_log(&controller, &id, warning);
                            }
                            recovery_attempts = recovery_attempts.max(5);
                            last_recovery = std::time::Instant::now();
                            continue;
                        }
                        set_health_warning(&id, None);
                    }
                    let refreshed = {
                        let _lifecycle = TUNNEL_LIFECYCLE.lock().await;
                        refresh_tunnel(&id, &helper_config, &private_key).await
                    };
                    match refreshed {
                        Ok(true) => {
                            last_refresh = Some(std::time::Instant::now());
                            unhealthy_ticks = 0;
                            log_event(
                                "health",
                                &format!(
                                    "{id}: 在 {} 上重新下发 peer，等待握手",
                                    status.interface_name
                                ),
                            );
                            continue;
                        }
                        Ok(false) => log_event("health", &format!("{id}: 配置已变化，需重建设备")),
                        Err(error) => {
                            log_event("health", &format!("{id}: 刷新 peer 失败: {error}"))
                        }
                    }
                }

                recovery_attempts = recovery_attempts.saturating_add(1);
                last_recovery = std::time::Instant::now();
                last_refresh = None;
                clear_recent_refresh(&id);
                unhealthy_ticks = 0;
                log_event(
                    "health",
                    &format!(
                        "{id}: 握手仍未恢复（{}），重建设备（第 {} 次）",
                        handshake_age_desc(&status),
                        recovery_attempts
                    ),
                );
                let recovery = {
                    let _lifecycle = TUNNEL_LIFECYCLE.lock().await;
                    setup_tunnel(&id, &helper_config, &private_key, HelperMode::Existing).await
                };
                if let Err(error) = recovery {
                    log_event("health", &format!("{id}: 重建设备失败: {error}"));
                }
            }
            Err(error) => {
                status_failures += 1;
                log_event(
                    "probe",
                    &format!("{id}: helper 状态查询失败（第 {status_failures} 次）: {error}"),
                );
                if status_failures >= MAX_STATUS_FAILURES {
                    return Err(format!("WireGuard helper 通信失败: {error}"));
                }
            }
        }
    }

    {
        let _lifecycle = TUNNEL_LIFECYCLE.lock().await;
        let _ = call_helper(
            WireGuardAction::Stop { id: id.clone() },
            HelperMode::Existing,
        )
        .await;
    }
    log_event("tunnel", &format!("{id}: 已停止"));
    set_health_warning(&id, None);
    clear_recent_refresh(&id);
    Ok(())
}

fn helper_config(config: &WireGuardConfig, private_key: &str) -> Result<WireGuardConfig, String> {
    let mut helper_config = config.clone();
    helper_config.private_key = SecretPatch {
        name: "private_key".into(),
        value: Some(private_key.to_string()),
        kind: SecretPatchKind::Set,
    };
    validate(&helper_config)?;
    Ok(helper_config)
}

async fn setup_tunnel(
    id: &str,
    helper_config: &WireGuardConfig,
    private_key: &str,
    mode: HelperMode,
) -> Result<WireGuardSetupResult, String> {
    call_helper(
        WireGuardAction::Setup {
            id: id.to_string(),
            config: helper_config.clone(),
            private_key: private_key.to_string(),
        },
        mode,
    )
    .await
    .and_then(|value| {
        serde_json::from_value::<WireGuardSetupResult>(value).map_err(|e| e.to_string())
    })
}

async fn refresh_tunnel(
    id: &str,
    helper_config: &WireGuardConfig,
    private_key: &str,
) -> Result<bool, String> {
    let value = call_helper(
        WireGuardAction::Refresh {
            id: id.to_string(),
            config: helper_config.clone(),
            private_key: private_key.to_string(),
        },
        HelperMode::Existing,
    )
    .await?;
    let result: RefreshResult = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if !result.needs_rebuild {
        if let Ok(mut refreshes) = refresh_store().lock() {
            refreshes.insert(id.to_string(), std::time::Instant::now());
        }
    }
    Ok(!result.needs_rebuild)
}

fn describe_peers(config: &WireGuardConfig) -> String {
    let peers: Vec<String> = config
        .peers
        .iter()
        .map(|peer| format!("{}:{}", peer.endpoint_host, peer.endpoint_port))
        .collect();
    format!("{} peer(s): {}", peers.len(), peers.join(", "))
}

/// Without persistent keepalive, an idle WireGuard peer need not handshake at
/// all. Only treat an old handshake as a failure for peers that are configured
/// to send keepalives (and therefore should keep making progress while idle).
fn tunnel_has_handshake(status: &WireGuardStatus) -> bool {
    if status.peers.is_empty() {
        return false;
    }
    let monitored: Vec<_> = status
        .peers
        .iter()
        .filter(|peer| peer.persistent_keepalive.is_some_and(|seconds| seconds > 0))
        .collect();
    monitored.is_empty()
        || monitored.iter().any(|peer| {
            let keepalive = u64::from(peer.persistent_keepalive.unwrap_or_default());
            let max_age = HEALTHY_HANDSHAKE_AGE_SEC.max(keepalive.saturating_mul(2) + 30);
            peer.handshake_age_sec.is_some_and(|age| age <= max_age)
        })
}

fn handshake_age_desc(status: &WireGuardStatus) -> String {
    match status
        .peers
        .iter()
        .filter_map(|peer| peer.handshake_age_sec)
        .min()
    {
        Some(age) => format!("最近握手 {age} 秒前"),
        None => "从未完成握手".to_string(),
    }
}

/// A localhost WireGuard endpoint needs a local UDP carrier such as wstunnel.
/// `lsof` is only diagnostic: failure to inspect sockets never blocks recovery.
async fn check_local_carrier(config: &WireGuardConfig) -> Option<String> {
    let peers: Vec<(String, u16)> = config
        .peers
        .iter()
        .filter(|peer| {
            matches!(
                peer.endpoint_host.as_str(),
                "localhost" | "127.0.0.1" | "::1"
            )
        })
        .map(|peer| (peer.endpoint_host.clone(), peer.endpoint_port))
        .collect();
    if peers.is_empty() {
        return None;
    }
    tokio::task::spawn_blocking(move || {
        let helper_pid = std::fs::read_to_string(helper_paths::pid_path())
            .ok()
            .and_then(|pid| pid.trim().parse::<u32>().ok());
        for (host, port) in peers {
            let Ok(output) = std::process::Command::new("/usr/sbin/lsof")
                .args(["-nP", &format!("-iUDP:{port}"), "-F", "pn"])
                .output()
            else {
                return None;
            };
            if !udp_listener_present(&String::from_utf8_lossy(&output.stdout), port, helper_pid) {
                return Some(format!("{host}:{port}"));
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}

fn udp_listener_present(output: &str, port: u16, helper_pid: Option<u32>) -> bool {
    let suffix = format!(":{port}");
    let mut current_pid = None;
    for line in output.lines() {
        if let Some(pid) = line.strip_prefix('p') {
            current_pid = pid.parse::<u32>().ok();
        } else if let Some(socket) = line.strip_prefix('n') {
            let local = socket.split("->").next().unwrap_or_default();
            if current_pid.is_some() && current_pid != helper_pid && local.ends_with(&suffix) {
                return true;
            }
        }
    }
    false
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
    // Bounded so the 5 s UI poll cannot pile up requests behind a busy helper.
    let value = timeout(
        Duration::from_secs(4),
        call_helper(
            WireGuardAction::Status { id: id.into() },
            HelperMode::Existing,
        ),
    )
    .await
    .map_err(|_| "WireGuard 状态查询超时".to_string())??;
    let mut status: Option<WireGuardStatus> = serde_json::from_value(value)
        .map_err(|error| format!("WireGuard 状态解析失败: {error}"))?;
    if let Some(status) = status.as_mut() {
        status.health_warning = health_warning(id);
    }
    Ok(status)
}

/// Re-applies the configuration of a running tunnel without starting a new
/// helper, so a wake-up or a broken handshake can be recovered without asking
/// for the administrator password again.
pub(crate) async fn revalidate(
    id: &str,
    config: &WireGuardConfig,
    private_key: &str,
    mode: HelperMode,
) -> Result<Option<WireGuardStatus>, String> {
    let helper_config = helper_config(config, private_key)?;
    let value = timeout(
        Duration::from_secs(4),
        call_helper(
            WireGuardAction::Status { id: id.into() },
            HelperMode::Existing,
        ),
    )
    .await
    .map_err(|_| "WireGuard 状态查询超时".to_string())??;
    let status: Option<WireGuardStatus> = serde_json::from_value(value)
        .map_err(|error| format!("WireGuard 状态解析失败: {error}"))?;
    let needs_refresh = match status.as_ref() {
        None => true,
        // On-demand peers have no idle health signal. Wake is a deliberate
        // one-time opportunity to re-arm them without rebuilding the device.
        Some(status) => {
            !tunnel_has_handshake(status)
                || !status
                    .peers
                    .iter()
                    .any(|peer| peer.persistent_keepalive.is_some_and(|sec| sec > 0))
        }
    };
    if !needs_refresh {
        return Ok(status);
    }
    log_event("health", &format!("{id}: 唤醒后刷新 peer 配置"));
    let refreshed = if status.is_none() {
        false
    } else {
        match refresh_tunnel(id, &helper_config, private_key).await {
            Ok(refreshed) => refreshed,
            Err(error) => {
                log_event(
                    "health",
                    &format!("{id}: 刷新 peer 失败，尝试重建: {error}"),
                );
                false
            }
        }
    };
    if !refreshed {
        setup_tunnel(id, &helper_config, private_key, mode).await?;
    }
    Ok(status)
}

async fn call_helper(
    action: WireGuardAction,
    mode: HelperMode,
) -> Result<serde_json::Value, String> {
    ensure_helper(mode).await?;
    let token = helper_token()?;
    timeout(Duration::from_secs(120), call_with_token(action, token))
        .await
        .map_err(|_| "WireGuard helper 响应超时".to_string())?
}

async fn call_with_token(
    action: WireGuardAction,
    token: String,
) -> Result<serde_json::Value, String> {
    let socket = helper_paths::socket_path();
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
    timeout(Duration::from_secs(30), reader.read_line(&mut line))
        .await
        .map_err(|_| "读取 WireGuard 结果超时".to_string())?
        .map_err(|e| format!("读取 WireGuard 结果失败: {e}"))?;
    let response: WireGuardResponse =
        serde_json::from_str(line.trim()).map_err(|e| format!("WireGuard 结果无效: {e}"))?;
    match response {
        WireGuardResponse::Ok { result } => Ok(result),
        WireGuardResponse::Error { error } => Err(error),
    }
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

async fn ensure_helper(mode: HelperMode) -> Result<(), String> {
    if helper_is_current().await {
        return Ok(());
    }
    if mode == HelperMode::Existing {
        // Never start the helper implicitly: that would raise an administrator
        // password prompt behind the user's back.
        return Err("WireGuard helper 未在运行（启动隧道需要管理员授权）".into());
    }

    let _start_guard = HELPER_START.lock().await;
    if helper_is_current().await {
        return Ok(());
    }
    // Best effort: let a predecessor restore its own routes and DNS. If it does
    // not answer, the new helper's single-instance lock takes over anyway.
    if let Ok(token) = helper_token() {
        let _ = timeout(
            Duration::from_secs(1),
            call_existing(WireGuardAction::Shutdown, token),
        )
        .await;
    }
    let token = helper_token()?;
    launch_helper(&token).await
}

/// A helper counts as current only when it answers with our exact protocol
/// version. Version 3 introduced the single-instance lock and full teardown, so
/// an older helper is deliberately replaced instead of being reused.
async fn helper_is_current() -> bool {
    let Ok(token) = helper_token() else {
        return false;
    };
    for attempt in 0..PROBE_ATTEMPTS {
        let probe = timeout(
            PROBE_TIMEOUT,
            call_existing(WireGuardAction::Ping, token.clone()),
        )
        .await;
        // Failed attempts (including a timeout) are not logged here: callers
        // poll every few seconds and would fill the log with one line per probe.
        if let Ok(Ok(value)) = probe {
            if value.get("version").and_then(Value::as_u64)
                == Some(u64::from(helper_paths::HELPER_PROTOCOL_VERSION))
            {
                return true;
            }
            log_event(
                "probe",
                &format!(
                    "helper 版本不符（期望 {}），将被替换",
                    helper_paths::HELPER_PROTOCOL_VERSION
                ),
            );
            return false;
        }
        if attempt + 1 < PROBE_ATTEMPTS {
            sleep(PROBE_GAP).await;
        }
    }
    false
}

async fn call_existing(
    action: WireGuardAction,
    token: String,
) -> Result<serde_json::Value, String> {
    call_with_token(action, token).await
}

async fn launch_helper(token: &str) -> Result<(), String> {
    let helper = helper_binary_path()?;
    let socket = helper_paths::socket_path();
    let pid_file = helper_paths::pid_path();
    let parent = socket.parent().ok_or("WireGuard socket 路径无效")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("创建 helper 目录失败: {e}"))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("设置 helper 目录权限失败: {e}"))?;

    // The socket file is deliberately left untouched here: the helper replaces
    // it itself, after taking the single-instance lock and reaping any
    // predecessor. Removing it here is exactly what used to orphan the old
    // helper and leak its utun device.
    let previous = std::fs::read_to_string(&pid_file)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let script = format!(
        "do shell script \"{} --socket {} --token {} >/dev/null 2>&1 &\" with administrator privileges",
        shell_quote(&helper.to_string_lossy()),
        shell_quote(&socket.to_string_lossy()),
        token,
    );
    log_event(
        "launch",
        &format!(
            "请求启动 helper（替换 pid={}）",
            previous.clone().unwrap_or_else(|| "无".into())
        ),
    );
    let output = tokio::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .await
        .map_err(|e| format!("无法请求管理员权限: {e}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let reason = if message.is_empty() {
            "用户拒绝授权".to_string()
        } else {
            message
        };
        log_event("launch", &format!("helper 启动失败: {reason}"));
        return Err(format!("WireGuard helper 启动失败: {reason}"));
    }

    // Confirm the new helper is the one answering, with the expected protocol.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if socket.exists() && helper_is_current().await {
            let pid = std::fs::read_to_string(&pid_file)
                .unwrap_or_default()
                .trim()
                .to_string();
            log_event(
                "launch",
                &format!(
                    "helper 就绪 pid={}（替换 pid={}）",
                    if pid.is_empty() { "未知".into() } else { pid },
                    previous.unwrap_or_else(|| "无".into())
                ),
            );
            return Ok(());
        }
        sleep(Duration::from_millis(200)).await;
    }
    log_event("launch", "helper 启动超时（socket 未就绪或版本不符）");
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

    fn status_with(ages: Vec<Option<u64>>, keepalive: Option<u16>) -> WireGuardStatus {
        WireGuardStatus {
            interface_name: "utun9".into(),
            listen_port: 0,
            public_key: None,
            health_warning: None,
            peers: ages
                .into_iter()
                .map(|age| crate::api::local_proxy::WireGuardPeerStatus {
                    public_key: "key".into(),
                    endpoint: None,
                    allowed_ips: Vec::new(),
                    persistent_keepalive: keepalive,
                    handshake_age_sec: age,
                    rx_bytes: 0,
                    tx_bytes: 0,
                })
                .collect(),
        }
    }

    /// Only peers expected to send traffic while idle need a recent handshake.
    #[test]
    fn health_requires_a_recent_handshake() {
        assert!(tunnel_has_handshake(&status_with(vec![Some(5)], Some(25))));
        assert!(tunnel_has_handshake(&status_with(
            vec![None, Some(HEALTHY_HANDSHAKE_AGE_SEC)],
            Some(25)
        )));
        assert!(!tunnel_has_handshake(&status_with(
            vec![Some(HEALTHY_HANDSHAKE_AGE_SEC + 1)],
            Some(25)
        )));
        assert!(!tunnel_has_handshake(&status_with(vec![None], Some(25))));
        assert!(!tunnel_has_handshake(&status_with(
            vec![None, Some(600)],
            Some(25)
        )));
        assert!(tunnel_has_handshake(&status_with(vec![None], None)));
        assert!(tunnel_has_handshake(&status_with(vec![Some(600)], Some(0))));
        assert!(tunnel_has_handshake(&status_with(
            vec![Some(500)],
            Some(300)
        )));
        assert!(!tunnel_has_handshake(&status_with(
            vec![Some(631)],
            Some(300)
        )));
        assert_eq!(
            handshake_age_desc(&status_with(vec![None], Some(25))),
            "从未完成握手"
        );
        assert_eq!(
            handshake_age_desc(&status_with(vec![Some(42)], Some(25))),
            "最近握手 42 秒前"
        );
    }

    #[test]
    fn carrier_probe_excludes_the_wireguard_helper_socket() {
        let only_helper = "p123\nn*:50814\n";
        assert!(!udp_listener_present(only_helper, 50814, Some(123)));
        let with_carrier = "p123\nn*:50814\np456\nn127.0.0.1:50814\n";
        assert!(udp_listener_present(with_carrier, 50814, Some(123)));
        assert!(!udp_listener_present(with_carrier, 50815, Some(123)));
    }

    #[test]
    fn timestamp_is_utc_iso8601() {
        let stamp = timestamp();
        assert_eq!(stamp.len(), 20, "unexpected timestamp: {stamp}");
        assert!(stamp.ends_with('Z'));
        assert!(stamp.starts_with("20"));
        assert!(stamp.contains('T'));
    }

    #[test]
    fn helper_config_carries_the_private_key_patch() {
        const KEY: &str = "TYjv11EHGusEY8Y/8Lt4kTMEnmIxUsGLI0ZBqhzmi6A=";
        let config = parse_config(SAMPLE).unwrap();
        let patched = helper_config(&config, KEY).unwrap();
        assert_eq!(patched.private_key.value.as_deref(), Some(KEY));
        assert_eq!(patched.private_key.kind, SecretPatchKind::Set);
    }

    #[test]
    fn helper_paths_agree_with_the_helper_side() {
        // The helper derives its pid/lock files from --socket, so the socket
        // path must be the parent of both.
        let socket = helper_paths::socket_path();
        assert_eq!(
            socket.file_name().unwrap(),
            helper_paths::HELPER_SOCKET_FILE
        );
        assert_eq!(
            helper_paths::sibling(&socket, helper_paths::HELPER_PID_FILE),
            helper_paths::pid_path()
        );
        // The helper refuses a client whose protocol version it does not match,
        // so the takeover guarantees must stay paired with a version bump.
        const { assert!(helper_paths::HELPER_PROTOCOL_VERSION >= 4) };
    }
}
