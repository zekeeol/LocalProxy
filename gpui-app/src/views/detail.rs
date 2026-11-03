//! Profile details and runtime state.
use crate::app::{AppShell, InlineOverlay};
use crate::{state, theme, widgets as w};
use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context, IntoElement};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use local_proxy_rust::api::local_proxy::ProxyType;

pub fn render(shell: &AppShell, cx: &mut Context<AppShell>) -> AnyElement {
    let Some(profile) = shell.selected.as_ref() else {
        return div().into_any_element();
    };
    let active = state::is_active(&profile.state);
    let toggle = if active {
        w::danger_button("toggle-profile", shell.t("common.stop")).icon(IconName::Square)
    } else {
        w::primary_button("toggle-profile", shell.t("common.start")).icon(IconName::Play)
    };
    let mut title = div().flex().flex_col().gap(px(7.)).child(
        div()
            .text_size(px(22.))
            .font_weight(gpui::FontWeight::BOLD)
            .child(profile.name.clone()),
    );
    let mut badges = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(7.))
        .child(w::status_pill(shell.lang, &profile.state))
        .child(w::type_pill(state::kind_label(shell.lang, &profile.kind)));
    for tag in &profile.tags {
        badges = badges.child(w::tag(tag.clone()));
    }
    title = title.child(badges);
    let id = profile.id.clone();
    let name = profile.name.clone();
    let mut actions = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(toggle.on_click(cx.listener(|this, _, _, cx| this.toggle_selected(cx))))
        .child(
            w::outline_button("edit-profile", shell.t("common.edit"))
                .icon(IconName::Pencil)
                .on_click(cx.listener(|this, _, window, cx| this.begin_edit(window, cx))),
        );
    if let Some(ssh) = &profile.ssh {
        let command = state::build_ssh_command(ssh, shell.settings.socks_proxy.as_ref());
        actions = actions.child(
            w::outline_button("copy-ssh", shell.t("detail.copySshCommand"))
                .on_click(cx.listener(move |this, _, _, cx| this.copy_text(command.clone(), cx))),
        );
    }
    actions = actions.child(
        w::icon_button("delete-profile", IconName::Trash)
            .tooltip(shell.t("detail.deleteConfig"))
            .accessibility_label(shell.t("detail.deleteConfig"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.inline_overlay = Some(InlineOverlay::ConfirmDelete {
                    profile_id: id.clone(),
                    name: name.clone(),
                });
                cx.notify();
            })),
    );
    let mut page = div().flex().flex_col().gap(px(16.)).child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .mb(px(6.))
            .child(title)
            .child(actions),
    );
    if let Some(error) = &profile.error {
        page = page.child(w::error_banner(error.clone()));
    }

    let mut endpoints = w::section_card().child(w::section_title(
        IconName::Layers,
        shell.t("detail.endpointsTitle"),
        shell.t("detail.endpointsDesc"),
    ));
    if let Some(http) = &profile.http {
        if http.use_system_proxy {
            endpoints = endpoints.child(
                div()
                    .p(px(10.))
                    .mb(px(10.))
                    .rounded(px(9.))
                    .bg(theme::route_bg())
                    .text_color(theme::teal())
                    .text_size(px(12.))
                    .child(format!(
                        "{} · {}",
                        shell.t("detail.httpProxyRoute"),
                        state::proxy_label(shell.settings.http_proxy.as_ref(), shell.lang)
                    )),
            );
        }
    }
    if let Some(ssh) = &profile.ssh {
        if ssh.use_system_proxy {
            endpoints = endpoints.child(
                div()
                    .p(px(10.))
                    .mb(px(10.))
                    .rounded(px(9.))
                    .bg(theme::route_bg())
                    .text_color(theme::teal())
                    .text_size(px(12.))
                    .child(format!(
                        "{} · SOCKS5 {} → {}@{}",
                        shell.t("route.title"),
                        state::proxy_label(shell.settings.socks_proxy.as_ref(), shell.lang),
                        ssh.username,
                        state::format_address(&ssh.host, ssh.port)
                    )),
            );
        }
    }
    let endpoint_values = shell.selected_endpoints();
    if endpoint_values.is_empty() {
        endpoints = endpoints.child(w::empty_hint(
            shell.t("detail.noEndpoints"),
            shell.t("detail.noEndpointsHint"),
        ));
    }
    for (index, endpoint) in endpoint_values.iter().enumerate() {
        let address = if profile.kind == ProxyType::WireGuard {
            profile
                .wireguard
                .as_ref()
                .map(|wg| wg.addresses.join(", "))
                .unwrap_or_default()
        } else {
            state::format_address_lang(&endpoint.host, endpoint.port, shell.lang)
        };
        let copy = address.clone();
        let display = if profile.kind == ProxyType::Http {
            format!("http://{address}")
        } else {
            address
        };
        let label = profile
            .ssh
            .as_ref()
            .and_then(|ssh| ssh.forwards.get(index))
            .map(|forward| forward.tag.trim())
            .filter(|tag| !tag.is_empty())
            .map(|tag| format!("{} · {tag}", endpoint.description))
            .unwrap_or_else(|| endpoint.description.clone());
        endpoints = endpoints.child(
            div()
                .id(w::element_id("endpoint", index))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .mb(px(8.))
                .p(px(14.))
                .rounded(px(12.))
                .border_1()
                .border_color(theme::endpoint_border())
                .bg(theme::endpoint_bg())
                .cursor_pointer()
                .hover(|s| s.border_color(theme::teal()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(label),
                        )
                        .child(
                            div()
                                .font_family(theme::mono_font())
                                .text_size(px(15.))
                                .text_color(theme::teal())
                                .child(display),
                        ),
                )
                .child(
                    Icon::new(IconName::Copy)
                        .size(px(16.))
                        .text_color(theme::muted()),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.copy_text(copy.clone(), cx))),
        );
    }
    page = page.child(endpoints);
    let mut config = w::section_card().child(w::section_title(
        IconName::Settings,
        shell.t("detail.configTitle"),
        shell.t("detail.configDesc"),
    ));
    if let Some(http) = &profile.http {
        config = config
            .child(w::key_value(
                shell.t("detail.upstreamBaseUrl"),
                http.upstream_url.clone(),
                true,
            ))
            .child(w::key_value(
                shell.t("detail.listenPort"),
                state::format_address_lang("127.0.0.1", http.listen_port, shell.lang),
                true,
            ))
            .child(w::key_value(
                shell.t("detail.authHeaders"),
                shell.tp(
                    "common.itemsHidden",
                    &[("n", &http.headers.len().to_string())],
                ),
                false,
            ))
            .child(w::key_value(
                shell.t("detail.authCookies"),
                shell.tp(
                    "common.itemsHidden",
                    &[("n", &http.cookies.len().to_string())],
                ),
                false,
            ))
            .child(w::key_value(
                shell.t("detail.appHttpProxy"),
                if http.use_system_proxy {
                    state::proxy_label(shell.settings.http_proxy.as_ref(), shell.lang)
                } else {
                    shell.t("common.notEnabled")
                },
                true,
            ));
    }
    if let Some(ssh) = &profile.ssh {
        config = config
            .child(w::key_value(
                shell.t("detail.sshConnection"),
                ssh.connection_name.clone(),
                false,
            ))
            .child(w::key_value(
                shell.t("detail.sshRemote"),
                format!(
                    "{}@{}",
                    ssh.username,
                    state::format_address(&ssh.host, ssh.port)
                ),
                true,
            ))
            .child(w::key_value(
                shell.t("detail.authMode"),
                match ssh.auth_mode {
                    local_proxy_rust::api::local_proxy::SshAuthMode::Password => {
                        shell.t(if ssh.password_configured {
                            "detail.passwordSaved"
                        } else {
                            "detail.passwordNotConfigured"
                        })
                    }
                    _ => shell.t("detail.privateKeyFile"),
                },
                false,
            ))
            .child(w::key_value(
                shell.t("detail.localForwards"),
                shell.tp(
                    "common.rulesCount",
                    &[("n", &ssh.forwards.len().to_string())],
                ),
                false,
            ))
            .child(w::key_value(
                shell.t("detail.appSocks5Proxy"),
                if ssh.use_system_proxy {
                    state::proxy_label(shell.settings.socks_proxy.as_ref(), shell.lang)
                } else {
                    shell.t("common.notEnabled")
                },
                true,
            ));
    }
    if let Some(ws) = &profile.wstunnel {
        config = config
            .child(w::key_value(
                shell.t("detail.wstunnelServer"),
                ws.server_url.clone(),
                true,
            ))
            .child(w::key_value(
                shell.t("detail.upgradePathPrefix"),
                ws.http_upgrade_path_prefix.clone(),
                true,
            ))
            .child(w::key_value(
                shell.t("detail.localForwards"),
                shell.tp(
                    "common.rulesCount",
                    &[("n", &ws.forwards.len().to_string())],
                ),
                false,
            ))
            .child(w::key_value(
                shell.t("detail.tlsVerify"),
                shell.t(if ws.tls_verify_certificate {
                    "common.enabled"
                } else {
                    "detail.tlsOffNote"
                }),
                false,
            ));
    }
    if let Some(wg) = &profile.wireguard {
        config = config
            .child(w::key_value(
                shell.t("detail.interfaceAddresses"),
                wg.addresses.join(", "),
                true,
            ))
            .child(w::key_value(
                shell.t("detail.interfaceMtu"),
                wg.mtu
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| shell.t("detail.systemDefault")),
                true,
            ))
            .child(w::key_value(
                shell.t("detail.listenPort"),
                wg.listen_port
                    .map(|port| port.to_string())
                    .unwrap_or_else(|| shell.t("common.randomPort")),
                true,
            ))
            .child(w::key_value(
                "DNS",
                if wg.dns.is_empty() {
                    shell.t("detail.dnsUnchanged")
                } else {
                    wg.dns.join(", ")
                },
                true,
            ))
            .child(w::key_value(
                shell.t("detail.peerCount"),
                wg.peers.len().to_string(),
                false,
            ));
        if shell.wg_stale {
            config = config.child(w::field_hint(shell.t("detail.statusMayBeStale")));
        }
        if let Some(status) = &shell.wg_status {
            if let Some(warning) = &status.health_warning {
                config = config.child(w::error_banner(
                    shell.tp("detail.carrierMissing", &[("endpoint", &warning.endpoint)]),
                ));
            }
            config = config
                .child(w::key_value(
                    shell.t("detail.actualInterface"),
                    status.interface_name.clone(),
                    true,
                ))
                .child(w::key_value(
                    shell.t("detail.localPublicKey"),
                    status
                        .public_key
                        .clone()
                        .unwrap_or_else(|| shell.t("common.notConnected")),
                    true,
                ));
            for peer in &status.peers {
                let short: String = peer.public_key.chars().take(8).collect();
                let handshake = peer
                    .handshake_age_sec
                    .map(|n| shell.tp("detail.secondsAgo", &[("n", &n.to_string())]))
                    .unwrap_or_else(|| shell.t("detail.handshakeNever"));
                config = config.child(w::key_value(
                    shell.tp("detail.peerKey", &[("key", &short)]),
                    format!(
                        "{} · {} {} · {} {} · {} {}",
                        peer.endpoint
                            .clone()
                            .unwrap_or_else(|| shell.t("common.notConnected")),
                        shell.t("detail.rxLabel"),
                        state::format_bytes(peer.rx_bytes),
                        shell.t("detail.txLabel"),
                        state::format_bytes(peer.tx_bytes),
                        shell.t("detail.handshakeLabel"),
                        handshake
                    ),
                    true,
                ));
            }
        }
    }
    page = page.child(config);
    let mut logs = w::section_card().child(
        div()
            .flex()
            .justify_between()
            .items_center()
            .child(w::section_title(
                IconName::List,
                shell.t("detail.logsTitle"),
                shell.t("detail.logsDesc"),
            ))
            .child(
                w::small_button("clear-logs", shell.t("common.clear"))
                    .on_click(cx.listener(|this, _, _, cx| this.clear_logs(cx))),
            ),
    );
    if shell.logs.is_empty() {
        logs = logs.child(w::muted_text(shell.t("detail.noLogs")));
    }
    for log in shell.logs.iter().take(30) {
        logs = logs.child(
            div()
                .flex()
                .gap(px(12.))
                .py(px(5.))
                .text_size(px(11.))
                .child(
                    div()
                        .font_family(theme::mono_font())
                        .text_color(theme::muted())
                        .child(log.time.clone()),
                )
                .child(
                    div()
                        .text_color(theme::teal())
                        .child(state::state_label(shell.lang, &log.state)),
                )
                .child(div().flex_1().child(log.message.clone())),
        );
    }
    page.child(logs).into_any_element()
}
