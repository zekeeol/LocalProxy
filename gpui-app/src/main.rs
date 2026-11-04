//! LocalProxy — native macOS UI built on GPUI Kit.
//!
//! The networking core lives in the `local_proxy_rust` crate (formerly driven
//! by an Electron sidecar). The native app calls that crate in-process; there
//! is no IPC protocol in the middle anymore.

// Import GPUI through the Kit facade so `gpui::` paths resolve without a
// direct dependency on the upstream snapshot crate.
extern crate gpui_kit as gpui;

mod app;
mod bridge;
mod i18n;
mod state;
mod theme;
mod views;
mod widgets;
mod wireguard_status;

use gpui::px;
use gpui::size;
use gpui_kit::prelude::*;
use gpui_kit::*;

fn window_options(cx: &mut App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1360.), px(900.)),
            cx,
        ))),
        window_min_size: Some(size(px(1060.), px(700.))),
        titlebar: Some(TitlebarOptions {
            title: Some("LocalProxy".into()),
            appears_transparent: true,
            // Keep the native macOS traffic lights; the custom titlebar draws
            // around them exactly like the original Electron UI.
            traffic_light_position: Some(gpui::point(px(18.), px(21.))),
        }),
        app_owns_titlebar_drag: true,
        ..Default::default()
    }
}

struct AppAssets;
impl gpui::AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if path == "brand/localproxy-icon.png" {
            return Ok(Some(std::borrow::Cow::Borrowed(include_bytes!(
                "../../assets/brand/localproxy-icon.png"
            ))));
        }
        gpui_kit::assets::AllAssets.load(path)
    }
    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        gpui_kit::assets::AllAssets.list(path)
    }
}

fn main() {
    application().with_assets(AppAssets).run(|cx| {
        init(cx);
        theme::install(cx);
        bridge::start();
        // Screen unlock can occur without a system wake. Keep the
        // distributed notification observer alive for the application.
        #[cfg(target_os = "macos")]
        let unlock_observer = {
            use block2::RcBlock;
            use objc2_foundation::{ns_string, NSDistributedNotificationCenter, NSNotification};
            let callback = RcBlock::new(|_: std::ptr::NonNull<NSNotification>| {
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    bridge::request_wireguard_revalidate();
                });
            });
            // The callback captures no non-Send state and accepts the
            // NSNotification supplied by Foundation.
            unsafe {
                NSDistributedNotificationCenter::defaultCenter()
                    .addObserverForName_object_queue_usingBlock(
                        Some(ns_string!("com.apple.screenIsUnlocked")),
                        None,
                        None,
                        &callback,
                    )
            }
        };
        cx.on_system_wake(|cx| {
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(3))
                    .await;
                bridge::request_wireguard_revalidate();
            })
            .detach();
        })
        .detach();
        // Stop services synchronously: GPUI's quit timeout is shorter than
        // the privileged helper's route/DNS teardown can take.
        cx.on_app_quit(move |_| {
            #[cfg(target_os = "macos")]
            let _ = &unlock_observer;
            local_proxy_rust::api::local_proxy::shutdown();
            async {}
        })
        .detach();

        let lang = i18n::detect_language();
        let (window, _shell) = open_window(window_options(cx), cx, |window, cx| {
            window.set_window_title("LocalProxy");
            cx.new(|cx| app::AppShell::new(window, cx, lang))
        })
        .expect("failed to open the LocalProxy window");

        cx.activate(true);
        let _ = window.update(cx, |_, window, _| window.activate_window());

        // Quit once the last window is gone.
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
