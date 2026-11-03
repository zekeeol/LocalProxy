# gpui-kit 0.7.3 cheat sheet (macOS, light theme, custom styling)

> Historical research for GPUI Kit 0.7.3; this application now uses 0.8.1. Check the current crate APIs and `gpui-app/rust-toolchain.toml` before applying these examples.

Verified against the cached sources. Paths below are relative to
`~/.cargo/registry/src/index.crates.io-*/`.

| Layer | Crate | Import |
| --- | --- | --- |
| GPUI | `gpui-pre` 0.3.9 | `gpui_kit::*` |
| behavior | `gpui-base` 0.7.3 | `gpui_kit::base` |
| components | `gpui-component` 0.7.3 | `gpui_kit::component` |
| icons | `gpui-kit-assets` 0.7.3 | `gpui_kit::assets` |

`gpui-kit = "0.7.3"`; add `features = ["test-support"]` under `[dev-dependencies]`
for `#[gpui_kit::test]`.

Snippets assume `use gpui_kit::*; use gpui_kit::prelude::*;` plus explicit
`gpui_kit::component::…` imports, and the `c(hex)` helper from §3.4. A blanket
`use gpui_kit::component::*;` also works but makes `Size` (geometry vs. component) and
`InputEvent` (IME vs. input) ambiguous — prefer named imports.

---

## 1. Bootstrap

```rust
pub fn application() -> gpui::Application;                 // gpui-kit/src/lib.rs:195
pub fn init(cx: &mut App);                                 // gpui-kit/src/lib.rs:202
pub fn open_window<V: Render>(
    options: WindowOptions,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> Result<(AnyWindowHandle, Entity<V>)>;                 // gpui-kit/src/lib.rs:176-191
```

- `application()` = `gpui_platform::application()` = `Application::with_platform(current_platform(false))`
  (`gpui-pre-platform/src/gpui_platform.rs:15-24`). Chain `.with_assets(..)` before `.run`.
- `init(cx)` = `gpui_component::init` (component feature is default):
  `theme::init` (installs `Theme` **and calls `Theme::change(Light)`**) → `global_state` →
  `inspector` (debug) → `root::init` (registers the component `WindowState` root plugin) →
  `gpui_base::init` → `input`, `diff`, `date_picker`, `dock`, `sheet`, `list`, `command`,
  `carousel`, `notification`, `popover`, `questionnaire`, `menu`, `table`, `tooltip`
  (`gpui-component/src/lib.rs:132-153`).
- `init` registers **no** asset source and binds **no** `cmd-q`/`cmd-w`/`alt-f4`
  (`gpui-kit/tests/root.rs:96-108`). Quit/close are application-owned.
- Because `init` ends with `Theme::change(Light)`, install your palette **after** `init`.

### `open_window` auto-wraps the view in a `Root`

`cx.new(|cx| base::Root::new(view, window, cx))` (`gpui-kit/src/lib.rs:182-186`); the
returned entity is your view, not the root. The root type is `gpui_kit::base::Root`
(also re-exported as `gpui_kit::component::Root` — same type, `gpui-component/src/lib.rs:91`).
So dialogs / notifications / tooltips render even if your view never mentions them;
`Root::render` puts plugin overlays in `div().absolute().inset_0()` above content
(`gpui-base/src/root.rs:320-350`). `Root::view()` returns your entity (`:165`), and
`handle.downcast::<Root>()` returns `Option<WindowHandle<Root>>` (`gpui-pre/src/window.rs:7531`).

### Minimal `main.rs`

```rust
use gpui_kit::*;

struct AppView { /* inputs, selects, … */ }

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0xedf4f6)).text_color(rgb(0x102436)).child("LocalProxy")
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)   // "icons/*.svg"; AllAssets for the full catalog
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            install_palette(cx);                                     // §3.4, after init
            cx.set_window_appearance(Some(WindowAppearance::Light));  // force Aqua chrome

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(120.), px(120.)),
                    size: size(px(1200.), px(820.)),
                })),
                titlebar: Some(TitlebarOptions {
                    title: Some("LocalProxy".into()),
                    appears_transparent: true,                     // hide system titlebar
                    traffic_light_position: Some(point(px(16.), px(18.))),
                }),
                app_owns_titlebar_drag: true,                      // we call start_window_move()
                window_min_size: Some(size(px(980.), px(640.))),
                window_background: WindowBackgroundAppearance::Opaque,
                ..Default::default()
            };

            let (_handle, _view) = gpui_kit::open_window(options, cx, |_window, cx| {
                cx.new(|_cx| AppView {})
            })
            .expect("open main window");

            cx.on_window_closed(|cx, _id| {
                if cx.windows().is_empty() { cx.quit(); }
            })
            .detach();
        });
}
```

---

## 2. macOS window

### `WindowOptions` (`gpui-pre/src/platform.rs:2491`, `Default` at `:2662`)

| Field | Type | Default | Note |
| --- | --- | --- | --- |
| `window_bounds` | `Option<WindowBounds>` | `None` | `Windowed`/`Maximized`/`Fullscreen(Bounds<Pixels>)`, `:2629` |
| `titlebar` | `Option<TitlebarOptions>` | transparent=false | `:2692` |
| `focus`, `show`, `is_movable`, `is_resizable`, `is_minimizable` | `bool` | `true` | |
| `kind` | `WindowKind` | `Normal` | `Normal`/`PopUp`/`Floating`/`Dialog`/`AnchoredPopup(_)` |
| `app_owns_titlebar_drag` | `bool` | `false` | **`true` when you draw the titlebar** |
| `window_min_size` | `Option<Size<Pixels>>` | `None` | → `setContentMinSize` (`gpui-pre-macos/src/window.rs:1174`) |
| `window_background` | `WindowBackgroundAppearance` | `Opaque` | transparency, **not** light/dark (`:2766`) |
| `app_id`, `display_id`, `icon`, `tabbing_identifier`, `window_decorations` | | `None` | `window_decorations` is X11/Wayland only |

`TitlebarOptions { title: Option<SharedString>, appears_transparent: bool,
traffic_light_position: Option<Point<Pixels>> }`. `appears_transparent: true` adds
`NSFullSizeContentViewWindowMask`, `setTitlebarAppearsTransparent(YES)` and
`setTitleVisibility(Hidden)` (`gpui-pre-macos/src/window.rs:1007-1009`, `:1180-1183`).

### Custom titlebar: dragging

```rust
// (a) manual — works for any element with an id
h_flex()
    .id("titlebar")
    .h(px(52.))
    .w_full()
    .items_center()
    .bg(c(palette::PANEL))
    .border_b_1()
    .border_color(c(palette::LINE))
    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
    .on_double_click(|_, window, _| window.titlebar_double_click())
    .child("LocalProxy")

// (b) platform-managed drag region (needs .id())
div().id("titlebar-strip").h(px(52.)).w_full().window_control_area(WindowControlArea::Drag)
```

`start_window_move` `window.rs:3006`; `titlebar_double_click` `window.rs:6932`;
`window_control_area(WindowControlArea::{Drag,Close,Max,Min})` is on
`StatefulInteractiveElement` (`gpui-pre/src/elements/div.rs:1257`, enum `window.rs:751`);
`on_double_click` comes from `gpui_base::InteractiveElementExt`, impl'd only for
`Stateful<E>` (`gpui-base/src/event.rs:44-60`).

The kit's `TitleBar` does the same; `TitleBar::window_options()` is a ready base
(`gpui-component/src/title_bar.rs:60-95`):

```rust
let options = WindowOptions { window_min_size: Some(size(px(980.), px(640.))),
                              ..TitleBar::window_options() };
```

### Traffic lights: position only

```rust
#[cfg(target_os = "macos")]
window.set_traffic_light_position(point(px(16.), px(18.)));  // window.rs:3049
```

There is **no hide/show API**: `WindowOptions` has no such field and the macOS impl only
reads `traffic_light_position` (`gpui-pre-macos/src/window.rs:1626-1630`). If you draw your
own red/amber/green dots you must either keep the native ones (recommended — they keep
hover/minimise/zoom semantics) and position them, or push them off-screen with a negative
position (undocumented; verify visually).

### Minimize / zoom / close / title

```rust
window.minimize_window();     // window.rs:6635
window.zoom_window();         // window.rs:2993 (toggle zoom)
window.toggle_fullscreen();   // window.rs:6641
window.remove_window();       // window.rs:2384 — &mut Window, closes this window
window.is_maximized();        // window.rs:2478
window.window_bounds();       // window.rs:2524
window.set_window_title("LocalProxy");  // &mut Window, window.rs:3042
window.set_background_appearance(WindowBackgroundAppearance::Opaque); // &self, window.rs:3059
```

From outside the window: `handle.update(cx, |_, window, _| window.minimize_window()).ok()`
(`AnyWindowHandle::update`, `window.rs:7545`). There is no `cx.remove_window()`; app quit
is `cx.quit()` (`app.rs:1231`).

### Light appearance, activation policy

```rust
cx.set_window_appearance(Some(WindowAppearance::Light));  // app.rs:1712 — macOS only,
// forces NSApplication.appearance; None follows the system again. Does NOT touch Theme.
cx.set_activation_policy(ActivationPolicy::Regular);     // app.rs:1967 (Accessory = no Dock icon)
cx.activate(true);                                       // app.rs:1536
```

Default policy is `Regular` while windowed (`gpui-pre/src/platform.rs:165`), so a normal
windowed app needs neither call. Keep `Theme` in `ThemeMode::Light` yourself and never call
`Theme::sync_system_appearance`.

---

## 3. Theme

### Shape (`gpui-component/src/theme/mod.rs:147`)

```rust
pub struct Theme {
    pub colors: ThemeColor,          // ~120 public Hsla fields
    pub tokens: ThemeTokens,         // renderable Backgrounds (may be gradients)
    pub mode: ThemeMode,             // Light | Dark
    pub font_family: SharedString,   // ".SystemUIFont"
    pub font_size: Pixels,           // 16 — becomes the window rem size
    pub mono_font_family: SharedString, // macOS "Menlo"
    pub mono_font_size: Pixels,      // 13
    pub radius: Pixels,              // 6
    pub radius_lg: Pixels,           // 8
    pub shadow: bool,                // true
    pub focus_ring: bool,            // true
    pub transparent: Hsla,
    pub scrollbar_mode: ScrollbarMode,
    pub notification: NotificationSettings, // placement/margins/max_items/width/delivery
    pub list: ListSettings, pub sheet: SheetSettings, pub motion: MotionTokens,
    pub light_theme: Rc<ThemeConfig>, pub dark_theme: Rc<ThemeConfig>,
    pub highlight_theme: Arc<HighlightTheme>,
}
```

`Theme` derefs to `ThemeColor` (`mod.rs:220-232`), so `theme.background == theme.colors.background`.
It is a `Global`. Relevant public color fields (`theme/theme_color.rs:61-345`): `background`,
`foreground`, `popover(_foreground)`, `primary(_foreground/_hover/_active)`, `secondary*`,
`muted(_foreground)`, `accent(_foreground)`, `border`, `input`, `ring`, `selection`,
`overlay`, `caret`, `link*`, `danger*`, `success*`, `warning*`, `info*`,
`button(_hover/_active/_foreground)`, `button_primary*`, `button_danger*`,
`switch`, `switch_thumb`, `scrollbar`, `scrollbar_thumb(_hover)`, `title_bar(_border)`,
`sidebar*`, `list*`, `table*`, `tab*`, `skeleton`, `slider_*`, `progress_bar`, `chart_*`,
`group_box*`, `description_list_*`, `drop_target`, `drag_border`, `window_border`, palette
bases `red/green/blue/yellow/magenta/cyan(+_light)`.

Read: `use gpui_kit::component::ActiveTheme; let t = cx.theme();` — the trait is implemented
for `App` only (`mod.rs:48`). `Context<T>` derefs to `App`, so `cx.theme()` works in `render`
and in `cx.listener`; a bare `&mut Window` has no theme.

### Four write paths

```rust
Theme::update(cx, |t| { /* edit */ });     // mod.rs:288 — use this
Theme::global_mut(cx).radius = px(10.);    // mod.rs:251 — no token sync, no refresh
Theme::change(ThemeMode::Light, None, cx); // mod.rs:389 — RELOADS the default theme
Theme::sync_base(cx);                      // mod.rs:473 — push to gpui-base
```

`Theme::update` reconciles `colors` ↔ `tokens`, rebuilds the gpui-base projection
(scrollbars, resize handles), and refreshes every window. `global_mut` skips all of it — the
scrollbar keeps painting old colors until `Theme::sync_base(cx)` + `cx.refresh_windows()`.
Never call `Theme::change` (or `sync_system_appearance`) after installing a palette.

### Our palette

```rust
use gpui_kit::component::{Theme, ThemeColor};
use gpui_kit::{App, Hsla, px, rgb};

fn c(hex: u32) -> Hsla { rgb(hex).into() }   // Rgba -> Hsla

pub mod palette {
    pub const CANVAS: u32 = 0xedf4f6;  pub const PANEL: u32 = 0xffffff;
    pub const INK: u32 = 0x102436;     pub const INK_SOFT: u32 = 0x385265;
    pub const MUTED: u32 = 0x718694;   pub const LINE: u32 = 0xd8e4e7;
    pub const TEAL: u32 = 0x0f766e;    pub const TEAL_BRIGHT: u32 = 0x0d9488;
    pub const CYAN: u32 = 0x0284c7;    pub const GREEN: u32 = 0x219653;
    pub const AMBER: u32 = 0xb87812;   pub const RED: u32 = 0xc34649;
}

pub fn install_palette(cx: &mut App) {
    use crate::palette::*;
    let mut colors = *ThemeColor::light();      // theme_color.rs:549 (ThemeColor: Copy)

    colors.background = c(CANVAS);              colors.foreground = c(INK);
    colors.popover = c(PANEL);                  colors.popover_foreground = c(INK);
    colors.secondary = c(0xf1f6f7);             colors.secondary_foreground = c(INK);
    colors.secondary_hover = c(0xe8eff0);       colors.secondary_active = c(0xdfe9ea);
    colors.muted = c(0xeef4f5);                 colors.muted_foreground = c(MUTED);
    colors.accent = c(0xe6f2f1);                colors.accent_foreground = c(TEAL);
    colors.border = c(LINE);                    colors.input = c(LINE);
    colors.ring = c(TEAL_BRIGHT);               colors.caret = c(TEAL);
    colors.selection = c(TEAL).opacity(0.22);

    colors.primary = c(TEAL);                   colors.primary_foreground = c(0xffffff);
    colors.primary_hover = c(TEAL_BRIGHT);      colors.primary_active = c(0x0b5f59);
    // filled buttons read tokens.button_primary*, NOT primary (button.rs:996,1146,1230)
    colors.button_primary = c(TEAL);            colors.button_primary_foreground = c(0xffffff);
    colors.button_primary_hover = c(TEAL_BRIGHT);
    colors.button_primary_active = c(0x0b5f59);

    colors.danger = c(RED);                     colors.danger_foreground = c(0xffffff);
    colors.danger_hover = c(0xd25b5e);          colors.danger_active = c(0xa93a3d);
    colors.button_danger = c(RED);              colors.button_danger_foreground = c(0xffffff);
    colors.success = c(GREEN);                  colors.success_foreground = c(0xffffff);
    colors.warning = c(AMBER);                  colors.warning_foreground = c(0xffffff);
    colors.info = c(CYAN);                      colors.info_foreground = c(0xffffff);
    colors.link = c(CYAN);                      colors.link_hover = c(0x0369a1);

    colors.overlay = c(INK).opacity(0.35);      // dialog/notification backdrop
    colors.title_bar = c(PANEL);                colors.title_bar_border = c(LINE);
    colors.sidebar = c(PANEL);                  colors.sidebar_foreground = c(INK_SOFT);
    colors.sidebar_border = c(LINE);            colors.sidebar_accent = c(0xeef4f5);
    colors.sidebar_accent_foreground = c(TEAL);
    colors.sidebar_primary = c(TEAL);           colors.sidebar_primary_foreground = c(0xffffff);

    colors.switch = c(LINE);                    colors.switch_thumb = c(0xffffff);
    colors.scrollbar = Hsla::transparent_black();
    colors.scrollbar_thumb = c(0xc3d3d6).opacity(0.9);
    colors.scrollbar_thumb_hover = c(0x9fb4b8);

    Theme::update(cx, |theme| {
        theme.colors = colors;
        theme.radius = px(8.);
        theme.radius_lg = px(12.);
        theme.font_family = "Inter".into();        // must be installed/embedded (§3.5)
        theme.font_size = px(14.);                 // also the window rem size
        theme.mono_font_family = "JetBrains Mono".into();
        theme.mono_font_size = px(12.);
        theme.focus_ring = true;
        theme.shadow = true;
        theme.notification.width = px(360.);
    });
}
```

Anything not listed keeps the shipped shadcn grey/blue: `list_*`, `table_*`, `tab_*`,
`accordion`, `group_box*`, `description_list_*`, `skeleton`, `slider_*`, `progress_bar`,
`chart_*`, `drop_target`, `drag_border`. Restyle those components explicitly or override
the fields if you use them.

### Background vs. surface

The root plugin paints the window with `theme.tokens.background`
(`gpui-component/src/root.rs:444-452`) and the default `Dialog` surface uses the same token
(`dialog.rs:720`). So either

- set `colors.background = CANVAS` and give dialogs their panel color explicitly — `Dialog`
  is `Styled` (`dialog.rs:553`) and `refine_style` runs last, so `dialog.bg(c(PANEL))` wins; or
- set `background = PANEL` and paint the canvas yourself in the root view.

### Fonts and CJK

```rust
use std::borrow::Cow;

cx.text_system().add_fonts(vec![
    Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Regular.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/Inter-SemiBold.ttf").as_slice()),
]).expect("fonts");                                    // text_system.rs:295

Theme::update(cx, |t| t.font_family = "Inter".into()); // family name from the font file
```

`theme.font_family` is applied to the root surface and `theme.font_size` becomes the rem size
(`gpui-component/src/root.rs:440-452`), so both cascade. GPUI stores one family; glyph-level
fallback is the platform's (CoreText cascades to PingFang SC on macOS). For an explicit
cascade, set `TextStyle::font_fallbacks` (`gpui-pre/src/style.rs:449`) via `.font(Font {..})`
(`styled.rs:720-737`):

```rust
use gpui_kit::{Font, FontFallbacks};
let cjk = Font {
    family: "Inter".into(),
    fallbacks: Some(FontFallbacks::from_fonts(vec![
        "PingFang SC".into(), "Hiragino Sans GB".into(), "Noto Sans CJK SC".into(),
    ])),
    ..Default::default()                     // Font::default, text_system.rs:1311
};
div().font(cjk).size_full().children([/* … */])
```

A missing family does not crash: `resolve_font` falls back through `.ZedMono`, `.ZedSans`,
`Helvetica`, `Arial`… (`text_system.rs:370-388`) — so a typo silently renders in Helvetica.
Check with `cx.text_system().all_font_names()`.

### Tokens (`theme_tokens.rs`)

| Set | Values | Editable |
| --- | --- | --- |
| `RadiusTokens` | `radius/2`, `radius`, `radius_lg`, `radius*2`, `radius_full` (`theme/mod.rs:577`) | via `radius`/`radius_lg` |
| `SpacingTokens` | 2/4/8/12/16/24/32 px (`:142`) | **no** — `Theme::spacing_tokens()` returns the constant (`mod.rs:588`) |
| `TypographyTokens` | `xs 12/16`, `sm 14/20`, `md 16/24`, `lg 18/28`, `xl 20/28`, `mono_md 13/20` (`:175`) | sizes via `font_size`/`mono_font_size`; ratios fixed |
| `ShadowTokens` | `elevations(transparent.alpha(0.18))` when `shadow` (`mod.rs:601`) | on/off only |

JSON themes exist (`ThemeRegistry::load_themes_from_str`, `registry.rs:151` +
`Theme::apply_config`, `schema.rs:1064`) but any later `Theme::change` replaces them;
`Theme::update` is simpler and survives.

---

## 4. Layout & styling

```rust
use gpui_kit::component::{ActiveTheme, StyledExt, ThemeStyled, h_flex, v_flex};
use gpui_kit::prelude::*;                   // Styled, ParentElement, InteractiveElement, …
use gpui_kit::{div, hsla, point, px, relative, rems, rgb, rgba, size};
```

`prelude` = `AppContext, Context, Element, InteractiveElement, IntoElement, ParentElement,
Refineable, Render, RenderOnce, StatefulInteractiveElement, Styled, StyledImage,
VisualContext, FluentBuilder` (`gpui-pre/src/prelude.rs:5-9`). `Sizable`, `Disableable`,
`Selectable`, `ButtonVariants`, `StyledExt`, `ThemeStyled` are **not** in it.

```rust
div()        // block/flex container
h_flex()     // div().flex().flex_row().items_center()   gpui-base/src/styled.rs:38
v_flex()     // div().flex().flex_col()                  gpui-base/src/styled.rs:46
```

`h_flex` centers on the cross axis; `v_flex` does not (children stretch) — a full-height
column in a row needs `.h_full()`.

Units/colors: `px(16.)` (`geometry.rs:3748`), `rems(1.)` (`:3735`),
`relative(0.5)` (`:3717`), `rgb(0x0f766e)` (`color.rs:14`), `rgba(0x0f766e80)` (`:18`),
`hsla(0.55,0.4,0.14,0.35)` (`color.rs:424`), `rgb(..).into()` → `Hsla` (`color.rs:677`),
`.opacity(f32)`/`.alpha(f32)` on `Hsla`.

**Numeric suffixes are rem-based**, not px: `.p_4()` = 1rem = 16px at the default font size,
`.gap_2()` = 8px, `.text_sm()` = 14/20. They scale with `theme.font_size`; use `px(...)` for
our fixed spacing.

| Method | Notes |
| --- | --- |
| `.flex()/.flex_row()/.flex_col()/.flex_wrap()`, `.items_*()`, `.justify_*()` | `Styled` |
| `.flex_grow(1.)/.flex_grow_1()`, `.flex_shrink_0()`, `.flex_none()` | |
| `.w()/.h()/.size()`, `.w_full()/.h_full()`, `.min_w_0()`, `.max_h()` | macro-generated (`gpui-pre-macros/src/styles.rs:843-925`) |
| `.gap()/.gap_x()/.gap_y()`, `.gap_2()`; `.p_*/.px_*/.py_*/.m_*` | rem ramp |
| `.rounded(px(8.))`, `.rounded_lg()`, `.rounded_t_*()` | takes `AbsoluteLength` |
| `.border_1()`, `.border_b()`, `.border_color(..)` | `border_1` = 1px (`styles.rs:1329`) |
| `.bg(impl Into<Fill>)` (`styled.rs:490`), `.text_color(impl Into<Hsla>)` (`:514`) | |
| `.text_size(px(13.))`, `.text_xs()/.text_sm()/.text_base()/.text_lg()` | `styled.rs:538-590` |
| `.font_weight(FontWeight::SEMIBOLD)`, `.font_bold()`, `.font_family("Inter")`, `.font(Font)` | |
| `.line_height(px(20.))`, `.line_height(relative(1.5))` | `DefiniteLength` |
| `.shadow_sm()/.shadow_md()/.shadow_lg()`, `.shadow(vec![..])` | `gpui_base::box_shadow(x,y,blur,spread,color)` |
| `.truncate()` | hidden+nowrap+ellipsis (`styled.rs:139`) |
| `.relative()/.absolute()`, `.inset_0()`, `.top_0()/.right_4()/.bottom_0()/.left(..)` | |
| `.overflow_hidden()`, `.overflow_x_hidden()` | `Styled` (no y-scroll here) |
| `.id("x")` | `InteractiveElement`; **required** before `.overflow_y_scroll()`, `.track_scroll()`, `.window_control_area()`, `.on_double_click()`, `.focusable()` |
| `.overflow_y_scroll()`, `.overflow_scroll()` | `Stateful<Div>` only (`div.rs:1535`, impl `:4080`) |
| `.on_click`, `.on_mouse_down(btn, ..)`, `.on_mouse_up`, `.on_mouse_down_out`, `.on_mouse_move`, `.on_hover` | `InteractiveElement` |
| `.hover(\|s\| s.bg(..))` | eager `f(StyleRefinement::default())` (`div.rs:842`); **a second call panics in debug** |
| `.cursor_pointer()/.cursor_text()/.cursor_not_allowed()/.cursor_grab()` | macro-generated |
| `.occlude()` | blocks clicks to elements behind (`div.rs:1250`) |
| `.child(..)/.children(..)/.when(..)/.when_some(..)/.map(..)` | `ParentElement` / `FluentBuilder` |

```rust
div().id("row").px_3().py_2().rounded(px(6.))
    .hover(|s| s.bg(cx.theme().accent))
    .child("Row")
```

Scrolling:

```rust
use gpui_kit::component::scroll::ScrollableElement as _;

div().id("main-scroll").size_full().overflow_y_scroll().child(long_content());
div().id("main-scroll").size_full().overflow_y_scrollbar().child(long_content()); // themed thumb
```

`Scrollable::render` adds `.overflow_y_scroll()`, `.lock_scroll_axis()` and a `Scrollbar`
overlay (`gpui-component/src/scroll/scrollable.rs:122-180`).

Images:

```rust
use gpui_kit::{img, ImageSource};

img(std::path::PathBuf::from("/Users/me/logo.png")).w(px(24.)).h(px(24.)) // local file
img("icons/globe.svg").w(px(16.))                                         // embedded asset
img(ImageSource::Custom(std::sync::Arc::new(|window, cx| { /* .. */ })))
```

`ImageSource` = `Resource(Resource) | Render(Arc<RenderImage>) | Image(Arc<Image>) |
Custom(fn)` (`gpui-pre/src/elements/img.rs:42`). `From<&str>`: URI → `Resource::Uri`,
anything else → `Resource::Embedded` (`:63-71`); `From<PathBuf>/<&Path>` → `Resource::Path`,
read with `fs::read` (`:89-104`, `:634`). **So `img("/tmp/a.png")` is an asset lookup, not a
file read.**

Asset registration:

```rust
gpui_kit::application().with_assets(gpui_kit::assets::Assets)  // 106 component icons
// .with_assets(gpui_kit::assets::AllAssets)                   // full Lucide catalog
// .with_assets(MyAssets)                                      // own AssetSource impl
```

`AllAssets` embeds all icons (`gpui-kit-assets/src/native_assets.rs:7-11`);
`icon_assets!(AppAssets, [Search, Check])` generates a source with only those
(`gpui-kit-assets/src/lib.rs:70-91`). Asset names are the SVG file names: `x.svg` → `X`,
`circle-alert.svg` → `CircleAlert`, `settings-2.svg` → `Settings2`
(`gpui-kit-assets/build.rs:26-38`).

---

## 5. Components

### 5.1 Button

```rust
use gpui_kit::component::{Disableable, button::{Button, ButtonVariants as _}};

Button::new("save")                     // id: impl Into<ElementId>
    .label("Save")                      // impl Into<SharedString>; omit for icon-only
    .primary()                          // primary/secondary/danger/warning/success/info/
                                        // ghost/link/text/outline/custom
    .icon(IconName::Check)              // impl Into<ButtonIcon>; IconName works
    .loading(self.saving)               // spinner + ignores clicks
    .disabled(!self.dirty)              // Disableable
    .tooltip("Save changes (⌘S)")
    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
        this.save();
        cx.notify();
    }))
```

Sizes: `Size::{XSmall, Small, Medium(default), Large}` via `Sizable` — `.xsmall()`, `.small()`,
`.large()`, `.with_size(px(40.))`. No `.medium()`. `Button` implements `Styled` (`button.rs:590`),
so `.w(px(160.))`, `.h(px(36.))`, `.rounded(px(8.))` refine after the variant style. One-off
colors: `Button::custom(ButtonCustomVariant::new(cx).color(..).foreground(..).hover(..).active(..))`
(`button.rs:40-140`). Drop-down: `.dropdown_menu(|menu, _, _| menu.menu("Save", Box::new(Save)))`
(`gpui-kit/tests/menu.rs:37-46`). Icon-only: add `.accessibility_label("…")`.

### 5.2 Input / InputState

```rust
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};

struct Form { url: Entity<InputState>, note: Entity<TextareaState>, _subs: Vec<Subscription> }

impl Form {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url = cx.new(|cx| InputState::new(window, cx)          // state.rs:10922
            .placeholder("https://api.example.com")                // :904
            .default_value("http://127.0.0.1:8080"));              // :1331
        let note = cx.new(|cx| TextareaState::new(window, cx).rows(4));
        let subs = vec![cx.subscribe(&url, |this: &mut Self, _s, ev: &InputEvent, cx| {
            match ev {
                InputEvent::Change => { /* value changed */ }
                InputEvent::PressEnter { secondary, shift } => {
                    let _ = (secondary, shift);
                    this.submit(cx);
                }
                InputEvent::Focus | InputEvent::Blur => {}
            }
            cx.notify();
        })];
        Self { url, note, _subs: subs }
    }
}

// render
Input::new(&self.url)
    .id("url")
    .w_full().h(px(38.))
    .prefix(div().child(Icon::new(IconName::Link)))
    .suffix(div().child("…"))
    .disabled(false)
    .readonly(false)
```

- Values: `read(cx).value() -> SharedString` (`state.rs:1347`), `set_value(v, window, cx)`
  (`:1038`), `selected_value()` (`:1356`), `text() -> &Rope` (`:1377`),
  `set_selected_range(range, cx)` (`:3150`).
- Password: `InputState::new(window, cx).masked(true)` (`:10946`); `toggle_masked(window, cx)`
  (`:732`); `value()` still returns plaintext.
- Multi-line: `TextareaState::new(window, cx)` + `Textarea::new(&state)`; `.rows(n)`,
  `.auto_grow(min, max)` (`:11144`, `:11178`). `InputState` is single-line by type.
- Number-ish: `InputState::new(..).step(1.).min(0.).max(65535.)` + `NumberInput`
  (`state.rs:10996-11045`, `input/number_input.rs`).
- Disabled/readonly on the element (`input.rs:346`, `:356`) or state (`set_disabled`/`set_readonly`).
- Focus: `state.read(cx).focus_handle(cx).focus(window, cx)` or `InputState::focus(window, cx)` (`:1405`).
- **No `.on_change` / `.on_submit` on `Input`** — subscribe to the state and match `InputEvent`
  (above; the kit does exactly this, `tests/input/editing.rs:44-53`).
- Validation: no error UI. `InputState::validate(|text, cx| bool)` (`:10974`) rejects edits;
  paint your own error row and/or `InputGroup::new("id").invalid(true)` (`input/group.rs:90`).
- `cleanable(bool)` is on `Input` (`input.rs:317`), not on the state.
- `component::input::InputEvent` (`Change | PressEnter { secondary, shift } | Focus | Blur`,
  `state.rs:127-132`) is **not** `gpui::InputEvent` (IME). Alias one.

### 5.3 Select / Combobox / dropdown menus

```rust
use gpui_kit::component::{
    IndexPath,
    select::{Select, SelectEvent, SelectState},
    searchable_list::SearchableListItem,
};

#[derive(Clone)]
struct Profile { id: u64, name: SharedString }

impl SearchableListItem for Profile {          // searchable_list/delegate.rs:8
    type Value = u64;
    fn title(&self) -> SharedString { self.name.clone() }
    fn value(&self) -> &u64 { &self.id }
}

// inside fn new(window: &mut Window, cx: &mut Context<Self>)
let profiles = cx.new(|cx| {
    SelectState::new(
        vec![Profile { id: 1, name: "Work".into() }, Profile { id: 2, name: "Home".into() }],
        Some(IndexPath::new(0)),       // default selection
        window,
        cx,
    )
    .searchable(true)
});
let _sub = cx.subscribe(&profiles, |this: &mut Self, _s, ev: &SelectEvent<Vec<Profile>>, cx| {
    if let SelectEvent::Confirm(Some(id)) = ev { this.selected = Some(*id); cx.notify(); }
});

// render — no on_change; the trigger is Styled
Select::new(&self.profiles)
    .id("profile-select")
    .title_prefix("Profile: ")
    .placeholder("Choose…")
    .menu_width(px(260.))
    .w(px(240.))
    .disabled(false)
```

- `SelectState::new(delegate, Option<IndexPath>, &mut Window, &mut Context<Self>)` (`select.rs:152`);
  `Select::new(&Entity<SelectState<D>>)` (`:649`).
- No `SelectItem` in 0.7.3: items are any `SearchableListItem`; `Vec<T>` is already a delegate
  (`searchable_list/vec.rs:43`), `SearchableVec<T>` filters incrementally.
- Programmatic: `set_selected_index(Some(IndexPath::new(2)), window, cx)` (`:309`),
  `set_selected_value(&v, window, cx)` (`:337`).
- Reads: `selected_value() -> Option<&Value>` (`:372`), `selected_index()` (`:367`), `focus()` (`:377`).
- Emits `SelectEvent::Confirm(..)` and `DismissEvent` (`:71`, `:131`); Esc/outside/blur/Enter dismiss.
- Multi-select: `ComboboxState::new(delegate, Vec<IndexPath>, window, cx)` +
  `Combobox::new(&state)` (`combobox.rs:145`, `:769`), `.selected_values()` (`:299`).
- Menus without state: `Button::dropdown_menu` or
  `DropdownButton::new("save").button(Button::new("s").label("Save")).dropdown_menu(|m, _, _| m)`
  (`button/dropdown_button.rs:79-93`).

### 5.4 Switch and Checkbox

```rust
use gpui_kit::component::{checkbox::Checkbox, switch::Switch};

Switch::new("proxy-enabled").checked(self.enabled).label("Enable proxy")
    .color(cx.theme().primary)                 // track color when on (default primary)
    .on_change(cx.listener(|this, checked: &bool, _, cx| {
        this.enabled = *checked; cx.notify();
    }))

Checkbox::new("agree").label("Remember").checked(self.agree)
    .on_change(cx.listener(|this, checked: &bool, _, cx| {
        this.agree = *checked; cx.notify();
    }))
```

Both are **controlled**: the widget never flips itself; you must write the value and
`cx.notify()` (kit test `controls.rs:328`). `on_click` is an alias of `on_change`
(`switch.rs:79-92`, `checkbox.rs:100-114`). Disable with `Disableable`.

### 5.5 Modal / Dialog

The kit has **no `Modal` type**. Available:

- `window.open_dialog(cx, |dialog, window, cx| …)` — `WindowExt::open_dialog`
  (`gpui-component/src/window_ext.rs:75`). The builder is a *render* closure and **may run
  again** on layer rebuild: create `InputState`s before opening, never inside it (`:40-56`).
- `window.open_alert_dialog(cx, |a, _, _| a.warning().title("…").description("…"))`
  (`window_ext.rs:101`, `dialog/alert_dialog.rs:80`).
- `window.open_sheet` / `open_sheet_at` / `close_sheet` (`:20-38`), `close_dialog(cx)`,
  `has_active_dialog(cx)`, `close_all_dialogs(cx)`.
- Defaults (`DialogProps::default`, `dialog.rs:217`): width `px(448.)`, overlay on,
  `overlay_closable: true`, `keyboard: true` (Esc), close button, `margin_top: None`.
- Focus: the dialog shell is focused on open with a focus trap; the previously focused
  handle is saved at open and restored on close (`gpui-component/src/root.rs:247-275`).

**Centering:** horizontal is `(viewport - width) / 2`, vertical is
`margin_top.unwrap_or(viewport.height / 10.)` (`dialog.rs:613-620`) — 10% from the top, *not*
centered. For a fixed-height panel pass a computed offset; for unknown heights use the
custom overlay below.

```rust
use gpui_kit::component::{WindowExt as _, dialog::{DialogAction, DialogClose, DialogFooter}};

let panel_h = px(420.);
window.open_dialog(cx, move |dialog, window, _| {
    let y = (window.viewport_size().height - panel_h) / 2.;
    dialog
        .title("Edit profile")
        .margin_top(y)
        .w(px(520.))
        .bg(c(palette::PANEL))          // wins: refine_style runs last (dialog.rs:553)
        .overlay_closable(true)
        .child(Input::new(&draft).id("name"))
        .footer(DialogFooter::new()
            .child(DialogClose::new().trigger(|b| b.label("Cancel")))
            .child(DialogAction::new().child(Button::new("ok").label("Save").primary())))
        .on_ok(|_, _, _| true)          // false keeps it open
});
```

Custom centered overlay (full control, unknown height):

```rust
fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    div().relative().size_full()
        .child(self.page(cx))
        .when_some(self.modal.take(), |this, modal| {
            this.child(
                div().id("modal-backdrop")
                    .absolute().inset_0()
                    .flex().items_center().justify_center()
                    .bg(c(palette::INK).opacity(0.35))
                    .occlude()                       // block clicks behind
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                        this.modal = None; cx.notify();
                    }))
                    .child(
                        div().id("modal-card")
                            .w(px(520.)).rounded(px(12.))
                            .bg(c(palette::PANEL))
                            .border_1().border_color(c(palette::LINE))
                            .shadow_lg().p_6()
                            .occlude()
                            // clicks inside must not reach the backdrop
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(modal),
                    ),
            )
        })
}
```

`cx.stop_propagation()` = `App::stop_propagation` (`app.rs:2645`), reachable through
`Context<T>`; inverse is `cx.propagate()`. `.occlude()` is on `InteractiveElement` (`div.rs:1250`).

### 5.6 Notification / toast

```rust
use gpui_kit::component::{WindowExt as _, notification::Notification};

window.push_notification(Notification::success("Connected"), cx);
window.push_notification(Notification::new()
    .title("Profile saved").message("Work profile updated")
    .icon(IconName::CircleCheck).autohide(true), cx);   // false = sticky
window.clear_notifications(cx);
window.remove_notification::<MyTag>(cx);                // dedupe by id type
```

Builder: `.message`, `.title`, `.icon`, `.with_type(NotificationType)`, `.placement(Anchor::..)`,
`.autohide(bool)`, `.on_click`, `.action(|n, window, cx| Button::…)`:
**auto-hide is a hard-coded 5 s** with no per-notification timeout (`notification.rs:872`).
Placement/width/margins/max live in `theme.notification` (default `TopRight`, 382px, 10 items,
`notification.rs:537-575`). For a 3 s or bottom-center toast, draw it yourself:

```rust
fn toast(&mut self, msg: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) {
    self.toast = Some(msg.into());
    cx.notify();
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(std::time::Duration::from_secs(3)).await;
        this.update(cx, |this, cx| { this.toast = None; cx.notify(); }).ok();
    }).detach();
}

// render branch
.when_some(self.toast.clone(), |this, msg| {
    this.child(div().id("toast").absolute().bottom(px(24.)).left_0().right_0()
        .flex().justify_center()
        .child(div().px_4().py_2().rounded(px(10.))
            .bg(c(palette::INK)).text_color(c(palette::PANEL)).shadow_lg().child(msg)))
})
```

### 5.7 Tooltip, Tag, Spinner, Icon, Separator, Tabs, Label, Clipboard

```rust
// Tooltip: built into Button/Switch/Checkbox/Clipboard; there is no public
// "attach to any element" helper (ManagedTooltipExt is pub(crate), tooltip.rs:232).
Button::new("copy").ghost().icon(IconName::Copy).tooltip("Copy to clipboard")

// Tag / badge (Tag is the badge; no separate Badge type)
Tag::custom(c(0xe6f2f1), c(palette::TEAL), c(0xb8dcd8)).child("Connected") // tag.rs:177
Tag::success().child("Up")
Tag::secondary().outline().small().child("v0.7.3")   // .rounded(px(6.)) / .rounded_full()

Spinner::new()                                        // spinner.rs:20 (Loader + rotation)
    .small().color(cx.theme().primary)

Icon::new(IconName::Settings)                         // any IconNamed: impl Into<Icon>
    .with_size(gpui_kit::component::Size::Small)
    .text_color(cx.theme().muted_foreground)
IconName::Search                                      // renders bare too (RenderOnce)

Separator::horizontal()                               // separator.rs:40
Separator::vertical().color(cx.theme().border)
Separator::horizontal().dashed()

TabBar::new("settings-tabs")                          // controlled
    .selected_index(self.tab)
    .children([Tab::new().label("General"), Tab::new().label("Advanced")])
    .on_click(cx.listener(|this, ix: &usize, _, cx| { this.tab = *ix; cx.notify(); }))
// variants: .underline() .pill() .segmented() .outline()  (tab_bar.rs:81-105)

Label::new("LocalProxy").secondary("127.0.0.1:8080")  // label.rs:63

Clipboard::new("copy-key").value("sk-1234").tooltip("Copy")
    .accessibility_label("Copy API key")              // clipboard.rs:36

// Clipboard write anywhere
cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));  // app.rs:1750
let pasted = cx.read_from_clipboard().and_then(|i| i.text());
```

---

## 6. `IconName`: exact spellings

Two enums:

1. `gpui_kit::component::IconName` — only the ~106 default icons, generated from
   `gpui-kit-assets/default-icons.txt` (`gpui-kit-assets/build.rs:73-108`).
2. `gpui_kit::assets::IconName` — the full ~1830-icon catalog from `assets/icons/*.svg`
   (`build.rs:20-51`). Variant = file stem split on `-`, `_`, `.` with each part capitalized:
   `circle-alert` → `CircleAlert`, `x` → `X`, `refresh-cw` → `RefreshCw`, `settings-2` → `Settings2`.

Both implement `IconNamed` and are accepted by `Icon::new` / `Button::icon`. An icon outside
the default bundle only renders if the asset source has it → use `AllAssets` (or
`icon_assets!`), otherwise `Assets` reports "could not find asset".

| Wanted | `component::IconName` | with `AllAssets` |
| --- | --- | --- |
| globe / settings / play | `Globe`, `Settings`+`Settings2`, `Play` | same |
| stop / square | `Square` | `Square`, `CircleStop`, `SquareStop` |
| copy / plus / check | `Copy`, `Plus`, `Check` | same |
| trash / delete | `Delete` (no `Trash`) | `Delete`, `Trash` |
| refresh | `RefreshCw` (no `Refresh`) | `RefreshCw`, `RotateCw` |
| terminal | `SquareTerminal` | `Terminal`, `SquareTerminal` |
| alert-circle | `CircleAlert` (no `AlertCircle`) | `CircleAlert` |
| check-circle | `CircleCheck` (no `CheckCircle`) | `CircleCheck` |
| x / close | `Close`, `CircleX` (no `X`) | `X`, `Close`, `CircleX` |
| chevron-down | `ChevronDown` | same |
| more-horizontal | `Ellipsis` | `Ellipsis`, `EllipsisVertical` |
| pencil / edit | `Pencil` (no `Edit`) | `Pencil` |
| help | — | `CircleQuestionMark` (no `Help`) |
| save / key / list / layers / server / tag / activity | — | `Save`, `Key`+`KeyRound`+`KeySquare`, `List`, `Layers`, `Server`+`ServerCog`, `Tag`, `Activity` |
| shield / link / wifi / lock | — | `Shield`+`ShieldCheck`+`ShieldAlert`, `Link`, `Wifi`+`WifiOff`, `Lock`+`LockKeyhole` |
| arrow-right / search / eye / eye-off / external-link | `ArrowRight`, `Search`, `Eye`, `EyeOff`, `ExternalLink` | same |
| upload / download | — | `Upload`, `Download` |

Default subset (the only variants that exist on `component::IconName`): `ALargeSmall ArrowDown
ArrowLeft ArrowRight ArrowUp Asterisk Ban Battery BatteryCharging BatteryFull BatteryLow
BatteryMedium BatteryWarning Bell BookOpen Bot Building2 Calendar CaseSensitive ChartPie Check
ChevronDown ChevronLeft ChevronRight ChevronUp ChevronsUpDown CircleAlert CircleCheck CircleUser
CircleX Close Copy Cpu Dash Delete Ellipsis EllipsisVertical ExternalLink Eye EyeOff File FileText
Folder FolderClosed FolderOpen Frame GalleryVerticalEnd Github Globe HardDrive Heart HeartOff Inbox
Info Inspector LayoutDashboard Loader LoaderCircle Map Maximize MemoryStick Menu Mic Minimize Minus
Moon Network Palette PanelBottom PanelBottomOpen PanelLeft PanelLeftClose PanelLeftOpen PanelRight
PanelRightClose PanelRightOpen Pause Play Plus Redo Redo2 RefreshCw Replace ResizeCorner RotateCw
Search Settings Settings2 SortAscending SortDescending Square SquareTerminal Star StarFill StarOff
Sun ThumbsDown ThumbsUp TriangleAlert Undo Undo2 User WindowClose WindowMaximize WindowMinimize
WindowRestore`. Iterate `assets::IconName::ALL` for the rest.

---

## 7. Async and state (`gpui-pre` 0.3.9)

| Type | Use |
| --- | --- |
| `Context<T>` | synchronous entity+app state (render, listeners) |
| `&mut App` | app-level work in raw callbacks |
| `AsyncApp` | held across `.await` from `App::spawn` / `Context::spawn` |
| `AsyncWindowContext` | held across `.await` from `Context::spawn_in` (has a window) |

```rust
cx.spawn(async move |this, cx| { /* this: WeakEntity<T>, cx: &mut AsyncApp */ })
//   Context::spawn, app/context.rs:230
cx.spawn_in(window, async move |this, cx| { /* cx: &mut AsyncWindowContext */ })
//   Context::spawn_in, app/context.rs:690 — takes `&Window`
```

Both return `Task<R>`: a `Future` you must await, store, or `.detach()`
(`Task::detach`, `gpui-pre-scheduler-0.3.9/src/executor.rs:572`). Dropping a `Task` cancels it.

```rust
this.update(cx, |this, cx| { this.x = 1; cx.notify(); }).ok();  // Result
this.update_in(cx, |this, window, cx| { … }).ok();              // AsyncWindowContext
entity.update(cx, |this, cx| …);  entity.read(cx);  weak.upgrade();
```

`cx.notify()` marks the entity changed. Listeners: `Context::observe(&entity, ..)`
(`context.rs:64`), `Context::subscribe(&entity, ..)` (`:94`, events),
`cx.listener(|this, event, window, cx| ..)` (`:246`).

### Background job → update view

```rust
fn reload(&mut self, cx: &mut Context<Self>) {
    if self.loading { return; }
    self.loading = true;
    cx.notify();

    let task: Task<Result<Vec<Profile>, String>> =
        cx.background_executor().spawn(async move { fetch_profiles_blocking() }); // executor.rs:103

    cx.spawn(async move |this, cx| {
        let result = task.await;
        this.update(cx, |this, cx| {
            this.loading = false;
            match result { Ok(v) => this.profiles = v, Err(e) => this.error = Some(e.into()) }
            cx.notify();
        }).ok();
    }).detach();
}
```

### 100 ms poll over `std::sync::mpsc::Receiver`

`Receiver` is `Send` but neither `Clone` nor `Sync`, so keep it in the view and drain it
inside `this.update`:

```rust
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

struct Live { events: Receiver<ProxyEvent>, polling: bool }

impl Live {
    fn start_polling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.polling { return; }
        self.polling = true;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await; // executor.rs:183
                let keep_going = this.update(cx, |this, cx| {
                    let mut changed = false;
                    loop {
                        match this.events.try_recv() {
                            Ok(ev) => { this.apply(ev); changed = true; }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => return false,
                        }
                    }
                    if changed { cx.notify(); }
                    true
                }).unwrap_or(false);
                if !keep_going { break; }
            }
        }).detach();
    }
}
```

There is no `Timer::after` in `gpui-pre` 0.3.9; only `BackgroundExecutor::timer`.
In tests advance time with `cx.background_executor.advance_clock(d)` + `cx.run_until_parked()`.

Blocking producer (sockets, subprocess waits, file watches):

```rust
let (tx, rx) = std::sync::mpsc::channel::<ProxyEvent>();
std::thread::spawn(move || { let _ = tx.send(ProxyEvent::Ready); });  // store `rx` in the view
```

Or `cx.background_executor().spawn(async move { … tx.send(..) }).detach()` for `Send` async
work. `cx.http_client()` is the app-wide `Arc<dyn HttpClient>` (`App::set_http_client` replaces it).

---

## 8. Testing and building

```rust
// tests/ui_smoke.rs — needs features = ["test-support"] in dev-dependencies
use gpui_kit::component::{button::Button, input::{Input, InputState}};
use gpui_kit::test::{TestSupportExt as _, TestWindowExt as _};
use gpui_kit::{div, prelude::*, px, AppContext, Context, Entity, TestAppContext, Window};

struct Hello { input: Entity<InputState> }

impl Render for Hello {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
            .child(Input::new(&self.input).id("name").w(px(240.)))
            .child(Button::new("ok").label("OK"))
    }
}

#[gpui_kit::test]
fn typing_reaches_the_state(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _view) = cx.update(|cx| {
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| Hello { input: cx.new(|cx| InputState::new(window, cx)) })
        })
    }).expect("window");

    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("name", cx);
        window.input("主機 A", cx);
        assert_eq!(window.find("name").value(), Some("主機 A"));
        window.click("ok", cx);
    }).unwrap();
}
```

- `gpui_kit::test::{TestWindowExt, TestAppContextExt, TestSupportExt, ElementSnapshot}`
  (`gpui-kit/src/test.rs:20-43`). `TestWindowExt`: `find`, `try_find`, `within`, `render_frame`,
  `click`, `click_at`, `double_click`, `hover`, `scroll`, `drag`, `press`, `input`.
- `.test_support()` (no-op in release) is implemented for
  `Element<PrepaintState=Option<Hitbox>> + InteractiveElement` (`gpui-base/src/observe.rs:20-38`)
  — use it on `div()`, not on every kit builder.
- `cx.wait_for(handle, Duration, |window, cx| …).await` inside `#[gpui_kit::test] async fn`
  for timers/animations. Ids must be unique per scope (`tests/window.rs:164-173`).
- `VisualTestContext::from_window(handle.into(), cx)` for paint assertions
  (`window.painted_quads()`) — macOS/Metal only (`tests/rendering.rs:1-40`, `text_selection.rs:91-99`).

Build/run:

- `cargo build` / `cargo run` need no `.app` bundle; Metal and font-kit arrive through
  `gpui-kit`'s platform dependency.
- A Dock icon, display name, document types and URL schemes need a real
  `LocalProxy.app/Contents/{Info.plist,MacOS,Resources}` bundle — not configured by `gpui-kit`.
- No entitlement for outbound network in an unsigned/unsandboxed binary; if you enable App
  Sandbox add `com.apple.security.network.client` (and `.server` if you listen).
- Toolchain pinned to 1.94.0 by `rust-toolchain.toml`; `gpui-pre` is edition 2024 (a 2021
  consumer crate is fine).

---

## 9. Gotchas

1. `open_window` needs `&mut App`: call it inside `Application::run` or `cx.update(..)`.
2. Order is `init` → `install_palette` → `open_window`. `init` reloads the default light theme.
3. Never call `Theme::sync_system_appearance`; it re-runs `Theme::change` and wipes the palette.
4. `Theme::global_mut` does not sync tokens/base/windows. Use `Theme::update` (or follow with
   `Theme::sync_base(cx)` + `cx.refresh_windows()`).
5. Primary buttons read `button_primary*`, not `primary` (`button.rs:996`) — set both plus
   hover/active/foreground or filled buttons stay grey.
6. `.overflow_y_scroll()`, `.track_scroll()`, `.window_control_area()`, `.on_double_click()`,
   `.focusable()` require `.id(..)`; `.on_click()` and `.occlude()` do not.
7. `.hover()` can be called once per element — a second call panics in debug (`div.rs:843`).
8. `.p_4()/.gap_2()/.text_sm()` are rem-based and follow `theme.font_size`; use `px()` for
   pixel-exact spacing.
9. `div()` has no background; the window background comes from the root plugin
   (`theme.tokens.background`). Our canvas/panel split needs an explicit `.bg()` (§3).
10. Two `InputEvent` types: `gpui::InputEvent` (IME) vs `component::input::InputEvent`
    (`Change`/`PressEnter`/`Focus`/`Blur`). Alias one when both are in scope.
11. `SharedString`: `.into()` from `&'static str`/`String`/`Arc<str>`; kit APIs take
    `impl Into<SharedString>` so literals work as-is.
12. Trait imports: `Styled`, `ParentElement`, `InteractiveElement`, `StatefulInteractiveElement`,
    `Render`, `IntoElement` come from `prelude::*`. `Sizable`, `Disableable`, `Selectable`,
    `ButtonVariants`, `StyledExt`, `ThemeStyled`, `ActiveTheme`, `WindowExt`,
    `ScrollableElement` must be imported from `gpui_kit::component::*`.
13. `impl IntoElement` return types: branches of a `when` that yield different concrete
    element types must be unified with `.into_any_element()` (or restructured).
14. Builders that are `Styled` (so `.w()/.bg()` work): `Button`, `Input`, `Select`,
    `Combobox`, `Tag`, `Dialog`, `InputGroup`. `Spinner` is not — wrap it.
15. Dialog builders may run more than once — create `InputState`s outside and capture them.
16. Notifications auto-hide after a hard-coded 5 s (`notification.rs:872`).
17. The default `Dialog` is 10% from the top, not centered (`dialog.rs:618`).
18. `IconName` variants differ between `component` and `assets` (§6); non-default icons need
    `AllAssets`.
19. `img("relative/path.png")` is an asset lookup; use `PathBuf`/`&Path` for real files.
20. macOS traffic lights cannot be hidden through the API — only repositioned.
21. `window.remove_window()` takes `&mut Window` and only marks the window for removal; on
    macOS the process keeps running unless `cx.on_window_closed` quits it.
22. `cx.set_window_appearance` and `ActivationPolicy` are macOS-only
    (`#[cfg(target_os = "macos")]` if you ever build elsewhere).
