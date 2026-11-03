//! Type selection, destructive-action and host-key confirmation, and notifications.
use crate::app::{AppShell, InlineOverlay};
use crate::{state::ToastTone, theme, widgets as w};
use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context, IntoElement};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    Icon,
};
use local_proxy_rust::api::local_proxy::ProxyType;

pub fn render(shell: &AppShell, cx: &mut Context<AppShell>) -> AnyElement {
    let mut layer = div().absolute().inset_0();
    if let Some(overlay) = shell.inline_overlay.clone() {
        let card = match overlay {
            InlineOverlay::TypePicker => {
                let mut options = div().flex().flex_col().gap(px(8.));
                for (kind, hint) in [
                    (ProxyType::Http, "typePicker.httpHint"),
                    (ProxyType::Ssh, "typePicker.sshHint"),
                    (ProxyType::Wstunnel, "typePicker.wstunnelHint"),
                    (ProxyType::WireGuard, "typePicker.wgHint"),
                ] {
                    let id = format!("choose-{:?}", kind);
                    let title = crate::state::kind_label(shell.lang, &kind);
                    let icon = super::sidebar::kind_icon(&kind);
                    options = options.child(
                        Button::new(id)
                            .ghost()
                            .accessibility_label(title.clone())
                            .w_full()
                            .h(px(72.))
                            .justify_start()
                            .px(px(14.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(theme::line())
                            .bg(theme::panel())
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap(px(14.))
                                    .child(w::section_icon(icon))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .items_start()
                                            .gap(px(4.))
                                            .child(w::heading(title))
                                            .child(w::muted_text(shell.t(hint))),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.begin_create(kind.clone(), window, cx)
                            })),
                    );
                }
                w::modal_card(520.)
                    .child(w::modal_heading(
                        shell.t("typePicker.title"),
                        shell.t("typePicker.desc"),
                    ))
                    .child(div().p(px(20.)).child(options))
                    .child(w::modal_footer().child(
                        w::outline_button("type-cancel", shell.t("common.cancel")).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.inline_overlay = None;
                                cx.notify();
                            }),
                        ),
                    ))
            }
            InlineOverlay::ConfirmDelete { profile_id, name } => {
                let prompt = shell.tp("app.deleteConfirm", &[("name", &name)]);
                w::modal_card(480.)
                    .child(w::modal_heading(shell.t("detail.deleteConfig"), prompt))
                    .child(
                        w::modal_footer()
                            .child(
                                w::outline_button("delete-cancel", shell.t("common.cancel"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.inline_overlay = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                w::danger_button("delete-confirm", shell.t("common.delete"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if this.selected_id.as_deref() == Some(profile_id.as_str())
                                        {
                                            this.delete_selected(cx);
                                        }
                                    })),
                            ),
                    )
            }
            InlineOverlay::HostKey(prompt) => {
                let accept = prompt.clone();
                let reject = prompt.clone();
                w::modal_card(520.)
                    .child(w::modal_heading(
                        shell.t("hostKey.title"),
                        shell.t("hostKey.desc"),
                    ))
                    .child(
                        div()
                            .p(px(22.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(w::text(prompt.message))
                            .child(w::text(shell.t("hostKey.body")))
                            .child(
                                div()
                                    .p(px(12.))
                                    .rounded(px(9.))
                                    .bg(theme::panel_teal())
                                    .font_family(theme::mono_font())
                                    .text_color(theme::ink())
                                    .child(prompt.fingerprint),
                            )
                            .child(w::field_hint(shell.t("hostKey.hint"))),
                    )
                    .child(
                        w::modal_footer()
                            .child(
                                w::outline_button("host-key-reject", shell.t("hostKey.reject"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.confirm_host_key(&reject, false, cx)
                                    })),
                            )
                            .child(
                                w::primary_button(
                                    "host-key-accept",
                                    shell.t("hostKey.trustAndContinue"),
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.confirm_host_key(&accept, true, cx),
                                )),
                            ),
                    )
            }
        };
        layer = layer.child(w::modal_backdrop().child(card));
    }
    if !shell.toasts.is_empty() {
        let mut stack = div()
            .absolute()
            .right(px(24.))
            .bottom(px(24.))
            .w(px(360.))
            .flex()
            .flex_col()
            .gap(px(8.));
        for toast in &shell.toasts {
            let id = toast.id;
            let (icon, color) = match toast.tone {
                ToastTone::Success => (IconName::CircleCheck, theme::green()),
                ToastTone::Info => (IconName::Info, theme::teal()),
                ToastTone::Error => (IconName::CircleAlert, theme::red()),
            };
            stack = stack.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(12.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::panel())
                    .child(Icon::new(icon).size(px(17.)).text_color(color))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .child(toast.message.clone()),
                    )
                    .child(
                        w::icon_button(w::element_id("dismiss-toast", id), IconName::X)
                            .accessibility_label(shell.t("common.close"))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.dismiss_toast(id, cx)),
                            ),
                    ),
            );
        }
        layer = layer.child(stack);
    }
    layer.into_any_element()
}
