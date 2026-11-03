//! Small styled building blocks shared by every view.
//!
//! The visual language mirrors the original Electron UI (soft cards, teal
//! accents, compact type). Views compose these helpers so spacing and colors
//! stay consistent without repeating long style chains.

use gpui::prelude::*;
use gpui::{div, px, rgb, BoxShadow, Div, ElementId, FontWeight, Hsla, IntoElement, SharedString};
use gpui_kit::assets::IconName;
use gpui_kit::base::box_shadow;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Icon;

use crate::i18n::Lang;
use crate::state::{state_label, StatusTone};
use crate::theme;

// ---------------------------------------------------------------------------
// Layout primitives
// ---------------------------------------------------------------------------

/// A white surface card with a hairline border and the standard 18px radius.
pub fn card() -> Div {
    div()
        .p(px(20.))
        .rounded(theme::card_radius())
        .border_1()
        .border_color(theme::card_border())
        .bg(theme::panel())
}

/// Card used for a page section (endpoints, configuration, logs).
pub fn section_card() -> Div {
    card().mb(px(18.))
}

pub fn row() -> Div {
    div().flex().flex_row().items_center()
}

pub fn column() -> Div {
    div().flex().flex_col()
}

/// The medium shadow used by floating menus and dialogs.
pub fn floating_shadow() -> Vec<BoxShadow> {
    vec![box_shadow(0., 14., 34., 0., rgba(0x2849531f))]
}

// `rgba` is not a const fn; keep a local alias so helpers stay readable.
fn rgba(value: u32) -> Hsla {
    gpui::rgba(value).into()
}

// ---------------------------------------------------------------------------
// Typography
// ---------------------------------------------------------------------------

pub fn text(value: impl Into<SharedString>) -> Div {
    div()
        .text_size(theme::BODY_SIZE)
        .text_color(theme::ink())
        .child(value.into())
}

pub fn muted_text(value: impl Into<SharedString>) -> Div {
    div()
        .text_size(theme::SMALL_SIZE)
        .text_color(theme::muted())
        .child(value.into())
}

pub fn heading(value: impl Into<SharedString>) -> Div {
    div()
        .text_size(theme::HEADING_SIZE)
        .font_weight(FontWeight::BOLD)
        .text_color(theme::ink())
        .child(value.into())
}

pub fn section_title(
    icon: IconName,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
) -> Div {
    row().gap(px(9.)).child(section_icon(icon)).child(
        column().child(heading(title)).child(
            div()
                .text_size(theme::SMALL_SIZE)
                .text_color(theme::muted())
                .child(description.into()),
        ),
    )
}

/// The 27x27 rounded teal icon square used in section headers.
pub fn section_icon(icon: IconName) -> Div {
    div()
        .size(px(27.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .bg(theme::teal_tint())
        .child(Icon::new(icon).size(px(15.)).text_color(theme::teal()))
}

// ---------------------------------------------------------------------------
// Pills, badges, tags
// ---------------------------------------------------------------------------

pub fn pill(label: impl Into<SharedString>, foreground: Hsla, background: Hsla) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .px(px(8.))
        .py(px(3.))
        .rounded(px(9999.))
        .bg(background)
        .text_color(foreground)
        .text_size(theme::TINY_SIZE)
        .font_weight(FontWeight::BOLD)
        .child(label.into())
}

pub fn status_pill(lang: Lang, state: &local_proxy_rust::api::local_proxy::ServiceState) -> Div {
    let (fg, bg) = status_colors(StatusTone::from_state(state));
    let dot = div().size(px(5.)).rounded(px(9999.)).bg(fg);
    pill(state_label(lang, state), fg, bg).child(dot)
}

pub fn status_colors(tone: StatusTone) -> (Hsla, Hsla) {
    match tone {
        StatusTone::Running => (theme::green_text().into(), theme::green_tint().into()),
        StatusTone::Pending => (theme::amber_text().into(), theme::amber_tint().into()),
        StatusTone::Error => (theme::red_text().into(), theme::red_tint().into()),
        StatusTone::Stopped => (theme::stopped_text().into(), theme::stopped_tint().into()),
    }
}

pub fn status_dot(tone: StatusTone) -> Div {
    let (fg, _) = status_colors(tone);
    div().size(px(9.)).rounded(px(9999.)).bg(fg)
}

pub fn type_pill(label: impl Into<SharedString>) -> Div {
    div()
        .px(px(8.))
        .py(px(3.))
        .rounded(px(9999.))
        .bg(theme::teal_tint())
        .text_color(theme::teal())
        .text_size(theme::TINY_SIZE)
        .font_weight(FontWeight::BOLD)
        .child(label.into())
}

pub fn tag(label: impl Into<SharedString>) -> Div {
    tag_colored(label, theme::tag_bg(), theme::tag_border(), theme::teal())
}

fn tag_colored(
    label: impl Into<SharedString>,
    background: gpui::Rgba,
    border: gpui::Rgba,
    foreground: gpui::Rgba,
) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .max_w(px(180.))
        .overflow_hidden()
        .px(px(8.))
        .py(px(3.))
        .rounded(px(9999.))
        .border_1()
        .border_color(border)
        .bg(background)
        .text_color(foreground)
        .text_size(theme::TINY_SIZE)
        .font_weight(FontWeight::BOLD)
        .child(label.into())
}

pub fn count_badge(count: usize) -> Div {
    div()
        .min_w(px(22.))
        .h(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(9999.))
        .bg(rgba(0x0f766e1c))
        .text_color(theme::teal())
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .child(count.to_string())
}

// ---------------------------------------------------------------------------
// Key / value rows
// ---------------------------------------------------------------------------

pub fn key_value(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    mono: bool,
) -> Div {
    let mut value_element = div()
        .flex_1()
        .min_w_0()
        .text_right()
        .text_size(if mono { px(11.) } else { theme::BODY_SIZE })
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::ink())
        .child(value.into());
    if mono {
        value_element = value_element.font_family(theme::mono_font());
    }
    div()
        .flex()
        .flex_row()
        .items_baseline()
        .justify_between()
        .gap(px(14.))
        .py(px(9.))
        .border_b_1()
        .border_color(rgb(0xedf2f3))
        .text_size(theme::BODY_SIZE)
        .child(
            div()
                .flex_shrink_0()
                .text_color(theme::muted())
                .child(label.into()),
        )
        .child(value_element)
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

pub fn primary_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id)
        .primary()
        .label(label)
        .h(px(34.))
        .px(px(14.))
        .rounded(px(9.))
        .text_size(theme::BODY_SIZE)
}

pub fn danger_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id)
        .danger()
        .label(label)
        .h(px(34.))
        .px(px(14.))
        .rounded(px(9.))
        .text_size(theme::BODY_SIZE)
}

/// White button with a hairline border (the old `outline-button`).
pub fn outline_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id)
        .ghost()
        .label(label)
        .h(px(34.))
        .px(px(14.))
        .rounded(px(9.))
        .border_1()
        .border_color(theme::line_strong())
        .bg(theme::panel())
        .text_color(theme::ink_soft())
        .text_size(theme::BODY_SIZE)
}

/// Subtle text-only button used in the titlebar.
pub fn ghost_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id)
        .ghost()
        .label(label)
        .h(px(32.))
        .px(px(10.))
        .rounded(px(9.))
        .text_color(theme::ink_soft())
        .text_size(theme::SMALL_SIZE)
}

/// Compact bordered button used inside section headers.
pub fn small_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id)
        .ghost()
        .label(label)
        .h(px(28.))
        .px(px(9.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme::line_strong())
        .bg(theme::panel())
        .text_color(theme::ink_soft())
        .text_size(theme::TINY_SIZE)
}

pub fn icon_button(id: impl Into<ElementId>, icon: IconName) -> Button {
    Button::new(id)
        .ghost()
        .icon(icon)
        .h(px(32.))
        .w(px(32.))
        .p(px(0.))
        .rounded(px(8.))
        .text_color(theme::ink_soft())
}

// ---------------------------------------------------------------------------
// Form controls
// ---------------------------------------------------------------------------

/// A labelled form field with optional hint text.
pub fn form_field(
    label: impl Into<SharedString>,
    hint: Option<impl Into<SharedString>>,
    control: impl IntoElement,
) -> Div {
    let mut field = column()
        .gap(px(6.))
        .child(
            div()
                .text_size(theme::SMALL_SIZE)
                .font_weight(FontWeight::BOLD)
                .text_color(theme::ink_soft())
                .child(label.into()),
        )
        .child(control);
    if let Some(hint) = hint {
        field = field.child(
            div()
                .text_size(theme::TINY_SIZE)
                .text_color(theme::muted())
                .child(hint.into()),
        );
    }
    field
}

/// Standard single-line text input styled like the old UI.
pub fn text_input(state: &gpui::Entity<InputState>) -> Input {
    Input::new(state)
        .h(px(36.))
        .px(px(11.))
        .rounded(px(9.))
        .border_1()
        .border_color(rgb(0xcfdee1))
        .bg(rgb(0xfbfdfd))
        .text_size(theme::BODY_SIZE)
}

// ---------------------------------------------------------------------------
// Modal scaffolding
// ---------------------------------------------------------------------------

/// Full-window dimmed backdrop that swallows clicks behind the dialog.
pub fn modal_backdrop() -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(px(28.))
        .bg(theme::backdrop())
        .occlude()
}

/// Dialog surface. Width is fixed; height is constrained by the caller.
pub fn modal_card(width: f32) -> Div {
    let mut card = div()
        .flex()
        .flex_col()
        .w(px(width))
        .max_h(px(850.))
        .overflow_hidden()
        .rounded(px(20.))
        .border_1()
        .border_color(rgba(0xc5d6dae6))
        .bg(rgb(0xf8fbfc));
    card.style().box_shadow = Some(vec![box_shadow(0., 26., 80., 0., rgba(0x15323e47))]);
    card
}

// The card paints the surface. GPUI's overflow clipping is rectangular, so
// backgrounds on the heading or footer would cover the card's rounded corners.
pub fn modal_heading(title: impl Into<SharedString>, description: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_row()
        .items_start()
        .justify_between()
        .gap(px(18.))
        .px(px(24.))
        .pt(px(20.))
        .pb(px(17.))
        .border_b_1()
        .border_color(theme::line())
        .child(
            column()
                .gap(px(5.))
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::ink())
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_size(theme::SMALL_SIZE)
                        .text_color(theme::muted())
                        .child(description.into()),
                ),
        )
}

/// Scrollable dialog body.
pub fn modal_body() -> gpui::Stateful<Div> {
    div()
        .id("modal-body")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(24.))
        .py(px(20.))
}

pub fn modal_footer() -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_end()
        .gap(px(8.))
        .px(px(24.))
        .py(px(14.))
        .border_t_1()
        .border_color(theme::line())
}

/// Nested white card used inside dialog bodies (`form-section`).
pub fn form_section() -> Div {
    div()
        .mb(px(18.))
        .p(px(16.))
        .rounded(px(14.))
        .border_1()
        .border_color(theme::line())
        .bg(theme::panel())
}

pub fn form_section_title(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
) -> Div {
    div()
        .flex()
        .flex_row()
        .items_baseline()
        .justify_between()
        .gap(px(10.))
        .mb(px(13.))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::ink())
                .child(title.into()),
        )
        .child(
            div()
                .text_size(theme::TINY_SIZE)
                .text_color(theme::muted())
                .child(description.into()),
        )
}

// ---------------------------------------------------------------------------
// Misc
// ---------------------------------------------------------------------------

/// Error banner used by forms and the detail header.
pub fn error_banner(message: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        .py(px(9.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme::error_border())
        .bg(theme::error_bg())
        .text_color(theme::red_text())
        .text_size(theme::SMALL_SIZE)
        .child(Icon::new(IconName::CircleAlert).size(px(15.)))
        .child(message.into())
}

/// Small inline hint under a control.
pub fn field_hint(message: impl Into<SharedString>) -> Div {
    div()
        .mt(px(6.))
        .text_size(theme::TINY_SIZE)
        .text_color(theme::muted())
        .child(message.into())
}

/// Empty-state block used in lists and cards.
pub fn empty_hint(title: impl Into<SharedString>, hint: impl Into<SharedString>) -> Div {
    column()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .min_h(px(120.))
        .p(px(20.))
        .text_size(theme::BODY_SIZE)
        .text_color(theme::muted())
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::ink_soft())
                .child(title.into()),
        )
        .child(div().text_size(theme::SMALL_SIZE).child(hint.into()))
}

/// Element id helper for repeated interactive rows.
pub fn element_id(prefix: &str, id: impl std::fmt::Display) -> ElementId {
    ElementId::Name(SharedString::from(format!("{prefix}-{id}")))
}
