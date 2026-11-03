//! Left sidebar: profile list, create action and core status footer.

use gpui::prelude::*;
use gpui::{
    div, px, rgb, rgba, AnyElement, Context, ElementId, EntityId, IntoElement, SharedString,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use local_proxy_rust::api::local_proxy::ProxyType;

use crate::app::AppShell;
use crate::state::{endpoint_for_summary, kind_label, StatusTone};
use crate::theme;
use crate::widgets as w;
use crate::widgets::status_dot;

pub fn render(shell: &AppShell, cx: &mut Context<AppShell>) -> AnyElement {
    let mut list = div()
        .id("profile-list")
        .flex()
        .flex_col()
        .gap(px(4.))
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(13.))
        .pb(px(16.));

    if shell.profiles.is_empty() {
        list = list.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(5.))
                .p(px(20.))
                .text_size(theme::BODY_SIZE)
                .text_color(theme::muted())
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme::ink_soft())
                        .child(shell.t("sidebar.emptyTitle")),
                )
                .child(
                    div()
                        .text_size(theme::SMALL_SIZE)
                        .child(shell.t("sidebar.emptyHint")),
                ),
        );
    } else {
        for profile in &shell.profiles {
            list = list.child(profile_card(shell, profile, cx));
        }
    }

    let add_button = w::icon_button("add-profile", IconName::Plus)
        .accessibility_label(shell.t("sidebar.addTitle"))
        .size(px(34.))
        .rounded(px(11.))
        .border_1()
        .border_color(rgb_teal_border())
        .bg(rgb(0xf5fbfb))
        .text_color(theme::teal())
        .on_click(cx.listener(|this, _, window, cx| this.open_new_profile(window, cx)));

    let header = div()
        .flex()
        .flex_col()
        .px(px(23.))
        .pt(px(26.))
        .pb(px(18.))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(20.))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(theme::ink())
                                .child(shell.t("sidebar.heading")),
                        )
                        .child(div().ml(px(8.)).child(w::count_badge(shell.profiles.len()))),
                )
                .child(add_button),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .mt(px(15.))
                .text_size(theme::BODY_SIZE)
                .text_color(rgb(0x5f7886))
                .child(Icon::new(IconName::Lock).size(px(15.)))
                .child(shell.t("sidebar.loopback")),
        );

    let footer = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .px(px(18.))
        .py(px(15.))
        .border_t_1()
        .border_color(theme::line_strong())
        .bg(rgba(0xffffff61))
        .text_size(theme::SMALL_SIZE)
        .text_color(rgb(0x67808c))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(7.))
                .child(
                    Icon::new(IconName::ShieldCheck)
                        .size(px(15.))
                        .text_color(theme::green()),
                )
                .child(shell.t("sidebar.privateChannel")),
        );

    div()
        .flex()
        .flex_col()
        .w(theme::sidebar_width())
        .min_w(px(290.))
        .h_full()
        .border_r_1()
        .border_color(theme::line_strong())
        .bg(theme::sidebar())
        .child(header)
        .child(list)
        .child(footer)
        .into_any_element()
}

fn profile_card(
    shell: &AppShell,
    profile: &local_proxy_rust::api::local_proxy::ProfileSummary,
    cx: &mut Context<AppShell>,
) -> AnyElement {
    let selected = shell.selected_id.as_deref() == Some(profile.id.as_str());
    let tone = StatusTone::from_state(&profile.state);
    let id = profile.id.clone();
    let key_id = id.clone();
    let a11y_id = id.clone();
    let shell_entity = cx.entity().downgrade();

    let mut card = div()
        .id(ElementId::Name(SharedString::from(format!(
            "profile-{}",
            profile.id
        ))))
        .role(gpui::Role::Button)
        .aria_label(profile.name.clone())
        .focusable()
        .relative()
        .w_full()
        .p(px(14.))
        .rounded(px(15.))
        .border_1()
        .border_color(rgb(0xd8e8ea))
        .bg(rgba(0xffffffb8))
        .cursor_pointer()
        .hover(|style| style.border_color(rgb(0xb9d9dd)).bg(theme::panel()))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.select_profile(id.clone(), cx);
        }))
        .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
            if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                this.select_profile(key_id.clone(), cx);
                cx.stop_propagation();
            }
        }))
        .on_a11y_action(gpui::AccessibleAction::Click, move |_, _, cx| {
            shell_entity
                .update(cx, |this, cx| this.select_profile(a11y_id.clone(), cx))
                .ok();
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .justify_between()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(theme::ink())
                                .overflow_hidden()
                                .child(profile.name.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.))
                                .mt(px(7.))
                                .text_size(theme::SMALL_SIZE)
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(0x68808d))
                                .child(
                                    Icon::new(kind_icon(&profile.kind))
                                        .size(px(14.))
                                        .text_color(theme::teal()),
                                )
                                .child(kind_label(shell.lang, &profile.kind)),
                        ),
                )
                .child(status_dot(tone)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .mt(px(7.))
                .text_size(theme::SMALL_SIZE)
                .font_family(theme::mono_font())
                .text_color(rgb(0x5f7581))
                .child(Icon::new(IconName::Link2).size(px(13.)))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .child(endpoint_for_summary(profile, shell.lang)),
                ),
        );

    if !profile.tags.is_empty() {
        let mut tags = div().flex().flex_row().flex_wrap().gap(px(5.)).mt(px(10.));
        for tag in &profile.tags {
            tags = tags.child(w::tag(tag.clone()));
        }
        card = card.child(tags);
    }

    if selected {
        card = card.border_color(rgb(0xa9dddc)).bg(rgb(0xbfe8e5)).child(
            div()
                .absolute()
                .left(px(-1.))
                .top(px(14.))
                .bottom(px(14.))
                .w(px(4.))
                .rounded(px(6.))
                .bg(theme::teal_bright()),
        );
    }

    card.into_any_element()
}

pub fn kind_icon(kind: &ProxyType) -> IconName {
    match kind {
        ProxyType::Http => IconName::Globe,
        ProxyType::WireGuard => IconName::ShieldCheck,
        ProxyType::Wstunnel => IconName::Link2,
        ProxyType::Ssh => IconName::Terminal,
    }
}

fn rgb_teal_border() -> gpui::Rgba {
    gpui::rgba(0x0f766e2e)
}

#[allow(unused)]
fn _keep(_id: EntityId) {}
