use serde::{Deserialize, Serialize};

use crate::core;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProxyType {
    Http,
    Ssh,
    Wstunnel,
    WireGuard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceState {
    Stopped,
    Starting,
    AwaitingHostKey,
    Running,
    Reconnecting,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecretPatchKind {
    Keep,
    Set,
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretPatch {
    pub name: String,
    pub value: Option<String>,
    pub kind: SecretPatchKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SshAuthMode {
    Password,
    PrivateKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardDraft {
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpDraft {
    pub listen_port: u16,
    pub upstream_url: String,
    pub headers: Vec<SecretPatch>,
    pub cookies: Vec<SecretPatch>,
    pub use_system_proxy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshDraft {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_mode: SshAuthMode,
    pub private_key_path: String,
    pub password: SecretPatch,
    pub passphrase: SecretPatch,
    pub forwards: Vec<ForwardDraft>,
    pub use_system_proxy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WstunnelForward {
    pub protocol: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub timeout_sec: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WstunnelConfig {
    pub server_url: String,
    pub http_upgrade_path_prefix: String,
    pub forwards: Vec<WstunnelForward>,
    #[serde(default)]
    pub tls_verify_certificate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireGuardPeer {
    pub public_key: String,
    pub endpoint_host: String,
    pub endpoint_port: u16,
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub persistent_keepalive: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireGuardConfig {
    pub addresses: Vec<String>,
    #[serde(default)]
    pub dns: Vec<String>,
    #[serde(default)]
    pub mtu: Option<u16>,
    #[serde(default)]
    pub listen_port: Option<u16>,
    pub private_key: SecretPatch,
    pub peers: Vec<WireGuardPeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireGuardStatus {
    pub interface_name: String,
    pub listen_port: u16,
    pub public_key: Option<String>,
    pub peers: Vec<WireGuardPeerStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireGuardPeerStatus {
    pub public_key: String,
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<String>,
    pub persistent_keepalive: Option<u16>,
    pub handshake_age_sec: Option<u64>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireGuardSetupResult {
    pub interface_name: String,
    pub listen_port: u16,
    pub public_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireGuardView {
    pub addresses: Vec<String>,
    pub dns: Vec<String>,
    pub mtu: Option<u16>,
    pub listen_port: Option<u16>,
    pub private_key_configured: bool,
    pub peers: Vec<WireGuardPeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileDraft {
    pub id: Option<String>,
    pub name: String,
    pub tags: Vec<String>,
    pub kind: ProxyType,
    pub http: Option<HttpDraft>,
    pub ssh: Option<SshDraft>,
    pub wstunnel: Option<WstunnelConfig>,
    #[serde(default)]
    pub wireguard: Option<WireGuardConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretFieldView {
    pub name: String,
    pub configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpView {
    pub listen_port: u16,
    pub upstream_url: String,
    pub headers: Vec<SecretFieldView>,
    pub cookies: Vec<SecretFieldView>,
    pub use_system_proxy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardView {
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshView {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_mode: SshAuthMode,
    pub private_key_path: String,
    pub password_configured: bool,
    pub passphrase_configured: bool,
    pub forwards: Vec<ForwardView>,
    pub use_system_proxy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettingsDraft {
    pub http_proxy: Option<ProxyEndpoint>,
    pub socks_proxy: Option<ProxyEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettingsView {
    pub http_proxy: Option<ProxyEndpoint>,
    pub socks_proxy: Option<ProxyEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub kind: String,
    pub host: String,
    pub port: u16,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub kind: ProxyType,
    pub state: ServiceState,
    pub endpoints: Vec<Endpoint>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileView {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub kind: ProxyType,
    pub state: ServiceState,
    pub endpoints: Vec<Endpoint>,
    pub error: Option<String>,
    pub http: Option<HttpView>,
    pub ssh: Option<SshView>,
    pub wstunnel: Option<WstunnelConfig>,
    pub wireguard: Option<WireGuardView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StartOutcome {
    Started,
    AlreadyRunning { endpoints: Vec<Endpoint> },
}

pub fn parse_wg_config(config: String) -> Result<WireGuardConfig, String> {
    core::wireguard::parse_config(&config)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    State,
    HostKey,
    Log,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyEvent {
    pub profile_id: String,
    pub kind: EventKind,
    pub state: ServiceState,
    pub message: Option<String>,
    pub fingerprint: Option<String>,
    pub endpoints: Vec<Endpoint>,
}

pub fn init_app() {
    core::init();
}

pub fn list_profiles() -> Result<Vec<ProfileSummary>, String> {
    core::list_profiles()
}

pub fn get_profile(id: String) -> Result<ProfileView, String> {
    core::get_profile(&id)
}

pub fn get_settings() -> Result<AppSettingsView, String> {
    core::get_settings()
}

pub fn save_settings(settings: AppSettingsDraft) -> Result<AppSettingsView, String> {
    core::save_settings(settings)
}

pub fn save_profile(draft: ProfileDraft) -> Result<ProfileView, String> {
    core::save_profile(draft)
}

pub fn delete_profile(id: String) -> Result<(), String> {
    core::delete_profile(&id)
}

pub fn start_profile(id: String) -> Result<StartOutcome, String> {
    core::start_profile(&id)
}

pub fn stop_profile(id: String) -> Result<(), String> {
    core::stop_profile(&id)
}

pub fn confirm_host_key(id: String, fingerprint: String, accepted: bool) -> Result<(), String> {
    core::confirm_host_key(&id, &fingerprint, accepted)
}

pub fn subscribe_events() -> tokio::sync::broadcast::Receiver<ProxyEvent> {
    core::subscribe_events()
}

pub fn parse_wstunnel_command(command: String) -> Result<WstunnelConfig, String> {
    core::wstunnel::parse_command(&command)
}

pub async fn wireguard_status(id: String) -> Result<Option<WireGuardStatus>, String> {
    core::wireguard_status(&id).await
}
