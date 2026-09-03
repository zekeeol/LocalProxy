use crate::frb_generated::StreamSink;
use serde::{Deserialize, Serialize};

use crate::core;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProxyType {
    Http,
    Ssh,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecretPatchKind {
    Keep,
    Set,
    Clear,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileDraft {
    pub id: Option<String>,
    pub name: String,
    pub tags: Vec<String>,
    pub kind: ProxyType,
    pub http: Option<HttpDraft>,
    pub ssh: Option<SshDraft>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StartOutcome {
    Started,
    AlreadyRunning { endpoints: Vec<Endpoint> },
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

#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    core::init();
}

#[flutter_rust_bridge::frb(sync)]
pub fn list_profiles() -> Result<Vec<ProfileSummary>, String> {
    core::list_profiles()
}

#[flutter_rust_bridge::frb(sync)]
pub fn get_profile(id: String) -> Result<ProfileView, String> {
    core::get_profile(&id)
}

#[flutter_rust_bridge::frb(sync)]
pub fn get_settings() -> Result<AppSettingsView, String> {
    core::get_settings()
}

#[flutter_rust_bridge::frb(sync)]
pub fn save_settings(settings: AppSettingsDraft) -> Result<AppSettingsView, String> {
    core::save_settings(settings)
}

#[flutter_rust_bridge::frb(sync)]
pub fn save_profile(draft: ProfileDraft) -> Result<ProfileView, String> {
    core::save_profile(draft)
}

#[flutter_rust_bridge::frb(sync)]
pub fn delete_profile(id: String) -> Result<(), String> {
    core::delete_profile(&id)
}

#[flutter_rust_bridge::frb(sync)]
pub fn start_profile(id: String) -> Result<StartOutcome, String> {
    core::start_profile(&id)
}

#[flutter_rust_bridge::frb(sync)]
pub fn stop_profile(id: String) -> Result<(), String> {
    core::stop_profile(&id)
}

#[flutter_rust_bridge::frb(sync)]
pub fn confirm_host_key(id: String, fingerprint: String, accepted: bool) -> Result<(), String> {
    core::confirm_host_key(&id, &fingerprint, accepted)
}

#[flutter_rust_bridge::frb(sync)]
pub fn events(sink: StreamSink<ProxyEvent>) {
    core::events(sink);
}
