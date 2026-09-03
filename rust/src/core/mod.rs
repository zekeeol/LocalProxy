mod http_proxy;
mod ssh_proxy;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use serde::{Deserialize, Serialize};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{broadcast, oneshot};
use uuid::Uuid;

use crate::api::local_proxy::{
    AppSettingsDraft, AppSettingsView, Endpoint, EventKind, ForwardView, HttpDraft, HttpView,
    ProfileDraft, ProfileSummary, ProfileView, ProxyEndpoint, ProxyEvent, ProxyType,
    SecretFieldView, SecretPatch, SecretPatchKind, ServiceState, SshAuthMode, SshDraft, SshView,
    StartOutcome,
};

const SCHEMA_VERSION: u32 = 1;
const SERVICE_NAME: &str = "com.localproxy.LocalProxy";
const CONFIG_DIRECTORY: &str = "LocalProxy";
const CONFIG_FILE: &str = "profiles.json";

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static CONTROLLER: OnceLock<Arc<AppController>> = OnceLock::new();

#[derive(Debug)]
pub(crate) struct AppController {
    data_dir: PathBuf,
    store: Mutex<StoreFile>,
    secrets: SecretStore,
    active: Mutex<HashMap<String, ActiveService>>,
    statuses: Mutex<HashMap<String, RuntimeSnapshot>>,
    pending_host_keys: Mutex<HashMap<String, PendingHostKey>>,
    events: broadcast::Sender<ProxyEvent>,
}

#[derive(Debug)]
struct ActiveService {
    stop: oneshot::Sender<()>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeSnapshot {
    pub state: ServiceState,
    pub endpoints: Vec<Endpoint>,
    pub error: Option<String>,
}

#[derive(Debug)]
struct PendingHostKey {
    fingerprint: String,
    response: oneshot::Sender<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoreFile {
    schema_version: u32,
    profiles: Vec<StoredProfile>,
    known_hosts: HashMap<String, String>,
    #[serde(default)]
    settings: StoredAppSettings,
}

impl Default for StoreFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            profiles: Vec::new(),
            known_hosts: HashMap::new(),
            settings: StoredAppSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct StoredAppSettings {
    #[serde(default)]
    pub(crate) http_proxy: Option<StoredProxyEndpoint>,
    #[serde(default)]
    pub(crate) socks_proxy: Option<StoredProxyEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredProxyEndpoint {
    pub(crate) host: String,
    pub(crate) port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredProfile {
    id: String,
    name: String,
    #[serde(default)]
    tags: Vec<String>,
    kind: ProxyType,
    http: Option<StoredHttpProfile>,
    ssh: Option<StoredSshProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredHttpProfile {
    listen_port: u16,
    upstream_url: String,
    headers: Vec<StoredSecretField>,
    cookies: Vec<StoredSecretField>,
    #[serde(default)]
    use_system_proxy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSshProfile {
    host: String,
    port: u16,
    username: String,
    auth_mode: SshAuthMode,
    private_key_path: String,
    password_configured: bool,
    passphrase_configured: bool,
    forwards: Vec<StoredForward>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upstream_socks_port: Option<u16>,
    #[serde(default)]
    use_system_proxy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSecretField {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredForward {
    local_port: u16,
    remote_host: String,
    remote_port: u16,
    #[serde(default)]
    tag: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SecretStore;

impl SecretStore {
    fn username(profile_id: &str, category: &str, name: &str) -> String {
        format!("{profile_id}:{category}:{name}")
    }

    fn entry(profile_id: &str, category: &str, name: &str) -> Result<keyring::Entry, String> {
        let username = Self::username(profile_id, category, name);
        keyring::Entry::new(SERVICE_NAME, &username)
            .map_err(|error| format!("Keychain 初始化失败: {error:?}"))
    }

    fn set(&self, profile_id: &str, category: &str, name: &str, value: &str) -> Result<(), String> {
        Self::entry(profile_id, category, name)?
            .set_password(value)
            .map_err(|error| format!("Keychain 写入失败: {error:?}"))
    }

    fn get(&self, profile_id: &str, category: &str, name: &str) -> Result<String, String> {
        Self::entry(profile_id, category, name)?
            .get_password()
            .map_err(|error| format!("读取 Keychain 密钥失败: {error:?}"))
    }

    fn delete(&self, profile_id: &str, category: &str, name: &str) {
        if let Ok(entry) = Self::entry(profile_id, category, name) {
            let _ = entry.delete_credential();
        }
    }

    fn delete_profile(&self, profile: &StoredProfile) {
        if let Some(http) = &profile.http {
            for field in &http.headers {
                self.delete(&profile.id, "http-header", &field.name);
            }
            for field in &http.cookies {
                self.delete(&profile.id, "http-cookie", &field.name);
            }
        }
        if profile.ssh.is_some() {
            self.delete(&profile.id, "ssh", "password");
            self.delete(&profile.id, "ssh", "passphrase");
        }
    }
}

pub(crate) fn init() {
    if CONTROLLER.get().is_some() {
        return;
    }

    let _ = RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .thread_name("local-proxy-rust")
            .build()
            .expect("failed to create Tokio runtime")
    });

    let data_dir = data_directory();
    let _ = fs::create_dir_all(&data_dir);
    let store = load_store(&data_dir).unwrap_or_default();
    let (events, _) = broadcast::channel(256);
    let controller = Arc::new(AppController {
        data_dir,
        store: Mutex::new(store),
        secrets: SecretStore,
        active: Mutex::new(HashMap::new()),
        statuses: Mutex::new(HashMap::new()),
        pending_host_keys: Mutex::new(HashMap::new()),
        events,
    });
    let _ = CONTROLLER.set(controller);
}

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .thread_name("local-proxy-rust")
            .build()
            .expect("failed to create Tokio runtime")
    })
}

fn controller() -> Arc<AppController> {
    init();
    CONTROLLER
        .get()
        .expect("LocalProxy controller was not initialized")
        .clone()
}

fn data_directory() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(CONFIG_DIRECTORY)
}

fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CONFIG_FILE)
}

fn load_store(data_dir: &Path) -> Result<StoreFile, String> {
    let path = config_path(data_dir);
    if !path.exists() {
        return Ok(StoreFile::default());
    }
    let bytes = fs::read(&path).map_err(|error| format!("读取配置失败: {error}"))?;
    let mut store: StoreFile = serde_json::from_slice(&bytes)
        .map_err(|error| format!("配置文件格式无效，请检查 {}: {error}", path.display()))?;
    if store.schema_version != SCHEMA_VERSION {
        return Err(format!("不支持的配置版本: {}", store.schema_version));
    }
    store
        .profiles
        .retain(|profile| !profile.id.trim().is_empty());
    migrate_legacy_ssh_proxy(&mut store);
    Ok(store)
}

fn migrate_legacy_ssh_proxy(store: &mut StoreFile) {
    let Some(legacy_port) = store.profiles.iter().find_map(|profile| {
        profile
            .ssh
            .as_ref()
            .and_then(|ssh| ssh.upstream_socks_port)
            .filter(|port| *port != 0)
    }) else {
        return;
    };

    let migrated_port = if let Some(proxy) = store.settings.socks_proxy.as_ref() {
        proxy.port
    } else {
        store.settings.socks_proxy = Some(StoredProxyEndpoint {
            host: "127.0.0.1".to_string(),
            port: legacy_port,
        });
        legacy_port
    };
    for profile in &mut store.profiles {
        if let Some(ssh) = profile.ssh.as_mut() {
            if ssh.upstream_socks_port == Some(migrated_port) {
                ssh.use_system_proxy = true;
            }
        }
    }
}

fn persist_store(controller: &AppController) -> Result<(), String> {
    let store = controller
        .store
        .lock()
        .map_err(|_| "配置锁已损坏".to_string())?
        .clone();
    let path = config_path(&controller.data_dir);
    let temp_path = path.with_extension("json.tmp");
    let bytes =
        serde_json::to_vec_pretty(&store).map_err(|error| format!("序列化配置失败: {error}"))?;
    let mut file =
        fs::File::create(&temp_path).map_err(|error| format!("写入配置失败: {error}"))?;
    use std::io::Write;
    file.write_all(&bytes)
        .map_err(|error| format!("写入配置失败: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("同步配置失败: {error}"))?;
    fs::rename(&temp_path, &path).map_err(|error| format!("替换配置失败: {error}"))?;
    Ok(())
}

fn status_for(controller: &AppController, id: &str) -> RuntimeSnapshot {
    controller
        .statuses
        .lock()
        .ok()
        .and_then(|statuses| statuses.get(id).cloned())
        .unwrap_or(RuntimeSnapshot {
            state: ServiceState::Stopped,
            endpoints: Vec::new(),
            error: None,
        })
}

struct StatusUpdate {
    state: ServiceState,
    endpoints: Vec<Endpoint>,
    error: Option<String>,
    kind: EventKind,
    message: Option<String>,
    fingerprint: Option<String>,
}

fn set_status(controller: &AppController, id: &str, update: StatusUpdate) {
    if let Ok(mut statuses) = controller.statuses.lock() {
        statuses.insert(
            id.to_string(),
            RuntimeSnapshot {
                state: update.state.clone(),
                endpoints: update.endpoints.clone(),
                error: update.error.clone(),
            },
        );
    }
    let _ = controller.events.send(ProxyEvent {
        profile_id: id.to_string(),
        kind: update.kind,
        state: update.state,
        message: update.message,
        fingerprint: update.fingerprint,
        endpoints: update.endpoints,
    });
}

pub(crate) fn emit_log(controller: &AppController, id: &str, message: impl Into<String>) {
    let snapshot = status_for(controller, id);
    let message = message.into();
    let _ = controller.events.send(ProxyEvent {
        profile_id: id.to_string(),
        kind: EventKind::Log,
        state: snapshot.state,
        message: Some(message),
        fingerprint: None,
        endpoints: snapshot.endpoints,
    });
}

pub(crate) fn set_running(
    controller: &AppController,
    id: &str,
    endpoints: Vec<Endpoint>,
    message: Option<String>,
) {
    set_status(
        controller,
        id,
        StatusUpdate {
            state: ServiceState::Running,
            endpoints,
            error: None,
            kind: EventKind::State,
            message,
            fingerprint: None,
        },
    );
}

pub(crate) fn set_reconnecting(controller: &AppController, id: &str, message: String) {
    let endpoints = status_for(controller, id).endpoints;
    set_status(
        controller,
        id,
        StatusUpdate {
            state: ServiceState::Reconnecting,
            endpoints,
            error: None,
            kind: EventKind::State,
            message: Some(message),
            fingerprint: None,
        },
    );
}

pub(crate) fn service_finished(controller: &AppController, id: &str, result: Result<(), String>) {
    let active = controller
        .active
        .lock()
        .ok()
        .and_then(|mut active| active.remove(id));
    if active.is_none() {
        return;
    }
    if let Some(pending) = controller
        .pending_host_keys
        .lock()
        .ok()
        .and_then(|mut pending| pending.remove(id))
    {
        let _ = pending.response.send(false);
    }
    match result {
        Ok(()) => set_status(
            controller,
            id,
            StatusUpdate {
                state: ServiceState::Stopped,
                endpoints: Vec::new(),
                error: None,
                kind: EventKind::State,
                message: None,
                fingerprint: None,
            },
        ),
        Err(error) => set_status(
            controller,
            id,
            StatusUpdate {
                state: ServiceState::Error,
                endpoints: status_for(controller, id).endpoints,
                error: Some(error.clone()),
                kind: EventKind::State,
                message: Some(error),
                fingerprint: None,
            },
        ),
    }
}

pub(crate) fn known_host(controller: &AppController, host: &str, port: u16) -> Option<String> {
    controller
        .store
        .lock()
        .ok()
        .and_then(|store| store.known_hosts.get(&known_host_key(host, port)).cloned())
}

pub(crate) fn save_known_host(
    controller: &AppController,
    host: &str,
    port: u16,
    fingerprint: &str,
) -> Result<(), String> {
    {
        let mut store = controller
            .store
            .lock()
            .map_err(|_| "配置锁已损坏".to_string())?;
        store
            .known_hosts
            .insert(known_host_key(host, port), fingerprint.to_string());
    }
    persist_store(controller)
}

fn known_host_key(host: &str, port: u16) -> String {
    format!("{host}:{port}")
}

fn endpoint(kind: &str, port: u16, description: impl Into<String>) -> Endpoint {
    Endpoint {
        kind: kind.to_string(),
        host: "127.0.0.1".to_string(),
        port,
        description: description.into(),
    }
}

pub(crate) fn http_endpoint(port: u16) -> Endpoint {
    endpoint("http", port, format!("HTTP http://127.0.0.1:{port}"))
}

pub(crate) fn forward_endpoint(port: u16, remote_host: &str, remote_port: u16) -> Endpoint {
    endpoint(
        "sshForward",
        port,
        format!(
            "SSH 127.0.0.1:{port} → {}",
            format_forward_target(remote_host, remote_port)
        ),
    )
}

fn format_forward_target(remote_host: &str, remote_port: u16) -> String {
    if remote_host.contains(':') && !remote_host.starts_with('[') {
        format!("[{remote_host}]:{remote_port}")
    } else {
        format!("{remote_host}:{remote_port}")
    }
}

pub(crate) fn list_profiles() -> Result<Vec<ProfileSummary>, String> {
    let controller = controller();
    let store = controller
        .store
        .lock()
        .map_err(|_| "配置锁已损坏".to_string())?
        .clone();
    Ok(store
        .profiles
        .iter()
        .map(|profile| summary_for(&controller, profile))
        .collect())
}

pub(crate) fn get_profile(id: &str) -> Result<ProfileView, String> {
    let controller = controller();
    let profile = find_profile(&controller, id)?;
    Ok(view_for(&controller, &profile))
}

pub(crate) fn get_settings() -> Result<AppSettingsView, String> {
    let controller = controller();
    let settings = controller
        .store
        .lock()
        .map_err(|_| "配置锁已损坏".to_string())?
        .settings
        .clone();
    Ok(settings_view(&settings))
}

pub(crate) fn save_settings(settings: AppSettingsDraft) -> Result<AppSettingsView, String> {
    let controller = controller();
    let stored = StoredAppSettings {
        http_proxy: normalize_proxy_endpoint(settings.http_proxy, "HTTP 代理")?,
        socks_proxy: normalize_proxy_endpoint(settings.socks_proxy, "SOCKS5 代理")?,
    };
    {
        let mut store = controller
            .store
            .lock()
            .map_err(|_| "配置锁已损坏".to_string())?;
        store.settings = stored.clone();
    }
    persist_store(&controller)?;
    Ok(settings_view(&stored))
}

fn settings_view(settings: &StoredAppSettings) -> AppSettingsView {
    AppSettingsView {
        http_proxy: settings.http_proxy.as_ref().map(proxy_endpoint_view),
        socks_proxy: settings.socks_proxy.as_ref().map(proxy_endpoint_view),
    }
}

fn proxy_endpoint_view(endpoint: &StoredProxyEndpoint) -> ProxyEndpoint {
    ProxyEndpoint {
        host: endpoint.host.clone(),
        port: endpoint.port,
    }
}

fn normalize_proxy_endpoint(
    endpoint: Option<ProxyEndpoint>,
    label: &str,
) -> Result<Option<StoredProxyEndpoint>, String> {
    let Some(endpoint) = endpoint else {
        return Ok(None);
    };
    let raw_host = endpoint.host.trim();
    let host = raw_host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(raw_host);
    if host.is_empty() {
        return Err(format!("{label}主机不能为空"));
    }
    if host.chars().any(char::is_whitespace)
        || host.contains('/')
        || host.contains('?')
        || host.contains('#')
        || host.contains('@')
    {
        return Err(format!("{label}主机格式无效，请填写主机名或 IP 地址"));
    }
    if endpoint.port == 0 {
        return Err(format!("{label}端口必须大于 0"));
    }
    Ok(Some(StoredProxyEndpoint {
        host: host.to_string(),
        port: endpoint.port,
    }))
}

pub(crate) fn save_profile(draft: ProfileDraft) -> Result<ProfileView, String> {
    let controller = controller();
    let id = draft
        .id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    if is_active(&controller, &id) {
        return Err("运行中的代理不能修改，请先停止".to_string());
    }

    let previous = find_profile_optional(&controller, &id);
    let stored = build_stored_profile(&controller, &id, draft, previous.as_ref())?;
    let type_changed = previous
        .as_ref()
        .is_some_and(|previous| previous.kind != stored.kind);
    {
        let mut store = controller
            .store
            .lock()
            .map_err(|_| "配置锁已损坏".to_string())?;
        if let Some(existing) = store.profiles.iter_mut().find(|profile| profile.id == id) {
            *existing = stored.clone();
        } else {
            store.profiles.push(stored.clone());
        }
    }
    persist_store(&controller)?;
    if type_changed {
        if let Some(previous) = previous.as_ref() {
            controller.secrets.delete_profile(previous);
        }
    }
    Ok(view_for(&controller, &stored))
}

pub(crate) fn delete_profile(id: &str) -> Result<(), String> {
    let controller = controller();
    if is_active(&controller, id) {
        return Err("运行中的代理不能删除，请先停止".to_string());
    }
    let removed = {
        let mut store = controller
            .store
            .lock()
            .map_err(|_| "配置锁已损坏".to_string())?;
        let index = store
            .profiles
            .iter()
            .position(|profile| profile.id == id)
            .ok_or_else(|| "代理配置不存在".to_string())?;
        store.profiles.remove(index)
    };
    controller.secrets.delete_profile(&removed);
    persist_store(&controller)
}

pub(crate) fn start_profile(id: &str) -> Result<StartOutcome, String> {
    let controller = controller();
    let profile = find_profile(&controller, id)?;
    let settings = controller
        .store
        .lock()
        .map_err(|_| "配置锁已损坏".to_string())?
        .settings
        .clone();
    if let Ok(active) = controller.active.lock() {
        if active.contains_key(id) {
            return Ok(StartOutcome::AlreadyRunning {
                endpoints: status_for(&controller, id).endpoints,
            });
        }
    }
    validate_profile_proxy(&profile, &settings)?;

    let (stop_tx, stop_rx) = oneshot::channel();
    controller
        .active
        .lock()
        .map_err(|_| "运行状态锁已损坏".to_string())?
        .insert(id.to_string(), ActiveService { stop: stop_tx });
    set_status(
        &controller,
        id,
        StatusUpdate {
            state: ServiceState::Starting,
            endpoints: Vec::new(),
            error: None,
            kind: EventKind::State,
            message: None,
            fingerprint: None,
        },
    );

    let weak = Arc::downgrade(&controller);
    match &profile.kind {
        ProxyType::Http => {
            let Some(http) = profile.http else {
                remove_active(&controller, id);
                return Err("HTTP 配置不完整".to_string());
            };
            runtime().spawn(async move {
                let result =
                    http_proxy::run(weak.clone(), profile.id.clone(), http, settings, stop_rx)
                        .await;
                if let Some(controller) = weak.upgrade() {
                    service_finished(&controller, &profile.id, result);
                }
            });
        }
        ProxyType::Ssh => {
            let Some(ssh) = profile.ssh else {
                remove_active(&controller, id);
                return Err("SSH 配置不完整".to_string());
            };
            runtime().spawn(async move {
                let result =
                    ssh_proxy::run(weak.clone(), profile.id.clone(), ssh, settings, stop_rx).await;
                if let Some(controller) = weak.upgrade() {
                    service_finished(&controller, &profile.id, result);
                }
            });
        }
    }
    Ok(StartOutcome::Started)
}

fn validate_profile_proxy(
    profile: &StoredProfile,
    settings: &StoredAppSettings,
) -> Result<(), String> {
    match &profile.kind {
        ProxyType::Http => {
            if profile
                .http
                .as_ref()
                .is_some_and(|http| http.use_system_proxy)
                && settings.http_proxy.is_none()
            {
                return Err(
                    "该 HTTP 配置已启用系统代理，请先在应用设置中配置 HTTP 代理".to_string()
                );
            }
        }
        ProxyType::Ssh => {
            if profile.ssh.as_ref().is_some_and(|ssh| ssh.use_system_proxy)
                && settings.socks_proxy.is_none()
            {
                return Err(
                    "该 SSH 配置已启用系统代理，请先在应用设置中配置 SOCKS5 代理".to_string(),
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn stop_profile(id: &str) -> Result<(), String> {
    let controller = controller();
    let active = controller
        .active
        .lock()
        .map_err(|_| "运行状态锁已损坏".to_string())?
        .remove(id);
    if let Some(active) = active {
        let _ = active.stop.send(());
        let pending = controller
            .pending_host_keys
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(id));
        if let Some(pending) = pending {
            let _ = pending.response.send(false);
        }
        set_status(
            &controller,
            id,
            StatusUpdate {
                state: ServiceState::Stopped,
                endpoints: Vec::new(),
                error: None,
                kind: EventKind::State,
                message: None,
                fingerprint: None,
            },
        );
    }
    Ok(())
}

pub(crate) fn confirm_host_key(id: &str, fingerprint: &str, accepted: bool) -> Result<(), String> {
    let controller = controller();
    let pending = controller
        .pending_host_keys
        .lock()
        .map_err(|_| "主机密钥状态锁已损坏".to_string())?
        .remove(id)
        .ok_or_else(|| "当前没有等待确认的 SSH 主机密钥".to_string())?;
    if pending.fingerprint != fingerprint {
        let _ = pending.response.send(false);
        return Err("主机密钥指纹已变化，请重新发起连接".to_string());
    }
    if accepted {
        let profile = match find_profile(&controller, id) {
            Ok(profile) => profile,
            Err(error) => {
                let _ = pending.response.send(false);
                return Err(error);
            }
        };
        let Some(ssh) = profile.ssh else {
            let _ = pending.response.send(false);
            return Err("SSH 配置不存在".to_string());
        };
        if let Err(error) = save_known_host(&controller, &ssh.host, ssh.port, fingerprint) {
            let _ = pending.response.send(false);
            return Err(error);
        }
    }
    pending
        .response
        .send(accepted)
        .map_err(|_| "SSH 连接已结束".to_string())
}

pub(crate) fn events(sink: crate::frb_generated::StreamSink<ProxyEvent>) {
    let receiver = controller().events.subscribe();
    runtime().spawn(async move {
        let mut receiver = receiver;
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if sink.add(event).is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

pub(crate) fn upgrade(weak: &Weak<AppController>) -> Result<Arc<AppController>, String> {
    weak.upgrade()
        .ok_or_else(|| "LocalProxy 已关闭".to_string())
}

fn remove_active(controller: &AppController, id: &str) {
    if let Ok(mut active) = controller.active.lock() {
        active.remove(id);
    }
}

fn is_active(controller: &AppController, id: &str) -> bool {
    controller
        .active
        .lock()
        .map(|active| active.contains_key(id))
        .unwrap_or(false)
}

fn find_profile(controller: &AppController, id: &str) -> Result<StoredProfile, String> {
    find_profile_optional(controller, id).ok_or_else(|| "代理配置不存在".to_string())
}

fn find_profile_optional(controller: &AppController, id: &str) -> Option<StoredProfile> {
    controller.store.lock().ok().and_then(|store| {
        store
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
    })
}

fn summary_for(controller: &AppController, profile: &StoredProfile) -> ProfileSummary {
    let snapshot = status_for(controller, &profile.id);
    let endpoints = if snapshot.endpoints.is_empty() {
        configured_endpoints(profile)
    } else {
        snapshot.endpoints
    };
    ProfileSummary {
        id: profile.id.clone(),
        name: profile.name.clone(),
        tags: profile.tags.clone(),
        kind: profile.kind.clone(),
        state: snapshot.state,
        endpoints,
        error: snapshot.error,
    }
}

fn configured_endpoints(profile: &StoredProfile) -> Vec<Endpoint> {
    match (&profile.http, &profile.ssh) {
        (Some(http), _) => vec![http_endpoint(http.listen_port)],
        (_, Some(ssh)) => [
            ssh.forwards
                .iter()
                .map(|forward| {
                    forward_endpoint(
                        forward.local_port,
                        &forward.remote_host,
                        forward.remote_port,
                    )
                })
                .collect::<Vec<_>>(),
            Vec::new(),
        ]
        .concat(),
        (None, None) => Vec::new(),
    }
}

fn view_for(controller: &AppController, profile: &StoredProfile) -> ProfileView {
    let snapshot = status_for(controller, &profile.id);
    ProfileView {
        id: profile.id.clone(),
        name: profile.name.clone(),
        tags: profile.tags.clone(),
        kind: profile.kind.clone(),
        state: snapshot.state,
        endpoints: snapshot.endpoints,
        error: snapshot.error,
        http: profile.http.as_ref().map(|http| HttpView {
            listen_port: http.listen_port,
            upstream_url: http.upstream_url.clone(),
            headers: http
                .headers
                .iter()
                .map(|field| SecretFieldView {
                    name: field.name.clone(),
                    configured: true,
                })
                .collect(),
            cookies: http
                .cookies
                .iter()
                .map(|field| SecretFieldView {
                    name: field.name.clone(),
                    configured: true,
                })
                .collect(),
            use_system_proxy: http.use_system_proxy,
        }),
        ssh: profile.ssh.as_ref().map(|ssh| SshView {
            host: ssh.host.clone(),
            port: ssh.port,
            username: ssh.username.clone(),
            auth_mode: ssh.auth_mode.clone(),
            private_key_path: ssh.private_key_path.clone(),
            password_configured: ssh.password_configured,
            passphrase_configured: ssh.passphrase_configured,
            forwards: ssh
                .forwards
                .iter()
                .map(|forward| ForwardView {
                    local_port: forward.local_port,
                    remote_host: forward.remote_host.clone(),
                    remote_port: forward.remote_port,
                    tag: forward.tag.clone(),
                })
                .collect(),
            use_system_proxy: ssh.use_system_proxy,
        }),
    }
}

fn build_stored_profile(
    controller: &AppController,
    id: &str,
    draft: ProfileDraft,
    previous: Option<&StoredProfile>,
) -> Result<StoredProfile, String> {
    let name = draft.name.trim();
    if name.is_empty() {
        return Err("名称不能为空".to_string());
    }
    if name.len() > 120 {
        return Err("名称不能超过 120 个字符".to_string());
    }
    let tags = normalize_tags(&draft.tags)?;
    match draft.kind {
        ProxyType::Http => {
            let http = draft.http.ok_or_else(|| "HTTP 配置不完整".to_string())?;
            let old_http = previous.and_then(|profile| profile.http.as_ref());
            let stored_http = build_http(controller, id, http, old_http)?;
            Ok(StoredProfile {
                id: id.to_string(),
                name: name.to_string(),
                tags,
                kind: ProxyType::Http,
                http: Some(stored_http),
                ssh: None,
            })
        }
        ProxyType::Ssh => {
            let ssh = draft.ssh.ok_or_else(|| "SSH 配置不完整".to_string())?;
            let old_ssh = previous.and_then(|profile| profile.ssh.as_ref());
            let stored_ssh = build_ssh(controller, id, ssh, old_ssh)?;
            Ok(StoredProfile {
                id: id.to_string(),
                name: name.to_string(),
                tags,
                kind: ProxyType::Ssh,
                http: None,
                ssh: Some(stored_ssh),
            })
        }
    }
}

fn normalize_tags(raw_tags: &[String]) -> Result<Vec<String>, String> {
    let mut tags = Vec::with_capacity(raw_tags.len());
    for raw_tag in raw_tags {
        let tag = raw_tag.trim();
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > 32 {
            return Err("单个标签不能超过 32 个字符".to_string());
        }
        if tags
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(tag))
        {
            continue;
        }
        tags.push(tag.to_string());
    }
    if tags.len() > 8 {
        return Err("最多只能添加 8 个标签".to_string());
    }
    Ok(tags)
}

fn normalize_forward_tag(raw_tag: &str) -> Result<String, String> {
    let tag = raw_tag.trim();
    if tag.chars().count() > 32 {
        return Err("单个转发标签不能超过 32 个字符".to_string());
    }
    Ok(tag.to_string())
}

fn normalize_forward_host(raw_host: &str) -> Result<String, String> {
    let host = raw_host.trim();
    if host.is_empty() {
        return Err("本地转发的远端主机不能为空".to_string());
    }
    if host.len() > 253 {
        return Err("本地转发的远端主机不能超过 253 个字符".to_string());
    }
    if host
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '/' | '\\' | '@'))
    {
        return Err(
            "远端主机必须是域名、IPv4 或 IPv6 地址，不能包含空格、路径或用户名".to_string(),
        );
    }

    let (host, was_bracketed) = if host.starts_with('[') || host.ends_with(']') {
        if !(host.starts_with('[') && host.ends_with(']')) {
            return Err("IPv6 地址的方括号必须成对出现".to_string());
        }
        (&host[1..host.len() - 1], true)
    } else {
        (host, false)
    };
    if host.is_empty() {
        return Err("本地转发的远端主机不能为空".to_string());
    }
    if (was_bracketed || host.contains(':')) && host.parse::<IpAddr>().is_err() {
        return Err("远端主机中的 IPv6 地址格式无效；端口请填写在单独的端口栏".to_string());
    }
    Ok(host.to_string())
}

fn build_http(
    controller: &AppController,
    id: &str,
    draft: HttpDraft,
    old: Option<&StoredHttpProfile>,
) -> Result<StoredHttpProfile, String> {
    let upstream = url::Url::parse(draft.upstream_url.trim())
        .map_err(|error| format!("上游 URL 无效: {error}"))?;
    if !matches!(upstream.scheme(), "http" | "https") || upstream.host_str().is_none() {
        return Err("上游 URL 必须是包含主机的 http 或 https URL".to_string());
    }
    if upstream.username() != "" || upstream.password().is_some() || upstream.query().is_some() {
        return Err("上游 URL 不能包含用户名、密码或 query".to_string());
    }
    if upstream.fragment().is_some() {
        return Err("上游 URL 不能包含 fragment".to_string());
    }
    validate_port(draft.listen_port, "HTTP 监听端口")?;
    let headers = apply_secret_collection(
        controller,
        id,
        "http-header",
        &draft.headers,
        old.map(|old| old.headers.as_slice()).unwrap_or_default(),
        "header",
    )?;
    let cookies = apply_secret_collection(
        controller,
        id,
        "http-cookie",
        &draft.cookies,
        old.map(|old| old.cookies.as_slice()).unwrap_or_default(),
        "cookie",
    )?;
    Ok(StoredHttpProfile {
        listen_port: draft.listen_port,
        upstream_url: draft.upstream_url.trim_end_matches('/').to_string(),
        headers,
        cookies,
        use_system_proxy: draft.use_system_proxy,
    })
}

fn build_ssh(
    controller: &AppController,
    id: &str,
    draft: SshDraft,
    old: Option<&StoredSshProfile>,
) -> Result<StoredSshProfile, String> {
    let host = draft.host.trim();
    if host.is_empty() {
        return Err("SSH 主机不能为空".to_string());
    }
    if draft.username.trim().is_empty() {
        return Err("SSH 用户名不能为空".to_string());
    }
    if draft.port == 0 {
        return Err("SSH 远端端口必须大于 0".to_string());
    }
    let private_key_path = draft.private_key_path.trim();
    if matches!(draft.auth_mode, SshAuthMode::PrivateKey) && private_key_path.is_empty() {
        return Err("私钥认证需要填写私钥路径".to_string());
    }
    let mut forwards = Vec::with_capacity(draft.forwards.len());
    for forward in draft.forwards {
        validate_port(forward.local_port, "本地转发端口")?;
        let remote_host = normalize_forward_host(&forward.remote_host)?;
        if forward.remote_port == 0 {
            return Err("本地转发的远端端口必须大于 0".to_string());
        }
        forwards.push(StoredForward {
            local_port: forward.local_port,
            remote_host,
            remote_port: forward.remote_port,
            tag: normalize_forward_tag(&forward.tag)?,
        });
    }
    let mut local_ports = HashSet::new();
    for forward in &forwards {
        if forward.local_port != 0 && !local_ports.insert(forward.local_port) {
            return Err("SSH 本地监听端口不能重复".to_string());
        }
    }
    let old_password = old.map(|old| old.password_configured).unwrap_or(false);
    let old_passphrase = old.map(|old| old.passphrase_configured).unwrap_or(false);
    let password_configured = if matches!(draft.auth_mode, SshAuthMode::Password) {
        let configured = apply_fixed_secret(
            controller,
            id,
            "ssh",
            "password",
            &draft.password,
            old_password,
        )?;
        if !configured {
            return Err("密码认证需要填写密码".to_string());
        }
        configured
    } else {
        controller.secrets.delete(id, "ssh", "password");
        false
    };
    let passphrase_configured = if matches!(draft.auth_mode, SshAuthMode::PrivateKey) {
        apply_fixed_secret(
            controller,
            id,
            "ssh",
            "passphrase",
            &draft.passphrase,
            old_passphrase,
        )?
    } else {
        controller.secrets.delete(id, "ssh", "passphrase");
        false
    };
    Ok(StoredSshProfile {
        host: host.to_string(),
        port: draft.port,
        username: draft.username.trim().to_string(),
        auth_mode: draft.auth_mode,
        private_key_path: private_key_path.to_string(),
        password_configured,
        passphrase_configured,
        forwards,
        use_system_proxy: draft.use_system_proxy,
        upstream_socks_port: None,
    })
}

fn apply_fixed_secret(
    controller: &AppController,
    id: &str,
    category: &str,
    name: &str,
    patch: &SecretPatch,
    old_configured: bool,
) -> Result<bool, String> {
    match patch.kind {
        SecretPatchKind::Keep => Ok(old_configured),
        SecretPatchKind::Set => {
            let value = patch
                .value
                .as_deref()
                .ok_or_else(|| format!("{name} 密钥值不能为空"))?;
            controller.secrets.set(id, category, name, value)?;
            Ok(true)
        }
        SecretPatchKind::Clear => {
            controller.secrets.delete(id, category, name);
            Ok(false)
        }
    }
}

fn apply_secret_collection(
    controller: &AppController,
    id: &str,
    category: &str,
    patches: &[SecretPatch],
    old: &[StoredSecretField],
    kind_name: &str,
) -> Result<Vec<StoredSecretField>, String> {
    let mut fields = old.to_vec();
    let mut patched_names = Vec::new();
    let is_header = kind_name == "header";
    for patch in patches {
        let name = patch.name.trim();
        if name.is_empty() {
            return Err(format!("{kind_name} 名称不能为空"));
        }
        if is_header {
            let header_name = http::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| format!("header 名称无效: {name}"))?;
            if is_protected_header(&header_name) {
                return Err(format!(
                    "header 不能覆盖受保护的 hop-by-hop/Host 字段: {name}"
                ));
            }
        }
        if kind_name == "cookie" && !is_cookie_name(name) {
            return Err(format!("cookie 名称无效: {name}"));
        }
        if patched_names
            .iter()
            .any(|patched: &&str| same_field_name(patched, name, is_header))
        {
            return Err(format!("{kind_name} 名称不能重复: {name}"));
        }
        patched_names.push(name);
        let existing_index = fields
            .iter()
            .position(|field| same_field_name(&field.name, name, is_header));
        match patch.kind {
            SecretPatchKind::Keep => {
                if existing_index.is_none() {
                    return Err(format!("{name} 没有可保留的旧密钥"));
                }
            }
            SecretPatchKind::Set => {
                let value = patch
                    .value
                    .as_deref()
                    .ok_or_else(|| format!("{name} 的密钥值不能为空"))?;
                if kind_name == "header" {
                    http::HeaderValue::from_str(value)
                        .map_err(|_| format!("header 值无效: {name}"))?;
                }
                let storage_name = existing_index
                    .and_then(|index| fields.get(index).map(|field| field.name.as_str()))
                    .unwrap_or(name);
                controller.secrets.set(id, category, storage_name, value)?;
                if existing_index.is_none() {
                    fields.push(StoredSecretField {
                        name: name.to_string(),
                    });
                }
            }
            SecretPatchKind::Clear => {
                if let Some(index) = existing_index {
                    let old_name = fields[index].name.clone();
                    controller.secrets.delete(id, category, &old_name);
                    fields.remove(index);
                } else {
                    controller.secrets.delete(id, category, name);
                }
            }
        }
    }
    Ok(fields)
}

fn is_cookie_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(
                    character,
                    '!' | '#'
                        | '$'
                        | '%'
                        | '&'
                        | '\''
                        | '*'
                        | '+'
                        | '-'
                        | '.'
                        | '^'
                        | '_'
                        | '`'
                        | '|'
                        | '~'
                )
        })
}

fn same_field_name(left: &str, right: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn is_protected_header(name: &http::header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

fn validate_port(port: u16, label: &str) -> Result<(), String> {
    let _ = (port, label);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        format_forward_target, normalize_forward_host, normalize_forward_tag, normalize_tags,
    };

    #[test]
    fn normalizes_and_deduplicates_tags() {
        let tags = normalize_tags(&[
            "  production ".to_string(),
            "API".to_string(),
            "api".to_string(),
            "".to_string(),
        ])
        .unwrap();
        assert_eq!(tags, vec!["production".to_string(), "API".to_string()]);
    }

    #[test]
    fn rejects_too_many_tags() {
        let tags = (0..9)
            .map(|index| format!("tag-{index}"))
            .collect::<Vec<_>>();
        assert!(normalize_tags(&tags).is_err());
    }

    #[test]
    fn trims_forward_tag() {
        assert_eq!(normalize_forward_tag(" Redis ").unwrap(), "Redis");
        assert!(normalize_forward_tag(&"x".repeat(33)).is_err());
    }

    #[test]
    fn accepts_dns_names_and_ipv6_forward_hosts() {
        let hostname = "db-readonly.example.com";
        assert_eq!(
            normalize_forward_host(&format!(" {hostname} ")).unwrap(),
            hostname
        );
        assert_eq!(normalize_forward_host("192.0.2.10").unwrap(), "192.0.2.10");
        assert_eq!(
            normalize_forward_host("[2001:db8::8]").unwrap(),
            "2001:db8::8"
        );
        assert_eq!(
            format_forward_target("2001:db8::8", 3306),
            "[2001:db8::8]:3306"
        );
    }

    #[test]
    fn rejects_forward_hosts_with_embedded_port_or_invalid_ipv6() {
        assert!(normalize_forward_host("db.example.com:3306").is_err());
        assert!(normalize_forward_host("[db.example.com]").is_err());
        assert!(normalize_forward_host("2001:db8::not-an-ip").is_err());
        assert!(normalize_forward_host("db example.com").is_err());
    }
}
