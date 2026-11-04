//! Application shell: owns the profile list, the selected detail view, the
//! (single) window chrome and all dialogs, and coordinates the networking
//! core through [`crate::bridge`].

use std::time::{Duration, Instant};

use crate::bridge::{self, CoreUpdate};
use crate::i18n::{self, Lang};
use crate::state::{self, HostKeyPrompt, LogEntry, Toast, ToastTone};
use crate::theme;
use crate::views;
use crate::views::editor::ProfileEditor;
use crate::views::settings::SettingsView;
use crate::wireguard_status::StatusCache;
use gpui::prelude::*;
use gpui::{
    div, px, AnyElement, Context, Entity, IntoElement, Render, SharedString, Task, WeakEntity,
    Window,
};
use local_proxy_rust::api::local_proxy::{
    self, AppSettingsDraft, AppSettingsView, Endpoint, EventKind, ProfileDraft, ProfileSummary,
    ProfileView, ProxyEvent, ProxyType, ServiceState,
};

const POLL_INTERVAL: Duration = Duration::from_millis(120);
pub(crate) const WIREGUARD_POLL: Duration = Duration::from_secs(5);
const TOAST_LIFETIME: Duration = Duration::from_millis(3800);

/// Dialogs that don't need their own entity.
#[derive(Clone, Debug)]
pub enum InlineOverlay {
    /// Create a new profile: choose the proxy kind.
    TypePicker,
    /// SSH first-connection host key confirmation.
    HostKey(HostKeyPrompt),
    /// Delete confirmation for the selected profile.
    ConfirmDelete { profile_id: String, name: String },
}

pub struct AppShell {
    // ---- persistent UI state -------------------------------------------------
    pub lang: Lang,
    pub profiles: Vec<ProfileSummary>,
    pub selected_id: Option<String>,
    pub selected: Option<ProfileView>,
    pub settings: AppSettingsView,
    pub logs: Vec<LogEntry>,
    pub toasts: Vec<Toast>,
    pub engine_ready: bool,
    pub refreshing: bool,

    // ---- overlays ------------------------------------------------------------
    pub inline_overlay: Option<InlineOverlay>,
    pub editor: Option<Entity<ProfileEditor>>,
    pub settings_view: Option<Entity<SettingsView>>,
    pub menu_open: bool,
    pub language_menu_open: bool,

    // ---- wireguard status ----------------------------------------------------
    pub wg_statuses: StatusCache,

    // ---- bookkeeping ---------------------------------------------------------
    next_wireguard_poll: Instant,
    next_toast_id: u64,
    _poll_task: Task<()>,
}

impl AppShell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, lang: Lang) -> Self {
        let mut this = Self {
            lang,
            profiles: Vec::new(),
            selected_id: None,
            selected: None,
            settings: AppSettingsView {
                http_proxy: None,
                socks_proxy: None,
                ssh_connections: Vec::new(),
            },
            logs: Vec::new(),
            toasts: Vec::new(),
            engine_ready: false,
            refreshing: false,
            inline_overlay: None,
            editor: None,
            settings_view: None,
            menu_open: false,
            language_menu_open: false,
            wg_statuses: StatusCache::default(),
            next_wireguard_poll: Instant::now(),
            next_toast_id: 1,
            _poll_task: Task::ready(()),
        };
        cx.observe_window_activation(window, |_, window, _| {
            if window.is_window_active() {
                bridge::request_wireguard_revalidate();
            }
        })
        .detach();
        this.reload(cx);
        this._poll_task = this.start_poller(cx);
        this
    }

    // -----------------------------------------------------------------------
    // Translations
    // -----------------------------------------------------------------------

    pub fn t(&self, key: &str) -> String {
        i18n::t(self.lang, key)
    }

    pub fn tp(&self, key: &str, params: &[(&str, &str)]) -> String {
        i18n::tp(self.lang, key, params)
    }

    // -----------------------------------------------------------------------
    // Core data
    // -----------------------------------------------------------------------

    /// Reload settings, the profile list and the selected profile from the
    /// core. This is synchronous: all these calls are small file operations.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.refreshing = true;
        match local_proxy::get_settings() {
            Ok(settings) => {
                self.settings = settings;
                self.engine_ready = true;
            }
            Err(error) => {
                self.engine_ready = false;
                self.notify(error, ToastTone::Error, cx);
            }
        }
        match local_proxy::list_profiles() {
            Ok(profiles) => {
                self.profiles = profiles;
                self.engine_ready = true;
            }
            Err(error) => {
                self.engine_ready = false;
                self.notify(error, ToastTone::Error, cx);
            }
        }

        let keep = self
            .selected_id
            .clone()
            .filter(|id| self.profiles.iter().any(|profile| &profile.id == id));
        let next_id = keep.or_else(|| self.profiles.first().map(|profile| profile.id.clone()));
        self.selected_id = next_id.clone();
        self.selected = next_id.and_then(|id| local_proxy::get_profile(id).ok());
        self.wg_statuses.retain(|id| {
            self.profiles.iter().any(|profile| {
                profile.id == id
                    && profile.kind == ProxyType::WireGuard
                    && profile.state == ServiceState::Running
            })
        });
        let running_wireguard = self.selected.as_ref().and_then(|profile| {
            (profile.kind == ProxyType::WireGuard && profile.state == ServiceState::Running)
                .then(|| profile.id.clone())
        });
        if let Some(id) = running_wireguard {
            self.request_wireguard_status(&id, Instant::now());
        }
        self.refreshing = false;
        cx.notify();
    }

    pub fn select_profile(&mut self, id: String, cx: &mut Context<Self>) {
        let changed = self.selected_id.as_deref() != Some(id.as_str());
        match local_proxy::get_profile(id.clone()) {
            Ok(profile) => {
                let request_status = profile.kind == ProxyType::WireGuard
                    && profile.state == ServiceState::Running
                    && (changed || self.wg_statuses.get(&id).is_none());
                self.selected_id = Some(id.clone());
                self.selected = Some(profile);
                if request_status {
                    self.request_wireguard_status(&id, Instant::now());
                }
            }
            Err(error) => self.notify(error, ToastTone::Error, cx),
        }
        cx.notify();
    }

    pub fn toggle_selected(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.selected.clone() else {
            return;
        };
        let result = if state::is_active(&profile.state) {
            local_proxy::stop_profile(profile.id.clone()).map(|_| {
                self.notify(
                    self.tp("app.stopped", &[("name", &profile.name)]),
                    ToastTone::Info,
                    cx,
                );
            })
        } else {
            local_proxy::start_profile(profile.id.clone()).map(|_| {
                self.notify(
                    self.tp("app.starting", &[("name", &profile.name)]),
                    ToastTone::Success,
                    cx,
                );
            })
        };
        if let Err(error) = result {
            self.notify(error, ToastTone::Error, cx);
        }
        self.reload(cx);
    }

    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.selected.clone() else {
            return;
        };
        match local_proxy::delete_profile(profile.id) {
            Ok(()) => {
                self.notify(self.t("app.profileDeleted"), ToastTone::Info, cx);
                self.inline_overlay = None;
                self.selected_id = None;
                self.selected = None;
                self.reload(cx);
            }
            Err(error) => self.notify(error, ToastTone::Error, cx),
        }
    }

    pub fn confirm_host_key(
        &mut self,
        prompt: &HostKeyPrompt,
        accepted: bool,
        cx: &mut Context<Self>,
    ) {
        let result = local_proxy::confirm_host_key(
            prompt.profile_id.clone(),
            prompt.fingerprint.clone(),
            accepted,
        );
        self.inline_overlay = None;
        match result {
            Ok(()) => {
                let message = if accepted {
                    self.t("app.hostKeySaved")
                } else {
                    self.t("app.hostKeyRejected")
                };
                self.notify(
                    message,
                    if accepted {
                        ToastTone::Success
                    } else {
                        ToastTone::Info
                    },
                    cx,
                );
            }
            Err(error) => self.notify(error, ToastTone::Error, cx),
        }
        self.reload(cx);
    }

    // -----------------------------------------------------------------------
    // Editor / settings overlays
    // -----------------------------------------------------------------------

    pub fn open_new_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.inline_overlay = Some(InlineOverlay::TypePicker);
        let _ = window;
        cx.notify();
    }

    pub fn begin_create(&mut self, kind: ProxyType, window: &mut Window, cx: &mut Context<Self>) {
        self.inline_overlay = None;
        self.spawn_editor(kind, None, window, cx);
    }

    pub fn begin_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self.selected.clone() else {
            return;
        };
        self.spawn_editor(profile.kind.clone(), Some(profile), window, cx);
    }

    fn spawn_editor(
        &mut self,
        kind: ProxyType,
        initial: Option<ProfileView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let parent: WeakEntity<AppShell> = cx.entity().downgrade();
        let settings = self.settings.clone();
        let lang = self.lang;
        self.editor = Some(
            cx.new(|cx| ProfileEditor::new(parent, lang, settings, initial, kind, window, cx)),
        );
        cx.notify();
    }

    pub fn close_editor(&mut self, cx: &mut Context<Self>) {
        self.editor = None;
        cx.notify();
    }

    /// Called by the editor once it has collected a valid draft.
    pub fn save_profile_from_editor(&mut self, draft: ProfileDraft, cx: &mut Context<Self>) {
        match local_proxy::save_profile(draft) {
            Ok(saved) => {
                self.editor = None;
                self.selected_id = Some(saved.id.clone());
                self.selected = Some(saved);
                self.notify(self.t("app.profileSaved"), ToastTone::Success, cx);
                self.reload(cx);
            }
            Err(error) => self.notify(error, ToastTone::Error, cx),
        }
    }

    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_view.is_some() {
            return;
        }
        let parent: WeakEntity<AppShell> = cx.entity().downgrade();
        let settings = self.settings.clone();
        let lang = self.lang;
        self.settings_view =
            Some(cx.new(|cx| SettingsView::new(parent, lang, settings, window, cx)));
        cx.notify();
    }

    pub fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_view = None;
        cx.notify();
    }

    pub fn save_settings_from_view(&mut self, draft: AppSettingsDraft, cx: &mut Context<Self>) {
        match local_proxy::save_settings(draft) {
            Ok(settings) => {
                self.settings = settings;
                self.settings_view = None;
                self.notify(self.t("app.settingsSaved"), ToastTone::Success, cx);
                self.reload(cx);
            }
            Err(error) => self.notify(error, ToastTone::Error, cx),
        }
    }

    pub fn set_language(&mut self, lang: Lang, cx: &mut Context<Self>) {
        if self.lang == lang {
            return;
        }
        self.lang = lang;
        i18n::save_language(lang);
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Toasts / logs / clipboard
    // -----------------------------------------------------------------------

    pub fn notify(
        &mut self,
        message: impl Into<SharedString>,
        tone: ToastTone,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        self.toasts.push(Toast {
            id,
            message: message.into().to_string(),
            tone,
            expires_at: Instant::now() + TOAST_LIFETIME,
        });
        // Keep the stack short, like the original UI (max four visible).
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
        cx.notify();
    }

    pub fn dismiss_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        self.toasts.retain(|toast| toast.id != id);
        cx.notify();
    }

    pub fn push_log(&mut self, state: ServiceState, message: String) {
        self.logs.insert(
            0,
            LogEntry {
                time: state::now_time(),
                state,
                message,
            },
        );
        self.logs.truncate(100);
    }

    pub fn clear_logs(&mut self, cx: &mut Context<Self>) {
        self.logs.clear();
        cx.notify();
    }

    pub fn copy_text(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
        self.notify(
            self.tp("common.copied", &[("value", &text)]),
            ToastTone::Success,
            cx,
        );
    }

    // -----------------------------------------------------------------------
    // Core event pump
    // -----------------------------------------------------------------------

    fn start_poller(&self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            let alive = this.update(cx, |this, cx| this.pump(cx)).is_ok();
            if !alive {
                break;
            }
        })
    }

    fn request_wireguard_status(&mut self, id: &str, now: Instant) {
        if let Some(request_id) = self.wg_statuses.begin(id) {
            bridge::request_wireguard_status(id, request_id);
        }
        self.next_wireguard_poll = now + WIREGUARD_POLL;
    }

    fn pump(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for update in bridge::drain() {
            match update {
                CoreUpdate::Event(event) => {
                    self.handle_proxy_event(event, cx);
                    changed = true;
                }
                CoreUpdate::WireGuardStatus {
                    profile_id,
                    request_id,
                    status,
                    error,
                } => {
                    let result = match error {
                        Some(error) => Err(error),
                        None => Ok(status),
                    };
                    if self
                        .wg_statuses
                        .finish(&profile_id, request_id, result, Instant::now())
                    {
                        changed = true;
                    }
                }
            }
        }

        let now = Instant::now();
        let before = self.toasts.len();
        self.toasts.retain(|toast| toast.expires_at > now);
        if self.toasts.len() != before {
            changed = true;
        }

        // Poll WireGuard helper status while a running WireGuard profile is
        // selected (mirrors the 5s poll of the Electron UI).
        let due_wireguard = self.selected.as_ref().and_then(|profile| {
            (profile.kind == ProxyType::WireGuard
                && profile.state == ServiceState::Running
                && now >= self.next_wireguard_poll)
                .then(|| profile.id.clone())
        });
        if let Some(id) = due_wireguard {
            self.request_wireguard_status(&id, now);
        }

        if changed {
            cx.notify();
        }
    }

    fn handle_proxy_event(&mut self, event: ProxyEvent, cx: &mut Context<Self>) {
        self.engine_ready = true;
        let selected_was_running = self.selected.as_ref().is_some_and(|profile| {
            profile.id == event.profile_id
                && profile.kind == ProxyType::WireGuard
                && profile.state == ServiceState::Running
        });
        for profile in &mut self.profiles {
            if profile.id == event.profile_id {
                profile.state = event.state.clone();
                if !event.endpoints.is_empty()
                    || matches!(event.state, ServiceState::Stopped | ServiceState::Starting)
                {
                    profile.endpoints = event.endpoints.clone();
                }
                profile.error = if event.state == ServiceState::Error {
                    event.message.clone()
                } else {
                    None
                };
            }
        }
        if let Some(selected) = &mut self.selected {
            if selected.id == event.profile_id {
                selected.state = event.state.clone();
                if !event.endpoints.is_empty()
                    || matches!(event.state, ServiceState::Stopped | ServiceState::Starting)
                {
                    selected.endpoints = event.endpoints.clone();
                }
                selected.error = if event.state == ServiceState::Error {
                    event.message.clone()
                } else {
                    None
                };
            }
        }
        if matches!(event.kind, EventKind::State) {
            if event.state != ServiceState::Running {
                self.wg_statuses.invalidate(&event.profile_id);
            } else if !selected_was_running
                && self.selected.as_ref().is_some_and(|profile| {
                    profile.id == event.profile_id && profile.kind == ProxyType::WireGuard
                })
            {
                self.request_wireguard_status(&event.profile_id, Instant::now());
            }
        }
        if matches!(event.kind, EventKind::HostKey) {
            if let Some(fingerprint) = event.fingerprint.clone() {
                let message = local_proxy::get_profile(event.profile_id.clone())
                    .ok()
                    .and_then(|profile| profile.ssh)
                    .map(|ssh| {
                        let host = state::format_address(&ssh.host, ssh.port);
                        self.tp("hostKey.host", &[("host", &host)])
                    })
                    .unwrap_or_else(|| self.t("app.hostKeyNeeded"));
                self.inline_overlay = Some(InlineOverlay::HostKey(HostKeyPrompt {
                    profile_id: event.profile_id.clone(),
                    fingerprint,
                    message,
                }));
            }
        }
        if matches!(event.kind, EventKind::Log) || event.message.is_some() {
            let message = event.message.unwrap_or_default();
            if !message.is_empty() {
                self.push_log(event.state, message);
            }
        }
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Detail helpers used by views
    // -----------------------------------------------------------------------

    pub fn selected_endpoints(&self) -> Vec<Endpoint> {
        self.selected
            .as_ref()
            .map(|profile| state::endpoint_for_profile(profile, self.lang))
            .unwrap_or_default()
    }
}

impl Render for AppShell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .text_color(theme::ink())
            .font_family(".SystemUIFont");

        root = root.child(views::titlebar::render(self, cx));

        let content: AnyElement = if self.selected.is_some() {
            views::detail::render(self, cx)
        } else {
            views::empty::render(self, cx)
        };

        let body = div()
            .flex()
            .flex_row()
            .flex_1()
            .min_h_0()
            .child(views::sidebar::render(self, cx))
            .child(
                div()
                    .id("detail-scroll")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .px(px(34.))
                    .pt(px(28.))
                    .pb(px(42.))
                    .bg(theme::content_bg())
                    .child(content),
            );

        root = root.child(body);

        if let Some(editor) = self.editor.clone() {
            root = root.child(editor);
        }
        if let Some(settings) = self.settings_view.clone() {
            root = root.child(settings);
        }

        root = root.child(views::overlays::render(self, cx));
        root
    }
}
