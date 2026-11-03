//! Profile editor for all four core profile types.
use crate::app::AppShell;
use crate::i18n::{self, Lang};
use crate::{
    state::{self, SecretRowDraft, ToastTone},
    theme, widgets as w,
};
use gpui::prelude::*;
use gpui::{div, px, Context, Entity, IntoElement, Render, WeakEntity, Window};
use gpui_kit::component::{
    input::{InputState, Textarea, TextareaState},
    switch::Switch,
};
use local_proxy_rust::api::local_proxy::{
    self, AppSettingsView, ForwardDraft, HttpDraft, ProfileDraft, ProfileView, ProxyType,
    SecretPatchKind, SshDraft, WireGuardConfig, WstunnelConfig, WstunnelForward,
};

type Input = Entity<InputState>;
type Text = Entity<TextareaState>;
fn input(
    value: impl Into<String>,
    masked: bool,
    window: &mut Window,
    cx: &mut Context<ProfileEditor>,
) -> Input {
    let value = value.into();
    cx.new(|cx| {
        InputState::new(window, cx)
            .masked(masked)
            .default_value(value)
    })
}
fn text(
    value: impl Into<String>,
    rows: usize,
    window: &mut Window,
    cx: &mut Context<ProfileEditor>,
) -> Text {
    let value = value.into();
    cx.new(|cx| {
        TextareaState::new(window, cx)
            .rows(rows)
            .default_value(value)
    })
}
fn val(input: &Input, cx: &gpui::App) -> String {
    input.read(cx).value().to_string()
}
fn txt(input: &Text, cx: &gpui::App) -> String {
    input.read(cx).value().to_string()
}
fn field(label: String, state: &Input, id: String) -> gpui::Div {
    w::form_field(
        label.clone(),
        None::<String>,
        w::text_input(state)
            .id(id)
            .aria_label(label.clone())
            .w_full(),
    )
}
fn port(value: &Input, cx: &gpui::App, allow_zero: bool, lang: Lang) -> Result<u16, String> {
    val(value, cx)
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|v| allow_zero || *v > 0)
        .ok_or_else(|| {
            i18n::t(
                lang,
                if allow_zero {
                    "common.invalidPort"
                } else {
                    "common.invalidNonzeroPort"
                },
            )
        })
}
struct SecretRow {
    name: Input,
    value: Input,
    configured: bool,
    original: Option<String>,
}
impl SecretRow {
    fn new(
        name: String,
        value: String,
        configured: bool,
        original: Option<String>,
        window: &mut Window,
        cx: &mut Context<ProfileEditor>,
    ) -> Self {
        Self {
            name: input(name, false, window, cx),
            value: input(value, true, window, cx),
            configured,
            original,
        }
    }
    fn draft(&self, cx: &gpui::App) -> SecretRowDraft {
        SecretRowDraft {
            name: val(&self.name, cx),
            value: val(&self.value, cx),
            configured: self.configured,
            original_name: self.original.clone(),
        }
    }
}
struct ForwardRow {
    protocol: String,
    local: Input,
    host: Input,
    remote: Input,
    tag: Input,
    timeout: Input,
}
impl ForwardRow {
    fn new(
        protocol: String,
        local: u16,
        host: String,
        remote: u16,
        tag: String,
        timeout: Option<u64>,
        window: &mut Window,
        cx: &mut Context<ProfileEditor>,
    ) -> Self {
        Self {
            protocol,
            local: input(local.to_string(), false, window, cx),
            host: input(host, false, window, cx),
            remote: input(remote.to_string(), false, window, cx),
            tag: input(tag, false, window, cx),
            timeout: input(
                timeout.map(|x| x.to_string()).unwrap_or_default(),
                false,
                window,
                cx,
            ),
        }
    }
}
pub struct ProfileEditor {
    parent: WeakEntity<AppShell>,
    lang: Lang,
    settings: AppSettingsView,
    initial: Option<ProfileView>,
    kind: ProxyType,
    name: Input,
    tags: Input,
    listen: Input,
    upstream: Input,
    http_proxy: bool,
    headers: Vec<SecretRow>,
    cookies: Vec<SecretRow>,
    removed_headers: Vec<String>,
    removed_cookies: Vec<String>,
    batch_headers: Text,
    batch_cookies: Text,
    batch_headers_open: bool,
    batch_cookies_open: bool,
    ssh_connection: String,
    ssh_forwards: Vec<ForwardRow>,
    ws_command: Text,
    ws_server: Input,
    ws_prefix: Input,
    ws_tls: bool,
    ws_forwards: Vec<ForwardRow>,
    wg_text: Text,
    error: Option<String>,
}
impl ProfileEditor {
    pub fn new(
        parent: WeakEntity<AppShell>,
        lang: Lang,
        settings: AppSettingsView,
        initial: Option<ProfileView>,
        kind: ProxyType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let http = initial.as_ref().and_then(|p| p.http.as_ref());
        let ssh = initial.as_ref().and_then(|p| p.ssh.as_ref());
        let ws = initial.as_ref().and_then(|p| p.wstunnel.as_ref());
        let wg = initial.as_ref().and_then(|p| p.wireguard.as_ref());
        let headers = http
            .map(|h| {
                h.headers
                    .iter()
                    .map(|f| {
                        SecretRow::new(
                            f.name.clone(),
                            String::new(),
                            f.configured,
                            Some(f.name.clone()),
                            window,
                            cx,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let cookies = http
            .map(|h| {
                h.cookies
                    .iter()
                    .map(|f| {
                        SecretRow::new(
                            f.name.clone(),
                            String::new(),
                            f.configured,
                            Some(f.name.clone()),
                            window,
                            cx,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let ssh_forwards = ssh
            .map(|s| {
                s.forwards
                    .iter()
                    .map(|f| {
                        ForwardRow::new(
                            String::new(),
                            f.local_port,
                            f.remote_host.clone(),
                            f.remote_port,
                            f.tag.clone(),
                            None,
                            window,
                            cx,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let ws_forwards = ws
            .map(|s| {
                s.forwards
                    .iter()
                    .map(|f| {
                        ForwardRow::new(
                            f.protocol.clone(),
                            f.local_port,
                            f.remote_host.clone(),
                            f.remote_port,
                            String::new(),
                            f.timeout_sec,
                            window,
                            cx,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let ssh_connection = ssh
            .map(|s| s.connection_id.clone())
            .or_else(|| settings.ssh_connections.first().map(|s| s.id.clone()))
            .unwrap_or_default();
        let wg_text = wg
            .map(|wg| state::wireguard_config_text(wg, lang, None))
            .unwrap_or_default();
        Self {
            parent,
            lang,
            name: input(
                initial.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
                false,
                window,
                cx,
            ),
            tags: input(
                initial
                    .as_ref()
                    .map(|p| p.tags.join(", "))
                    .unwrap_or_default(),
                false,
                window,
                cx,
            ),
            listen: input(
                http.map(|h| h.listen_port).unwrap_or(0).to_string(),
                false,
                window,
                cx,
            ),
            upstream: input(
                http.map(|h| h.upstream_url.clone()).unwrap_or_default(),
                false,
                window,
                cx,
            ),
            http_proxy: http.map(|h| h.use_system_proxy).unwrap_or(false),
            headers,
            cookies,
            removed_headers: Vec::new(),
            removed_cookies: Vec::new(),
            batch_headers: text("", 3, window, cx),
            batch_cookies: text("", 3, window, cx),
            batch_headers_open: false,
            batch_cookies_open: false,
            ssh_connection,
            ssh_forwards,
            ws_command: text("", 3, window, cx),
            ws_server: input(
                ws.map(|s| s.server_url.clone()).unwrap_or_default(),
                false,
                window,
                cx,
            ),
            ws_prefix: input(
                ws.map(|s| s.http_upgrade_path_prefix.clone())
                    .unwrap_or_else(|| "v1".into()),
                false,
                window,
                cx,
            ),
            ws_tls: ws.map(|s| s.tls_verify_certificate).unwrap_or(false),
            ws_forwards,
            wg_text: text(wg_text, 12, window, cx),
            initial,
            kind,
            settings,
            error: None,
        }
    }
    fn t(&self, key: &str) -> String {
        i18n::t(self.lang, key)
    }
    fn parse_wg(&self, cx: &gpui::App) -> Result<WireGuardConfig, String> {
        let mut source = txt(&self.wg_text, cx);
        let has_key = source.lines().any(|line| {
            line.split_once('=')
                .map(|(key, _)| key.trim().eq_ignore_ascii_case("PrivateKey"))
                .unwrap_or(false)
        });
        let mut keep = false;
        if !has_key {
            if let Some(profile) = &self.initial {
                if profile
                    .wireguard
                    .as_ref()
                    .map(|wg| wg.private_key_configured)
                    .unwrap_or(false)
                {
                    let key = local_proxy::wireguard_private_key(profile.id.clone())?
                        .ok_or_else(|| self.t("editor.wgKeyRequired"))?;
                    let mut lines: Vec<String> = source.lines().map(str::to_string).collect();
                    if let Some(index) = lines
                        .iter()
                        .position(|line| line.trim().eq_ignore_ascii_case("[Interface]"))
                    {
                        lines.insert(index + 1, format!("PrivateKey = {key}"));
                        source = lines.join("\n");
                        keep = true;
                    }
                }
            }
        }
        let mut config = local_proxy::parse_wg_config(source)?;
        if keep {
            config.private_key.kind = SecretPatchKind::Keep;
            config.private_key.value = None;
        }
        Ok(config)
    }
    fn import_ws(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match local_proxy::parse_wstunnel_command(txt(&self.ws_command, cx)) {
            Ok(config) => {
                if val(&self.name, cx).trim().is_empty() {
                    let host = config
                        .server_url
                        .split("://")
                        .nth(1)
                        .unwrap_or(&config.server_url)
                        .split('/')
                        .next()
                        .unwrap_or("");
                    let name = i18n::tp(self.lang, "editor.defaultWstunnelName", &[("host", host)]);
                    self.name
                        .update(cx, |state, cx| state.set_value(name, window, cx));
                }
                self.ws_server
                    .update(cx, |s, cx| s.set_value(config.server_url, window, cx));
                self.ws_prefix.update(cx, |s, cx| {
                    s.set_value(config.http_upgrade_path_prefix, window, cx)
                });
                self.ws_tls = config.tls_verify_certificate;
                self.ws_forwards = config
                    .forwards
                    .into_iter()
                    .map(|f| {
                        ForwardRow::new(
                            f.protocol,
                            f.local_port,
                            f.remote_host,
                            f.remote_port,
                            String::new(),
                            f.timeout_sec,
                            window,
                            cx,
                        )
                    })
                    .collect();
                self.error = None;
                cx.notify();
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }
    fn batch(&mut self, headers: bool, window: &mut Window, cx: &mut Context<Self>) {
        let pairs = state::parse_batch_pairs(&txt(
            if headers {
                &self.batch_headers
            } else {
                &self.batch_cookies
            },
            cx,
        ));
        if pairs.is_empty() {
            self.error = Some(self.t("secrets.batchNoPairs"));
            cx.notify();
            return;
        }
        let rows = if headers {
            &mut self.headers
        } else {
            &mut self.cookies
        };
        for (name, value) in pairs {
            if let Some(row) = rows
                .iter()
                .find(|row| val(&row.name, cx).trim().eq_ignore_ascii_case(&name))
            {
                row.value
                    .update(cx, |state, cx| state.set_value(value, window, cx));
            } else {
                rows.push(SecretRow::new(name, value, false, None, window, cx));
            }
        }
        if headers {
            self.batch_headers_open = false;
            self.batch_headers
                .update(cx, |s, cx| s.set_value("", window, cx));
        } else {
            self.batch_cookies_open = false;
            self.batch_cookies
                .update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.error = None;
        cx.notify();
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let result = (|| -> Result<ProfileDraft, String> {
            let name = val(&self.name, cx).trim().to_string();
            if name.is_empty() {
                return Err(self.t("editor.nameRequired"));
            }
            let mut draft = ProfileDraft {
                id: self.initial.as_ref().map(|p| p.id.clone()),
                name,
                tags: val(&self.tags, cx)
                    .split([',', '\n'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
                kind: self.kind.clone(),
                http: None,
                ssh: None,
                wstunnel: None,
                wireguard: None,
            };
            match self.kind {
                ProxyType::Http => {
                    let upstream_url = val(&self.upstream, cx).trim().to_string();
                    if upstream_url.is_empty() {
                        return Err(self.t("editor.upstreamRequired"));
                    }
                    let headers = self.headers.iter().map(|r| r.draft(cx)).collect::<Vec<_>>();
                    let cookies = self.cookies.iter().map(|r| r.draft(cx)).collect::<Vec<_>>();
                    draft.http = Some(HttpDraft {
                        listen_port: port(&self.listen, cx, true, self.lang)?,
                        upstream_url,
                        headers: state::build_secret_patches(
                            &headers,
                            &self.removed_headers,
                            "Header",
                            self.lang,
                        )?,
                        cookies: state::build_secret_patches(
                            &cookies,
                            &self.removed_cookies,
                            "Cookie",
                            self.lang,
                        )?,
                        use_system_proxy: self.http_proxy,
                    });
                }
                ProxyType::Ssh => {
                    if self.ssh_connection.is_empty() {
                        return Err(self.t("editor.sshConnectionRequired"));
                    }
                    let mut forwards = Vec::new();
                    for row in &self.ssh_forwards {
                        let host = val(&row.host, cx).trim().to_string();
                        if host.is_empty() {
                            return Err(self.t("editor.forwardRemoteRequired"));
                        }
                        forwards.push(ForwardDraft {
                            local_port: port(&row.local, cx, true, self.lang)?,
                            remote_host: host,
                            remote_port: port(&row.remote, cx, false, self.lang)?,
                            tag: val(&row.tag, cx).trim().to_string(),
                        });
                    }
                    draft.ssh = Some(SshDraft {
                        connection_id: self.ssh_connection.clone(),
                        forwards,
                    });
                }
                ProxyType::Wstunnel => {
                    let mut forwards = Vec::new();
                    for row in &self.ws_forwards {
                        let timeout = val(&row.timeout, cx);
                        let timeout_sec = if timeout.trim().is_empty() {
                            None
                        } else {
                            Some(
                                timeout
                                    .trim()
                                    .parse::<u64>()
                                    .map_err(|_| self.t("common.invalidTimeout"))?,
                            )
                        };
                        forwards.push(WstunnelForward {
                            protocol: row.protocol.clone(),
                            local_port: port(&row.local, cx, false, self.lang)?,
                            remote_host: val(&row.host, cx).trim().to_string(),
                            remote_port: port(&row.remote, cx, false, self.lang)?,
                            timeout_sec: if row.protocol == "udp" {
                                timeout_sec
                            } else {
                                None
                            },
                        });
                    }
                    draft.wstunnel = Some(WstunnelConfig {
                        server_url: val(&self.ws_server, cx).trim().to_string(),
                        http_upgrade_path_prefix: val(&self.ws_prefix, cx).trim().to_string(),
                        tls_verify_certificate: self.ws_tls,
                        forwards,
                    });
                }
                ProxyType::WireGuard => draft.wireguard = Some(self.parse_wg(cx)?),
            }
            Ok(draft)
        })();
        match result {
            Ok(draft) => {
                self.parent
                    .update(cx, |parent, cx| parent.save_profile_from_editor(draft, cx))
                    .ok();
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }
    fn secret_section(&self, headers: bool, cx: &mut Context<Self>) -> gpui::Div {
        let (rows, label, hint, batch_open, batch) = if headers {
            (
                &self.headers,
                "Header",
                "secrets.headersHint",
                self.batch_headers_open,
                &self.batch_headers,
            )
        } else {
            (
                &self.cookies,
                "Cookie",
                "secrets.cookiesHint",
                self.batch_cookies_open,
                &self.batch_cookies,
            )
        };
        let mut section = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .mt(px(16.))
            .child(w::heading(format!("{label}s")))
            .child(w::muted_text(self.t(hint)));
        for (index, row) in rows.iter().enumerate() {
            let id = if headers { "header" } else { "cookie" };
            section = section.child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .child(div().flex_1().child(field(
                        label.into(),
                        &row.name,
                        format!("{id}-{index}-name"),
                    )))
                    .child(
                        div().flex_1().child(w::form_field(
                            self.t(if row.configured {
                                "secrets.valuePlaceholderKept"
                            } else {
                                "sshCard.optional"
                            }),
                            None::<String>,
                            w::text_input(&row.value)
                                .id(format!("{id}-{index}-value"))
                                .w_full(),
                        )),
                    )
                    .child(
                        w::small_button(format!("{id}-{index}-remove"), self.t("common.remove"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let (rows, removed) = if headers {
                                    (&mut this.headers, &mut this.removed_headers)
                                } else {
                                    (&mut this.cookies, &mut this.removed_cookies)
                                };
                                let row = rows.remove(index);
                                if row.configured {
                                    if let Some(name) = row.original {
                                        removed.push(name);
                                    }
                                }
                                cx.notify();
                            })),
                    ),
            );
        }
        section = section.child(
            div()
                .flex()
                .gap(px(8.))
                .child(
                    w::outline_button(
                        format!("add-{label}"),
                        i18n::tp(self.lang, "secrets.addLabel", &[("label", label)]),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let row =
                            SecretRow::new(String::new(), String::new(), false, None, window, cx);
                        if headers {
                            this.headers.push(row);
                        } else {
                            this.cookies.push(row);
                        }
                        cx.notify();
                    })),
                )
                .child(
                    w::small_button(
                        format!("batch-{label}"),
                        self.t(if batch_open {
                            "secrets.batchCollapse"
                        } else {
                            "secrets.batchExpand"
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if headers {
                            this.batch_headers_open = !this.batch_headers_open;
                        } else {
                            this.batch_cookies_open = !this.batch_cookies_open;
                        }
                        cx.notify();
                    })),
                ),
        );
        if batch_open {
            section = section
                .child(w::muted_text(self.t("secrets.batchHint")))
                .child(
                    Textarea::new(batch)
                        .aria_label(self.t("secrets.batchHint"))
                        .w_full(),
                )
                .child(
                    w::small_button(
                        format!("batch-apply-{label}"),
                        self.t("secrets.parseAndAdd"),
                    )
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.batch(headers, window, cx)),
                    ),
                );
        }
        section
    }
    fn forward_section(&self, ws: bool, cx: &mut Context<Self>) -> gpui::Div {
        let rows = if ws {
            &self.ws_forwards
        } else {
            &self.ssh_forwards
        };
        let mut section = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .mt(px(16.))
            .child(w::heading(self.t("editor.forwardsTitle")));
        for (index, row) in rows.iter().enumerate() {
            let prefix = if ws { "ws" } else { "ssh-forward" };
            let mut card = div()
                .p(px(12.))
                .rounded(px(10.))
                .border_1()
                .border_color(theme::line())
                .bg(theme::panel_soft())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(w::heading(i18n::tp(
                            self.lang,
                            "common.ruleIndex",
                            &[("n", &(index + 1).to_string())],
                        )))
                        .child(
                            w::small_button(
                                format!("{prefix}-{index}-remove"),
                                self.t("common.remove"),
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if ws {
                                        this.ws_forwards.remove(index);
                                    } else {
                                        this.ssh_forwards.remove(index);
                                    }
                                    cx.notify();
                                },
                            )),
                        ),
                );
            if ws {
                let udp = row.protocol == "udp";
                let udp_button = if udp {
                    w::primary_button(format!("udp-{index}"), "UDP")
                } else {
                    w::outline_button(format!("udp-{index}"), "UDP")
                };
                let tcp_button = if !udp {
                    w::primary_button(format!("tcp-{index}"), "TCP")
                } else {
                    w::outline_button(format!("tcp-{index}"), "TCP")
                };
                card = card.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .mt(px(8.))
                        .child(udp_button.on_click(cx.listener(move |this, _, _, cx| {
                            this.ws_forwards[index].protocol = "udp".into();
                            cx.notify();
                        })))
                        .child(tcp_button.on_click(cx.listener(move |this, _, _, cx| {
                            this.ws_forwards[index].protocol = "tcp".into();
                            cx.notify();
                        }))),
                );
            }
            card = card.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .mt(px(8.))
                    .child(div().w(px(110.)).child(field(
                        self.t("common.localPort"),
                        &row.local,
                        format!("{prefix}-{index}-local"),
                    )))
                    .child(div().flex_1().child(field(
                        self.t("common.remoteHost"),
                        &row.host,
                        format!("{prefix}-{index}-host"),
                    )))
                    .child(div().w(px(110.)).child(field(
                        self.t("common.remotePort"),
                        &row.remote,
                        format!("{prefix}-{index}-remote"),
                    ))),
            );
            if !ws {
                card = card.child(div().mt(px(8.)).child(field(
                    self.t("common.tag"),
                    &row.tag,
                    format!("{prefix}-{index}-tag"),
                )));
            } else if row.protocol == "udp" {
                card = card.child(
                    div()
                        .mt(px(8.))
                        .child(field(
                            self.t("editor.udpTimeout"),
                            &row.timeout,
                            format!("{prefix}-{index}-timeout"),
                        ))
                        .child(w::field_hint(self.t("editor.udpTimeoutHint"))),
                );
            }
            section = section.child(card);
        }
        section.child(
            w::outline_button(
                if ws {
                    "add-ws-forward"
                } else {
                    "add-ssh-forward"
                },
                self.t("editor.addRule"),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                let row = if ws {
                    ForwardRow::new(
                        "udp".into(),
                        50814,
                        "localhost".into(),
                        50814,
                        String::new(),
                        Some(0),
                        window,
                        cx,
                    )
                } else {
                    ForwardRow::new(
                        String::new(),
                        0,
                        String::new(),
                        0,
                        String::new(),
                        None,
                        window,
                        cx,
                    )
                };
                if ws {
                    this.ws_forwards.push(row);
                } else {
                    this.ssh_forwards.push(row);
                }
                cx.notify();
            })),
        )
    }
}
impl Render for ProfileEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = state::kind_label(self.lang, &self.kind);
        let title = i18n::tp(
            self.lang,
            if self.initial.is_some() {
                "editor.editTitle"
            } else {
                "editor.newTitle"
            },
            &[("kind", &kind)],
        );
        let mut body = w::modal_body();
        if self.kind == ProxyType::Wstunnel {
            body = body.child(
                w::form_section()
                    .child(w::form_section_title(
                        self.t("editor.quickImportTitle"),
                        self.t("editor.quickImportDesc"),
                    ))
                    .child(
                        Textarea::new(&self.ws_command)
                            .aria_label(self.t("editor.quickImportTitle"))
                            .w_full(),
                    )
                    .child(div().mt(px(8.)).child(
                        w::small_button("parse-wstunnel", self.t("editor.parseAndFill")).on_click(
                            cx.listener(|this, _, window, cx| this.import_ws(window, cx)),
                        ),
                    )),
            );
        }
        body = body.child(
            w::form_section()
                .child(w::form_section_title(
                    self.t("editor.basicInfo"),
                    self.t("editor.basicInfoDesc"),
                ))
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(div().flex_1().child(field(
                            self.t("editor.nameLabel"),
                            &self.name,
                            "profile-name".into(),
                        )))
                        .child(div().flex_1().child(field(
                            self.t("editor.tagOptional"),
                            &self.tags,
                            "profile-tags".into(),
                        ))),
                ),
        );
        match self.kind {
            ProxyType::Http => {
                let mut section = w::form_section()
                    .child(w::form_section_title(
                        self.t("editor.httpTitle"),
                        self.t("editor.httpDesc"),
                    ))
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(div().w(px(150.)).child(field(
                                self.t("common.localPort"),
                                &self.listen,
                                "http-listen".into(),
                            )))
                            .child(div().flex_1().child(field(
                                self.t("detail.upstreamBaseUrl"),
                                &self.upstream,
                                "http-upstream".into(),
                            ))),
                    )
                    .child(w::field_hint(self.t("editor.listenPortHint")))
                    .child(
                        div().mt(px(12.)).child(
                            Switch::new("http-use-proxy")
                                .checked(self.http_proxy)
                                .label(self.t("editor.useAppProxy"))
                                .on_change(cx.listener(|this, checked: &bool, _, cx| {
                                    this.http_proxy = *checked;
                                    cx.notify();
                                })),
                        ),
                    );
                section = section
                    .child(self.secret_section(true, cx))
                    .child(self.secret_section(false, cx));
                body = body.child(section);
            }
            ProxyType::Ssh => {
                let mut section = w::form_section().child(w::form_section_title(
                    self.t("editor.sshTitle"),
                    self.t("editor.sshDesc"),
                ));
                if self.settings.ssh_connections.is_empty() {
                    section = section.child(w::empty_hint(
                        self.t("editor.noSshConnections"),
                        self.t("editor.noSshConnectionsHint"),
                    ));
                } else {
                    section = section.child(w::heading(self.t("editor.useSshConnection")));
                    for connection in &self.settings.ssh_connections {
                        let id = connection.id.clone();
                        let selected = id == self.ssh_connection;
                        let label = format!(
                            "{} · {}@{}",
                            connection.name,
                            connection.username,
                            state::format_address(&connection.host, connection.port)
                        );
                        let button = if selected {
                            w::primary_button(format!("select-ssh-{id}"), label)
                        } else {
                            w::outline_button(format!("select-ssh-{id}"), label)
                        };
                        section = section.child(div().mt(px(6.)).child(button.on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.ssh_connection = id.clone();
                                cx.notify();
                            }),
                        )));
                    }
                }
                body = body.child(section.child(self.forward_section(false, cx)));
            }
            ProxyType::Wstunnel => {
                body = body.child(
                    w::form_section()
                        .child(w::form_section_title(
                            self.t("editor.wstunnelTitle"),
                            self.t("editor.wstunnelDesc"),
                        ))
                        .child(
                            div()
                                .flex()
                                .gap(px(10.))
                                .child(div().flex_1().child(field(
                                    self.t("editor.serverUrl"),
                                    &self.ws_server,
                                    "ws-server".into(),
                                )))
                                .child(div().w(px(180.)).child(field(
                                    self.t("detail.upgradePathPrefix"),
                                    &self.ws_prefix,
                                    "ws-prefix".into(),
                                ))),
                        )
                        .child(
                            div().mt(px(12.)).child(
                                Switch::new("ws-tls")
                                    .checked(self.ws_tls)
                                    .label(self.t("editor.tlsVerifyLabel"))
                                    .on_change(cx.listener(|this, checked: &bool, _, cx| {
                                        this.ws_tls = *checked;
                                        cx.notify();
                                    })),
                            ),
                        )
                        .child(self.forward_section(true, cx)),
                );
            }
            ProxyType::WireGuard => {
                body = body.child(
                    w::form_section()
                        .child(w::form_section_title(
                            self.t("editor.wgImportTitle"),
                            self.t("editor.wgImportDesc"),
                        ))
                        .child(
                            Textarea::new(&self.wg_text)
                                .aria_label(self.t("editor.wgImportTitle"))
                                .w_full(),
                        )
                        .child(
                            div().mt(px(8.)).child(
                                w::small_button("parse-wireguard", self.t("editor.parseAndImport"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        match this.parse_wg(cx) {
                                            Ok(_) => {
                                                if val(&this.name, cx).trim().is_empty() {
                                                    let name = this.t("editor.defaultWgName");
                                                    this.name.update(cx, |state, cx| {
                                                        state.set_value(name, window, cx)
                                                    });
                                                }
                                                this.error = None;
                                                this.parent
                                                    .update(cx, |p, cx| {
                                                        p.notify(
                                                            i18n::t(this.lang, "editor.wgImported"),
                                                            ToastTone::Success,
                                                            cx,
                                                        )
                                                    })
                                                    .ok();
                                            }
                                            Err(e) => this.error = Some(e),
                                        }
                                        cx.notify();
                                    })),
                            ),
                        ),
                );
            }
        }
        if let Some(error) = &self.error {
            body = body.child(w::error_banner(error.clone()));
        }
        w::modal_backdrop().child(
            w::modal_card(760.)
                .h((window.viewport_size().height - px(56.)).min(px(850.)))
                .child(w::modal_heading(title, self.t("editor.desc")))
                .child(body)
                .child(
                    w::modal_footer()
                        .child(
                            w::outline_button("editor-cancel", self.t("common.cancel")).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.parent.update(cx, |p, cx| p.close_editor(cx)).ok();
                                }),
                            ),
                        )
                        .child(
                            w::primary_button("editor-save", self.t("editor.save"))
                                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                        ),
                ),
        )
    }
}
