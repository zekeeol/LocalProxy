//! Application-wide outbound proxies and reusable SSH connections.
use crate::app::AppShell;
use crate::i18n::{self, Lang, LANGUAGES};
use crate::{theme, widgets as w};
use gpui::prelude::*;
use gpui::{div, px, Context, Entity, IntoElement, Render, WeakEntity, Window};
use gpui_kit::component::input::InputState;
use gpui_kit::component::switch::Switch;
use local_proxy_rust::api::local_proxy::{
    AppSettingsDraft, AppSettingsView, ProxyEndpoint, SecretPatch, SecretPatchKind, SshAuthMode,
    SshConnectionDraft, SshConnectionView,
};

type Input = Entity<InputState>;
fn input(
    value: impl Into<String>,
    masked: bool,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Input {
    let value = value.into();
    cx.new(|cx| {
        InputState::new(window, cx)
            .masked(masked)
            .default_value(value)
    })
}
fn value(input: &Input, cx: &gpui::App) -> String {
    input.read(cx).value().to_string()
}
fn patch(name: &str, value: String, configured: bool) -> SecretPatch {
    SecretPatch {
        name: name.into(),
        kind: if !value.is_empty() {
            SecretPatchKind::Set
        } else if configured {
            SecretPatchKind::Keep
        } else {
            SecretPatchKind::Clear
        },
        value: if value.is_empty() { None } else { Some(value) },
    }
}
struct SshRow {
    id: Option<String>,
    name: Input,
    host: Input,
    port: Input,
    username: Input,
    path: Input,
    password: Input,
    passphrase: Input,
    auth: SshAuthMode,
    password_configured: bool,
    passphrase_configured: bool,
    via_proxy: bool,
}
impl SshRow {
    fn new(
        existing: Option<&SshConnectionView>,
        window: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Self {
        Self {
            id: existing.map(|x| x.id.clone()),
            name: input(
                existing.map(|x| x.name.clone()).unwrap_or_default(),
                false,
                window,
                cx,
            ),
            host: input(
                existing.map(|x| x.host.clone()).unwrap_or_default(),
                false,
                window,
                cx,
            ),
            port: input(
                existing.map(|x| x.port).unwrap_or(22).to_string(),
                false,
                window,
                cx,
            ),
            username: input(
                existing.map(|x| x.username.clone()).unwrap_or_default(),
                false,
                window,
                cx,
            ),
            path: input(
                existing
                    .map(|x| x.private_key_path.clone())
                    .unwrap_or_default(),
                false,
                window,
                cx,
            ),
            password: input("", true, window, cx),
            passphrase: input("", true, window, cx),
            auth: existing
                .map(|x| x.auth_mode.clone())
                .unwrap_or(SshAuthMode::Password),
            password_configured: existing.map(|x| x.password_configured).unwrap_or(false),
            passphrase_configured: existing.map(|x| x.passphrase_configured).unwrap_or(false),
            via_proxy: existing.map(|x| x.use_system_proxy).unwrap_or(false),
        }
    }
}
pub struct SettingsView {
    parent: WeakEntity<AppShell>,
    lang: Lang,
    http_enabled: bool,
    socks_enabled: bool,
    http_host: Input,
    http_port: Input,
    socks_host: Input,
    socks_port: Input,
    ssh: Vec<SshRow>,
    error: Option<String>,
}
impl SettingsView {
    pub fn new(
        parent: WeakEntity<AppShell>,
        lang: Lang,
        settings: AppSettingsView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            parent,
            lang,
            http_enabled: settings.http_proxy.is_some(),
            socks_enabled: settings.socks_proxy.is_some(),
            http_host: input(
                settings
                    .http_proxy
                    .as_ref()
                    .map(|p| p.host.clone())
                    .unwrap_or_else(|| "127.0.0.1".into()),
                false,
                window,
                cx,
            ),
            http_port: input(
                settings
                    .http_proxy
                    .as_ref()
                    .map(|p| p.port)
                    .unwrap_or(8099)
                    .to_string(),
                false,
                window,
                cx,
            ),
            socks_host: input(
                settings
                    .socks_proxy
                    .as_ref()
                    .map(|p| p.host.clone())
                    .unwrap_or_else(|| "127.0.0.1".into()),
                false,
                window,
                cx,
            ),
            socks_port: input(
                settings
                    .socks_proxy
                    .as_ref()
                    .map(|p| p.port)
                    .unwrap_or(8099)
                    .to_string(),
                false,
                window,
                cx,
            ),
            ssh: settings
                .ssh_connections
                .iter()
                .map(|x| SshRow::new(Some(x), window, cx))
                .collect(),
            error: None,
        }
    }
    fn t(&self, key: &str) -> String {
        i18n::t(self.lang, key)
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let endpoint = |enabled: bool,
                        host: &Input,
                        port: &Input,
                        key: &str|
         -> Result<Option<ProxyEndpoint>, String> {
            if !enabled {
                return Ok(None);
            }
            let host = value(host, cx).trim().to_string();
            let port = value(port, cx).parse::<u16>().ok().filter(|p| *p > 0);
            match (host.is_empty(), port) {
                (false, Some(port)) => Ok(Some(ProxyEndpoint { host, port })),
                _ => Err(self.t(key)),
            }
        };
        let result = (|| -> Result<AppSettingsDraft, String> {
            let http_proxy = endpoint(
                self.http_enabled,
                &self.http_host,
                &self.http_port,
                "settings.invalidHttpProxy",
            )?;
            let socks_proxy = endpoint(
                self.socks_enabled,
                &self.socks_host,
                &self.socks_port,
                "settings.invalidSocksProxy",
            )?;
            let mut ssh_connections = Vec::new();
            for row in &self.ssh {
                let name = value(&row.name, cx).trim().to_string();
                let host = value(&row.host, cx).trim().to_string();
                let username = value(&row.username, cx).trim().to_string();
                let port = value(&row.port, cx).parse::<u16>().ok().filter(|p| *p > 0);
                let path = value(&row.path, cx).trim().to_string();
                let password = value(&row.password, cx);
                let passphrase = value(&row.passphrase, cx);
                if name.is_empty() || host.is_empty() || username.is_empty() || port.is_none() {
                    return Err(self.t("settings.invalidSshConnection"));
                }
                if matches!(row.auth, SshAuthMode::Password)
                    && password.is_empty()
                    && !row.password_configured
                {
                    return Err(i18n::tp(
                        self.lang,
                        "settings.sshNeedsPassword",
                        &[("name", &name)],
                    ));
                }
                if matches!(row.auth, SshAuthMode::PrivateKey) && path.is_empty() {
                    return Err(i18n::tp(
                        self.lang,
                        "settings.sshNeedsKey",
                        &[("name", &name)],
                    ));
                }
                ssh_connections.push(SshConnectionDraft {
                    id: row.id.clone(),
                    name,
                    host,
                    port: port.unwrap(),
                    username,
                    auth_mode: row.auth.clone(),
                    private_key_path: path,
                    password: patch("password", password, row.password_configured),
                    passphrase: patch("passphrase", passphrase, row.passphrase_configured),
                    use_system_proxy: row.via_proxy,
                });
            }
            Ok(AppSettingsDraft {
                http_proxy,
                socks_proxy,
                ssh_connections,
            })
        })();
        match result {
            Ok(draft) => {
                self.parent
                    .update(cx, |parent, cx| parent.save_settings_from_view(draft, cx))
                    .ok();
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }
}
impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut languages = div().flex().gap(px(6.)).flex_wrap();
        for lang in LANGUAGES {
            let button = if self.lang == lang {
                w::primary_button(format!("settings-lang-{}", lang.code()), lang.label())
            } else {
                w::outline_button(format!("settings-lang-{}", lang.code()), lang.label())
            };
            languages = languages.child(button.on_click(cx.listener(move |this, _, _, cx| {
                this.lang = lang;
                this.parent
                    .update(cx, |parent, cx| parent.set_language(lang, cx))
                    .ok();
                cx.notify();
            })));
        }
        let mut body = w::modal_body().child(
            w::form_section()
                .child(w::form_section_title(
                    self.t("settings.languageTitle"),
                    self.t("settings.languageDesc"),
                ))
                .child(languages),
        );
        let mut proxy = w::form_section().child(w::form_section_title(
            self.t("settings.upstreamProxyTitle"),
            self.t("settings.upstreamProxyDesc"),
        ));
        for (is_http, enabled, host, port) in [
            (
                true,
                self.http_enabled,
                self.http_host.clone(),
                self.http_port.clone(),
            ),
            (
                false,
                self.socks_enabled,
                self.socks_host.clone(),
                self.socks_port.clone(),
            ),
        ] {
            let (id, title, desc) = if is_http {
                ("http-proxy", "settings.httpProxy", "settings.httpProxyUse")
            } else {
                (
                    "socks-proxy",
                    "settings.socksProxy",
                    "settings.socksProxyUse",
                )
            };
            let mut block = div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .mb(px(14.))
                .child(
                    Switch::new(id)
                        .label(self.t(title))
                        .checked(enabled)
                        .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                            if is_http {
                                this.http_enabled = *checked;
                            } else {
                                this.socks_enabled = *checked;
                            }
                            cx.notify();
                        })),
                )
                .child(w::muted_text(self.t(desc)));
            if enabled {
                block = block.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .child(w::text_input(&host).id(format!("{id}-host")).w_full()),
                        )
                        .child(
                            div()
                                .w(px(120.))
                                .child(w::text_input(&port).id(format!("{id}-port")).w_full()),
                        ),
                );
            }
            proxy = proxy.child(block);
        }
        body = body.child(proxy);
        let mut ssh_section = w::form_section().child(w::form_section_title(
            self.t("settings.sshTitle"),
            self.t("settings.sshDesc"),
        ));
        if self.ssh.is_empty() {
            ssh_section = ssh_section.child(w::empty_hint(
                self.t("settings.noSsh"),
                self.t("settings.noSshHint"),
            ));
        }
        for (index, row) in self.ssh.iter().enumerate() {
            let field = |label: String, state: &Input, id: &str| {
                w::form_field(
                    label.clone(),
                    None::<String>,
                    w::text_input(state)
                        .id(format!("ssh-{index}-{id}"))
                        .aria_label(label)
                        .w_full(),
                )
            };
            let mut card = div()
                .p(px(14.))
                .mb(px(12.))
                .rounded(px(12.))
                .border_1()
                .border_color(theme::line())
                .bg(theme::panel_soft())
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(w::heading(i18n::tp(
                            self.lang,
                            "sshCard.defaultName",
                            &[("n", &(index + 1).to_string())],
                        )))
                        .child(
                            w::small_button(format!("remove-ssh-{index}"), self.t("common.remove"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.ssh.remove(index);
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .mt(px(10.))
                        .child(div().flex_1().child(field(
                            self.t("sshCard.nameLabel"),
                            &row.name,
                            "name",
                        )))
                        .child(div().flex_1().child(field(
                            self.t("sshCard.hostLabel"),
                            &row.host,
                            "host",
                        ))),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .mt(px(10.))
                        .child(div().w(px(130.)).child(field(
                            self.t("sshCard.portLabel"),
                            &row.port,
                            "port",
                        )))
                        .child(div().flex_1().child(field(
                            self.t("sshCard.usernameLabel"),
                            &row.username,
                            "username",
                        ))),
                );
            let password_active = matches!(row.auth, SshAuthMode::Password);
            let password_button = if password_active {
                w::primary_button(
                    format!("auth-password-{index}"),
                    self.t("sshCard.authPassword"),
                )
            } else {
                w::outline_button(
                    format!("auth-password-{index}"),
                    self.t("sshCard.authPassword"),
                )
            };
            let key_button = if !password_active {
                w::primary_button(format!("auth-key-{index}"), self.t("sshCard.authKey"))
            } else {
                w::outline_button(format!("auth-key-{index}"), self.t("sshCard.authKey"))
            };
            card = card.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .mt(px(12.))
                    .child(password_button.on_click(cx.listener(move |this, _, _, cx| {
                        this.ssh[index].auth = SshAuthMode::Password;
                        cx.notify();
                    })))
                    .child(key_button.on_click(cx.listener(move |this, _, _, cx| {
                        this.ssh[index].auth = SshAuthMode::PrivateKey;
                        cx.notify();
                    }))),
            );
            if password_active {
                card = card.child(
                    div().mt(px(10.)).child(w::form_field(
                        self.t("sshCard.passwordLabel"),
                        Some(self.t(if row.password_configured {
                            "sshCard.keepPassword"
                        } else {
                            "sshCard.enterPassword"
                        })),
                        w::text_input(&row.password)
                            .id(format!("ssh-{index}-password"))
                            .w_full(),
                    )),
                );
            } else {
                card = card
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(8.))
                            .mt(px(10.))
                            .child(div().flex_1().child(field(
                                self.t("sshCard.privateKeyLabel"),
                                &row.path,
                                "path",
                            )))
                            .child(
                                w::small_button(
                                    format!("ssh-key-choose-{index}"),
                                    self.t("common.choose"),
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                                            this.ssh[index].path.update(cx, |state, cx| {
                                                state.set_value(
                                                    path.display().to_string(),
                                                    window,
                                                    cx,
                                                )
                                            });
                                        }
                                    },
                                )),
                            ),
                    )
                    .child(
                        div().mt(px(10.)).child(w::form_field(
                            self.t("sshCard.passphraseLabel"),
                            Some(self.t(if row.passphrase_configured {
                                "sshCard.keepPassphrase"
                            } else {
                                "sshCard.optional"
                            })),
                            w::text_input(&row.passphrase)
                                .id(format!("ssh-{index}-passphrase"))
                                .w_full(),
                        )),
                    );
            }
            card = card.child(
                div().mt(px(12.)).child(
                    Switch::new(format!("ssh-proxy-{index}"))
                        .label(self.t("sshCard.viaSocks5Title"))
                        .checked(row.via_proxy)
                        .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                            this.ssh[index].via_proxy = *checked;
                            cx.notify();
                        })),
                ),
            );
            ssh_section = ssh_section.child(card);
        }
        ssh_section = ssh_section.child(
            w::outline_button("add-ssh", self.t("settings.addSsh")).on_click(cx.listener(
                |this, _, window, cx| {
                    this.ssh.push(SshRow::new(None, window, cx));
                    cx.notify();
                },
            )),
        );
        body = body.child(ssh_section);
        if let Some(error) = &self.error {
            body = body.child(w::error_banner(error.clone()));
        }
        w::modal_backdrop().child(
            w::modal_card(760.)
                .h((window.viewport_size().height - px(56.)).min(px(850.)))
                .child(w::modal_heading(
                    self.t("settings.title"),
                    self.t("settings.desc"),
                ))
                .child(body)
                .child(
                    w::modal_footer()
                        .child(
                            w::outline_button("settings-cancel", self.t("common.cancel")).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.parent
                                        .update(cx, |parent, cx| parent.close_settings(cx))
                                        .ok();
                                }),
                            ),
                        )
                        .child(
                            w::primary_button("settings-save", self.t("settings.save"))
                                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                        ),
                ),
        )
    }
}
