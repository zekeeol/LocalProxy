//! Design tokens for the native LocalProxy UI.
//!
//! The palette, radii and typography mirror the original Electron UI
//! (see `src/index.css` in git history) so the rewrite keeps the same look:
//! a soft blue-grey canvas, white cards, teal accents and a compact
//! 12–14px type scale.

use gpui::{px, rgb, rgba, App, Pixels, Rgba};
use gpui_kit::component::theme::Theme;

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

/// App background behind the sidebar and content pane.
pub fn canvas() -> Rgba {
    rgb(0xedf4f6)
}

/// White cards, dialogs and the titlebar.
pub fn panel() -> Rgba {
    rgb(0xffffff)
}

/// Very light teal-tinted panel used for nested sections.
pub fn panel_soft() -> Rgba {
    rgb(0xf7fbfc)
}

/// Slightly deeper teal panel, used for chips and icon backgrounds.
pub fn panel_teal() -> Rgba {
    rgb(0xe3f1f2)
}

/// Main text color.
pub fn ink() -> Rgba {
    rgb(0x102436)
}

/// Secondary text color.
pub fn ink_soft() -> Rgba {
    rgb(0x385265)
}

/// Muted labels and descriptions.
pub fn muted() -> Rgba {
    rgb(0x718694)
}

/// Hairline borders.
pub fn line() -> Rgba {
    rgb(0xd8e4e7)
}

/// Stronger borders (sidebar separator, inputs).
pub fn line_strong() -> Rgba {
    rgb(0xc5d6da)
}

// ---------------------------------------------------------------------------
// Brand
// ---------------------------------------------------------------------------

/// Primary teal used for accents, active states and primary buttons.
pub fn teal() -> Rgba {
    rgb(0x0f766e)
}

/// Brighter teal for hover states.
pub fn teal_bright() -> Rgba {
    rgb(0x0d9488)
}

/// Deep teal for pressed states.
pub fn teal_dark() -> Rgba {
    rgb(0x0b625c)
}

/// Light teal tint (icon squares, tags).
pub fn teal_tint() -> Rgba {
    rgb(0xe3f1f2)
}

/// Tag background.
pub fn tag_bg() -> Rgba {
    rgb(0xdcf3f0)
}

/// Tag border.
pub fn tag_border() -> Rgba {
    rgb(0xb7dedb)
}

/// Cyan used for focus rings / links.
pub fn cyan() -> Rgba {
    rgb(0x0284c7)
}

// ---------------------------------------------------------------------------
// Status colors
// ---------------------------------------------------------------------------

pub fn green() -> Rgba {
    rgb(0x219653)
}

pub fn green_tint() -> Rgba {
    rgb(0xdef4e4)
}

pub fn green_text() -> Rgba {
    rgb(0x257344)
}

pub fn amber() -> Rgba {
    rgb(0xb87812)
}

pub fn amber_tint() -> Rgba {
    rgb(0xfff1d9)
}

pub fn amber_text() -> Rgba {
    rgb(0x9b650f)
}

pub fn red() -> Rgba {
    rgb(0xc34649)
}

pub fn red_tint() -> Rgba {
    rgb(0xffebeb)
}

pub fn red_text() -> Rgba {
    rgb(0xa73b40)
}

pub fn stopped_tint() -> Rgba {
    rgb(0xedf2f4)
}

pub fn stopped_text() -> Rgba {
    rgb(0x70818a)
}

// ---------------------------------------------------------------------------
// Surfaces
// ---------------------------------------------------------------------------

/// Sidebar background.
pub fn sidebar() -> Rgba {
    rgb(0xe4f0f2)
}

/// Content pane background.
pub fn content_bg() -> Rgba {
    rgb(0xf5f9fa)
}

/// Dimmed backdrop behind modal dialogs.
pub fn backdrop() -> Rgba {
    rgba(0x11243254)
}

/// White card on top of the canvas.
pub fn card_border() -> Rgba {
    rgb(0xdee9eb)
}

/// Endpoint card background.
pub fn endpoint_bg() -> Rgba {
    rgb(0xf8fbfb)
}

/// Endpoint card border.
pub fn endpoint_border() -> Rgba {
    rgb(0xdbe7e9)
}

/// Green route banner background.
pub fn route_bg() -> Rgba {
    rgb(0xeffaf6)
}

/// Error banner background.
pub fn error_bg() -> Rgba {
    rgb(0xfff4f4)
}

/// Error banner border.
pub fn error_border() -> Rgba {
    rgb(0xf2cfd0)
}

// ---------------------------------------------------------------------------
// Typography helpers
// ---------------------------------------------------------------------------

pub fn mono_font() -> &'static str {
    "SF Mono"
}

pub const HEADING_SIZE: Pixels = px(14.);
pub const BODY_SIZE: Pixels = px(12.);
pub const SMALL_SIZE: Pixels = px(11.);
pub const TINY_SIZE: Pixels = px(10.);

// ---------------------------------------------------------------------------
// Component sizing
// ---------------------------------------------------------------------------

pub fn titlebar_height() -> Pixels {
    px(62.)
}

pub fn sidebar_width() -> Pixels {
    px(320.)
}

pub fn card_radius() -> Pixels {
    px(18.)
}

pub fn control_radius() -> Pixels {
    px(9.)
}

/// Install the LocalProxy palette into the GPUI Kit component theme.
///
/// Our own elements use the token functions above directly; the kit
/// components (Input, Button, Switch, Select, …) read these theme fields, so
/// they inherit the same look without per-call styling.
pub fn install(cx: &mut App) {
    Theme::update(cx, |theme| {
        theme.font_family = ".SystemUIFont".into();
        theme.font_size = px(13.);
        theme.mono_font_family = mono_font().into();
        theme.mono_font_size = px(11.);
        theme.radius = control_radius();
        theme.radius_lg = px(20.);
        theme.shadow = false;
        theme.focus_ring = true;

        theme.background = panel().into();
        theme.foreground = ink().into();
        theme.border = line().into();
        theme.input = line_strong().into();
        theme.muted = panel_soft().into();
        theme.muted_foreground = muted().into();
        theme.secondary = panel_soft().into();
        theme.secondary_foreground = ink_soft().into();
        theme.accent = panel_teal().into();
        theme.accent_foreground = teal().into();
        theme.popover = panel().into();
        theme.popover_foreground = ink().into();
        theme.ring = cyan().into();
        theme.caret = teal().into();
        theme.selection = rgba(0x0f766e2e).into();

        theme.primary = teal().into();
        theme.primary_hover = teal_bright().into();
        theme.primary_active = teal_dark().into();
        theme.primary_foreground = panel().into();

        theme.button = panel().into();
        theme.button_hover = rgb(0xf1f7f8).into();
        theme.button_active = rgb(0xe7f1f2).into();
        theme.button_foreground = ink_soft().into();

        theme.button_primary = teal().into();
        theme.button_primary_hover = teal_bright().into();
        theme.button_primary_active = teal_dark().into();
        theme.button_primary_foreground = panel().into();

        theme.button_danger = red_tint().into();
        theme.button_danger_hover = rgb(0xffdddd).into();
        theme.button_danger_active = rgb(0xffd0d0).into();
        theme.button_danger_foreground = red_text().into();

        theme.danger = red().into();
        theme.danger_foreground = red_text().into();
        theme.success = green().into();
        theme.success_foreground = green_text().into();
        theme.warning = amber().into();
        theme.warning_foreground = amber_text().into();
        theme.info = cyan().into();
        theme.info_foreground = panel().into();

        theme.scrollbar = rgba(0x3852651f).into();
        theme.scrollbar_thumb = rgba(0x38526533).into();
        theme.scrollbar_thumb_hover = rgba(0x38526559).into();
    });
}
