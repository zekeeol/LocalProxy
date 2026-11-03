//! Empty state shown when no profile exists yet.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context, IntoElement};
use gpui_kit::assets::IconName;

use crate::app::AppShell;
use crate::theme;
use crate::widgets as w;

pub fn render(shell: &AppShell, cx: &mut Context<AppShell>) -> AnyElement {
    let ready = shell.engine_ready;
    let title = if ready {
        shell.t("empty.readyTitle")
    } else {
        shell.t("empty.waitingTitle")
    };
    let description = if ready {
        shell.t("empty.readyDesc")
    } else {
        shell.t("empty.waitingDesc")
    };

    let mut body = div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .min_h(px(460.))
        .p(px(30.))
        .text_size(theme::BODY_SIZE)
        .text_color(theme::muted())
        .child(gpui::img("brand/localproxy-icon.png").size(px(90.)))
        .child(
            div()
                .mt(px(16.))
                .mb(px(8.))
                .text_size(px(20.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(theme::ink())
                .child(title),
        )
        .child(
            div()
                .max_w(px(420.))
                .text_center()
                .text_size(px(13.))
                .child(description),
        );

    if ready {
        body = body.child(
            div().mt(px(20.)).child(
                w::primary_button("empty-create", shell.t("empty.create"))
                    .icon(IconName::Plus)
                    .on_click(cx.listener(|this, _, window, cx| this.open_new_profile(window, cx))),
            ),
        );
    }

    body.into_any_element()
}
