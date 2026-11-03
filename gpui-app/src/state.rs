//! Shared UI state types and formatting helpers.
//!
//! Everything in this module is presentation-only: the networking core owns
//! the real configuration and runtime state. These helpers mirror the
//! formatting rules of the original Electron UI so both the layout and the
//! wording stay familiar.

use std::time::Instant;

use local_proxy_rust::api::local_proxy::{
    Endpoint, ProfileSummary, ProfileView, ProxyEndpoint, ProxyType, SecretPatch, SecretPatchKind,
    ServiceState, SshView, WireGuardView,
};

use crate::i18n::{t, tp, Lang};

// ---------------------------------------------------------------------------
// Small UI models
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastTone {
    Success,
    Info,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub tone: ToastTone,
    pub expires_at: Instant,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub time: String,
    pub state: ServiceState,
    pub message: String,
}

/// Host-key confirmation requested by an SSH profile.
#[derive(Clone, Debug)]
pub struct HostKeyPrompt {
    pub profile_id: String,
    pub fingerprint: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Status formatting
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusTone {
    Running,
    Pending,
    Error,
    Stopped,
}

impl StatusTone {
    pub fn from_state(state: &ServiceState) -> Self {
        match state {
            ServiceState::Running => StatusTone::Running,
            ServiceState::Starting | ServiceState::AwaitingHostKey | ServiceState::Reconnecting => {
                StatusTone::Pending
            }
            ServiceState::Error => StatusTone::Error,
            ServiceState::Stopped => StatusTone::Stopped,
        }
    }
}

pub fn is_active(state: &ServiceState) -> bool {
    matches!(
        state,
        ServiceState::Running
            | ServiceState::Starting
            | ServiceState::AwaitingHostKey
            | ServiceState::Reconnecting
    )
}

pub fn state_label(lang: Lang, state: &ServiceState) -> String {
    let key = match state {
        ServiceState::Running => "state.running",
        ServiceState::Starting => "state.starting",
        ServiceState::AwaitingHostKey => "state.awaiting",
        ServiceState::Reconnecting => "state.reconnecting",
        ServiceState::Error => "state.error",
        ServiceState::Stopped => "state.stopped",
    };
    t(lang, key)
}

pub fn kind_label(lang: Lang, kind: &ProxyType) -> String {
    let key = match kind {
        ProxyType::Http => "kind.http",
        ProxyType::Ssh => "kind.ssh",
        ProxyType::Wstunnel => "kind.wstunnel",
        ProxyType::WireGuard => "kind.wireguard",
    };
    t(lang, key)
}

// ---------------------------------------------------------------------------
// Endpoint formatting
// ---------------------------------------------------------------------------

pub fn format_address(host: &str, port: u16) -> String {
    format_address_lang(host, port, Lang::En)
}

pub fn format_address_lang(host: &str, port: u16, lang: Lang) -> String {
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    if port == 0 {
        format!("{host}:{}", t(lang, "common.randomPort"))
    } else {
        format!("{host}:{port}")
    }
}

pub fn format_bytes(value: u64) -> String {
    if value < 1024 {
        format!("{value} B")
    } else if value < 1024 * 1024 {
        format!("{:.1} KiB", value as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", value as f64 / (1024.0 * 1024.0))
    }
}

/// Derive display endpoints for a profile, exactly like the Electron UI did.
pub fn endpoint_for_profile(profile: &ProfileView, lang: Lang) -> Vec<Endpoint> {
    if !profile.endpoints.is_empty() {
        return profile.endpoints.clone();
    }
    if let Some(wstunnel) = &profile.wstunnel {
        return wstunnel
            .forwards
            .iter()
            .map(|forward| Endpoint {
                kind: forward.protocol.clone(),
                host: "127.0.0.1".into(),
                port: forward.local_port,
                description: format!(
                    "{} → {}",
                    forward.protocol.to_uppercase(),
                    format_address_lang(&forward.remote_host, forward.remote_port, lang)
                ),
            })
            .collect();
    }
    if let Some(wireguard) = &profile.wireguard {
        return vec![Endpoint {
            kind: "wireguard".into(),
            host: "utun".into(),
            port: 0,
            description: wireguard
                .peers
                .iter()
                .map(|peer| format_address(&peer.endpoint_host, peer.endpoint_port))
                .collect::<Vec<_>>()
                .join(", "),
        }];
    }
    if let Some(http) = &profile.http {
        return vec![Endpoint {
            kind: "http".into(),
            host: "127.0.0.1".into(),
            port: http.listen_port,
            description: t(lang, "app.localHttpProxy"),
        }];
    }
    profile
        .ssh
        .as_ref()
        .map(|ssh| {
            ssh.forwards
                .iter()
                .map(|forward| Endpoint {
                    kind: "sshForward".into(),
                    host: "127.0.0.1".into(),
                    port: forward.local_port,
                    description: format!(
                        "SSH → {}",
                        format_address_lang(&forward.remote_host, forward.remote_port, lang)
                    ),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn endpoint_for_summary(profile: &ProfileSummary, lang: Lang) -> String {
    match profile.endpoints.first() {
        Some(endpoint) => format_address_lang(&endpoint.host, endpoint.port, lang),
        None => t(lang, "app.localAddressAfterStart"),
    }
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

pub fn now_time() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

// ---------------------------------------------------------------------------
// Secret / header parsing helpers
// ---------------------------------------------------------------------------

/// Rows edited in the HTTP header/cookie editors.
#[derive(Clone, Debug)]
pub struct SecretRowDraft {
    pub name: String,
    pub value: String,
    pub configured: bool,
    pub original_name: Option<String>,
}

pub fn parse_batch_pairs(text: &str) -> Vec<(String, String)> {
    let clean: Vec<String> = text
        .replace("**", "")
        .replace('`', "")
        .replace('\r', "")
        .split('\n')
        .map(|line| {
            line.trim()
                .trim_start_matches("- ")
                .trim_start_matches("* ")
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect();

    let mut pairs = Vec::new();
    for line in &clean {
        if let Some(separator) = line.find(':') {
            let name = line[..separator].trim();
            let value = line[separator + 1..].trim();
            if !name.is_empty()
                && !value.is_empty()
                && !line.to_ascii_lowercase().starts_with("http")
            {
                pairs.push((name.to_string(), value.to_string()));
            }
        }
    }
    if !pairs.is_empty() {
        return pairs;
    }

    let mut index = 0;
    while index + 1 < clean.len() {
        let name = clean[index].trim();
        let value = clean[index + 1]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        if !name.is_empty() && !value.is_empty() {
            pairs.push((name.to_string(), value));
        }
        index += 2;
    }
    if !pairs.is_empty() {
        return pairs;
    }

    text.split(';')
        .filter_map(|item| {
            let item = item.trim();
            item.find('=').map(|separator| {
                (
                    item[..separator].trim().to_string(),
                    item[separator + 1..].trim().to_string(),
                )
            })
        })
        .filter(|(name, _)| !name.is_empty())
        .collect()
}

/// Convert edited rows plus explicit removals into `SecretPatch` operations.
///
/// Preserves the original semantics: configured fields are kept when the value
/// is left blank, renames emit a clear for the old name, and rows that were
/// removed become `Clear` patches.
pub fn build_secret_patches(
    rows: &[SecretRowDraft],
    removed: &[String],
    label: &str,
    lang: Lang,
) -> Result<Vec<SecretPatch>, String> {
    let mut patches = Vec::new();
    for row in rows {
        let name = row.name.trim();
        let value = row.value.as_str();
        if name.is_empty() && value.is_empty() {
            continue;
        }
        if name.is_empty() {
            return Err(tp(lang, "common.labelNameEmpty", &[("label", label)]));
        }
        if !row.configured && value.is_empty() {
            return Err(tp(lang, "common.nameRequiredValue", &[("name", name)]));
        }
        if let Some(original) = &row.original_name {
            if !original.is_empty() && original != name {
                if row.configured && value.is_empty() {
                    return Err(tp(lang, "common.nameRequiredValue", &[("name", name)]));
                }
                patches.push(SecretPatch {
                    name: original.clone(),
                    value: None,
                    kind: SecretPatchKind::Clear,
                });
            }
        }
        let kind = if row.configured && value.is_empty() {
            SecretPatchKind::Keep
        } else {
            SecretPatchKind::Set
        };
        patches.push(SecretPatch {
            name: name.to_string(),
            value: if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            },
            kind,
        });
    }
    for name in removed {
        if rows.iter().any(|row| row.name.trim() == name) {
            continue;
        }
        patches.push(SecretPatch {
            name: name.clone(),
            value: None,
            kind: SecretPatchKind::Clear,
        });
    }
    Ok(patches)
}

// ---------------------------------------------------------------------------
// SSH command / wg-quick reconstruction
// ---------------------------------------------------------------------------

pub fn build_ssh_command(ssh: &SshView, socks_proxy: Option<&ProxyEndpoint>) -> String {
    let proxy = if ssh.use_system_proxy {
        socks_proxy
            .map(|proxy| {
                format!(
                    "-o 'ProxyCommand=connect -S {} %h %p' ",
                    format_address(&proxy.host, proxy.port)
                )
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let forwards = ssh
        .forwards
        .iter()
        .map(|forward| {
            format!(
                "-L {}:{}:{}",
                forward.local_port, forward.remote_host, forward.remote_port
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "ssh -N {}{} -p {} {}@{}",
        proxy, forwards, ssh.port, ssh.username, ssh.host
    )
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
}

pub fn wireguard_config_text(
    wireguard: &WireGuardView,
    lang: Lang,
    private_key: Option<&str>,
) -> String {
    let mut lines: Vec<String> = vec!["[Interface]".into()];
    if let Some(key) = private_key {
        lines.push(format!("PrivateKey = {key}"));
    } else if wireguard.private_key_configured {
        lines.push(t(lang, "editor.wgPrivateKeySavedComment"));
    }
    lines.push(format!("Address = {}", wireguard.addresses.join(", ")));
    if let Some(mtu) = wireguard.mtu {
        lines.push(format!("MTU = {mtu}"));
    }
    if let Some(port) = wireguard.listen_port {
        lines.push(format!("ListenPort = {port}"));
    }
    if !wireguard.dns.is_empty() {
        lines.push(format!("DNS = {}", wireguard.dns.join(", ")));
    }
    for peer in &wireguard.peers {
        lines.push(String::new());
        lines.push("[Peer]".into());
        lines.push(format!("PublicKey = {}", peer.public_key));
        let host = if peer.endpoint_host.contains(':') {
            format!("[{}]", peer.endpoint_host)
        } else {
            peer.endpoint_host.clone()
        };
        lines.push(format!("Endpoint = {host}:{}", peer.endpoint_port));
        lines.push(format!("AllowedIPs = {}", peer.allowed_ips.join(", ")));
        if let Some(keepalive) = peer.persistent_keepalive {
            lines.push(format!("PersistentKeepalive = {keepalive}"));
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Misc display helpers
// ---------------------------------------------------------------------------

/// Compact summary of an app-level proxy endpoint.
pub fn proxy_label(proxy: Option<&ProxyEndpoint>, lang: Lang) -> String {
    match proxy {
        Some(proxy) => format_address_lang(&proxy.host, proxy.port, lang),
        None => t(lang, "common.notConfigured"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(name: &str, value: &str, configured: bool, original: Option<&str>) -> SecretRowDraft {
        SecretRowDraft {
            name: name.into(),
            value: value.into(),
            configured,
            original_name: original.map(str::to_string),
        }
    }

    #[test]
    fn secret_patches_keep_existing_values_and_remove_deleted_rows() {
        let rows = vec![
            secret("Authorization", "", true, Some("Authorization")),
            secret("X-New", "fixture", false, None),
        ];
        let patches = build_secret_patches(&rows, &["X-Old".into()], "Header", Lang::En).unwrap();
        assert_eq!(
            patches
                .iter()
                .map(|p| (&p.name, &p.kind))
                .collect::<Vec<_>>(),
            vec![
                (&"Authorization".to_string(), &SecretPatchKind::Keep),
                (&"X-New".to_string(), &SecretPatchKind::Set),
                (&"X-Old".to_string(), &SecretPatchKind::Clear),
            ]
        );
    }

    #[test]
    fn replacing_a_deleted_secret_does_not_clear_the_replacement() {
        let rows = vec![secret("X-Test", "new value", false, None)];
        let patches = build_secret_patches(&rows, &["X-Test".into()], "Header", Lang::En).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].kind, SecretPatchKind::Set);
    }

    #[test]
    fn renaming_a_saved_secret_requires_a_replacement_value() {
        let rows = vec![secret("New-Name", "", true, Some("Old-Name"))];
        assert!(build_secret_patches(&rows, &[], "Header", Lang::En).is_err());
    }

    #[test]
    fn batch_import_handles_headers_browser_pairs_and_cookies() {
        assert_eq!(
            parse_batch_pairs("Accept: text/plain\nX-Test: one"),
            vec![
                ("Accept".into(), "text/plain".into()),
                ("X-Test".into(), "one".into())
            ]
        );
        assert_eq!(
            parse_batch_pairs("Accept\ntext/plain\nX-Test\none"),
            vec![
                ("Accept".into(), "text/plain".into()),
                ("X-Test".into(), "one".into())
            ]
        );
        assert_eq!(
            parse_batch_pairs("one=1; two=2"),
            vec![("one".into(), "1".into()), ("two".into(), "2".into())]
        );
    }
}
