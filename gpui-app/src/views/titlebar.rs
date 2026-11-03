//! Window chrome: brand lockup, core status pill, language picker, refresh and
//! app-settings entry points.
//!
//! macOS keeps the native traffic lights (positioned by `main.rs`); this bar
//! leaves room for them on the left and owns the rest of the 62px strip.

use gpui::prelude::*;
use gpui::{div, px, rgba, AnyElement, Context, IntoElement, MouseButton, SharedString};
use gpui_kit::assets::IconName;

use crate::app::AppShell;
use crate::i18n::LANGUAGES;
use crate::theme;
use crate::widgets as w;

const TRAFFIC_LIGHT_GUTTER: f32 = 72.0;

pub fn render(shell: &AppShell, cx: &mut Context<AppShell>) -> AnyElement {
    let brand = brand_lockup(shell);

    let status_text = if shell.engine_ready {
        shell.t("titlebar.engineConnected")
    } else {
        shell.t("titlebar.engineOffline")
    };
    let status_color = if shell.engine_ready {
        theme::green()
    } else {
        theme::red()
    };
    let engine_pill = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(7.))
        .px(px(11.))
        .py(px(6.))
        .rounded(px(9999.))
        .border_1()
        .border_color(theme::line())
        .bg(rgba(0xffffffd9))
        .text_size(theme::TINY_SIZE)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme::muted())
        .child(div().size(px(7.)).rounded(px(9999.)).bg(status_color))
        .child(status_text);

    let mut right_actions = div().flex().flex_row().items_center().gap(px(6.));

    // Language picker -------------------------------------------------------
    let language_label: SharedString = shell.lang.label().into();
    let language_button = w::ghost_button("language-picker", language_label.clone())
        .icon(IconName::Globe)
        .on_click(cx.listener(|this, _, _, cx| {
            this.language_menu_open = !this.language_menu_open;
            this.menu_open = false;
            cx.notify();
        }));

    let mut language = div().relative().child(language_button);
    if shell.language_menu_open {
        let mut menu = div()
            .absolute()
            .top(px(38.))
            .right(px(0.))
            .w(px(150.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(6.))
            .rounded(px(11.))
            .border_1()
            .border_color(theme::line_strong())
            .bg(theme::panel());
        menu.style().box_shadow = Some(w::floating_shadow());
        for lang in LANGUAGES {
            let active = shell.lang == lang;
            let label: SharedString = lang.label().into();
            let mut option = w::ghost_button(
                SharedString::from(format!("language-{}", lang.code())),
                label,
            )
            .w_full()
            .justify_start();
            if active {
                option = option.bg(theme::teal_tint()).text_color(theme::teal());
            }
            option = option.on_click(cx.listener(move |this, _, _, cx| {
                this.language_menu_open = false;
                this.set_language(lang, cx);
            }));
            menu = menu.child(option);
        }
        language = language.child(gpui::deferred(menu).with_priority(1));
    }

    let settings_button = w::ghost_button("open-settings", shell.t("titlebar.appSettings"))
        .icon(IconName::Settings)
        .on_click(cx.listener(|this, _, window, cx| {
            this.language_menu_open = false;
            this.open_settings(window, cx);
        }));

    let refresh_button = w::icon_button("refresh-profiles", IconName::RefreshCw)
        .accessibility_label(shell.t("titlebar.refreshTitle"))
        .on_click(cx.listener(|this, _, _, cx| this.reload(cx)));

    right_actions = right_actions
        .child(language)
        .child(settings_button)
        .child(refresh_button);

    // The brand + status area moves the window; the action cluster does not.
    let drag_area = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(22.))
        .flex_1()
        .h_full()
        .pl(px(TRAFFIC_LIGHT_GUTTER))
        .on_mouse_down(MouseButton::Left, |_, window, _| {
            window.start_window_move();
        })
        .child(brand)
        .child(engine_pill);

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .h(theme::titlebar_height())
        .w_full()
        .px(px(24.))
        .border_b_1()
        .border_color(theme::line())
        .bg(rgba(0xfffffff0))
        .child(drag_area)
        .child(right_actions)
        .into_any_element()
}

fn brand_lockup(_shell: &AppShell) -> AnyElement {
    let logo = gpui::img("brand/localproxy-icon.png")
        .size(px(36.))
        .flex_shrink_0();

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .child(logo)
        .child(
            div()
                .text_size(px(18.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(theme::ink())
                .child("LocalProxy"),
        )
        .into_any_element()
}
