//! Dynamic traffic-light layout — port of `readest/.../traffic_light.rs` to `objc2`.
//!
//! `tauri.conf.json:trafficLightPosition` is only the pre-mount fallback
//! (`{x:20,y:25}`). After the webview mounts, the frontend measures the
//! real header height (`h-12` = 48px, via `ResizeObserver` on `#toolbar-row`)
//! and invokes `set_traffic_lights {visible, headerHeight}`. Rust then
//! owns the *vertical* position:
//!
//! ```text
//! y = ((header_height - button_height) / 2 + natural_origin_y).max(0)
//! container.height = visible ? button_height + y : 0
//! container.origin.y = window.height - container.height
//! button.origin = (x_inset + i*spacing, natural_origin_y) // AppKit's rest
//! ```
//!
//! `natural_origin_y` is cached in `OnceLock` on first read (~5pt on
//! Sonoma/Sequoia, ~7pt on Tahoe/26). Re-reading after AppKit autoresizes
//! the container would feed back and drift `y` — caching makes the formula
//! a fixed-point. Mirrors `readest` `measure_close_button` + `compute…`.
//!
//! `objc2` is kept (not `cocoa`/`objc` `msg_send!`) because the rest of
//! this crate already depends on `objc2-app-kit 0.3` and `cocoa` is frozen.
//! The geometry types are `objc2_core_foundation::{CGPoint,CGRect,CGSize}`
//! (aliased as `NSPoint/NSRect/NSSize` by `objc2-foundation`'s `geometry`
//! module when `objc2-core-foundation` is enabled — here we use the
//! core-foundation names directly to avoid feature gating).

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::OnceLock;

    use objc2::rc::Retained;
    use objc2_app_kit::{NSButton, NSView, NSWindow, NSWindowButton};
    use objc2_core_foundation::{CGPoint, CGRect};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use tauri::{
        plugin::{Builder, TauriPlugin},
        Window,
    };

    // Last values so resize / ThemeChanged can re-apply without IPC.
    static mut TRAFFIC_LIGHTS_VISIBLE: bool = true;
    static mut TRAFFIC_LIGHT_HEADER_HEIGHT: f64 = DEFAULT_HEADER_HEIGHT;
    static NATURAL_BUTTON_ORIGIN_Y: OnceLock<f64> = OnceLock::new();

    const DEFAULT_HEADER_HEIGHT: f64 = 48.0; // pdf-reader `h-12`
    const TRAFFIC_LIGHT_X_INSET: f64 = 20.0; // matches tauri.conf.json x:20

    /// `y` = distance from the title-bar container's top to the button's top.
    /// After AppKit applies it, the button's window-top position is
    /// `y - button_origin_y` because `origin.y` (AppKit's rest) is preserved.
    fn compute_traffic_light_y(header_height: f64, button_height: f64, button_origin_y: f64) -> f64 {
        ((header_height - button_height) / 2.0 + button_origin_y).max(0.0)
    }

    /// Reads the close button's frame; caches `origin.y` on first call.
    /// Returns `(button_height, natural_origin_y)`. Falls back to `(14,5)`
    /// for decorationless windows that have no standard buttons.
    fn measure_close_button(ns_window: &NSWindow) -> (f64, f64) {
        let Some(close) = ns_window.standardWindowButton(NSWindowButton::CloseButton) else {
            return (14.0, *NATURAL_BUTTON_ORIGIN_Y.get().unwrap_or(&5.0));
        };
        // NSButton is an NSView subclass — `frame()` is on NSView.
        let view: &NSView = &close;
        let frame: CGRect = view.frame();
        let h = frame.size.height;
        let cached = *NATURAL_BUTTON_ORIGIN_Y.get_or_init(|| frame.origin.y);
        (h, cached)
    }

    /// Owns both the container size and the button origins. This is the
    /// sole authority for traffic-light layout — we do not call Tauri's
    /// `set_traffic_light_position` (it would ping-pong via tao's
    /// `inset_traffic_lights` on every `drawRect`).
    fn position_traffic_lights(ns_window: &NSWindow, visible: bool, header_height: f64) {
        let Some(close) = ns_window.standardWindowButton(NSWindowButton::CloseButton) else {
            return;
        };
        let miniaturize = ns_window.standardWindowButton(NSWindowButton::MiniaturizeButton);
        let zoom = ns_window.standardWindowButton(NSWindowButton::ZoomButton);

        // The container that hosts all three lights is the superview of the
        // close button's superview. `superview()` is unsafe (unretained).
        let container: Option<Retained<NSView>> = unsafe {
            close
                .superview()
                .and_then(|v| v.superview())
        };
        let Some(container) = container else {
            return;
        };

        let title_bar_frame_height = if visible {
            let (button_height, button_origin_y) = measure_close_button(ns_window);
            let y = compute_traffic_light_y(header_height, button_height, button_origin_y);
            button_height + y
        } else {
            0.0
        };

        // Resize the container and pin it to the window's top edge.
        let window_frame: CGRect = ns_window.frame();
        let mut rect: CGRect = container.frame();
        rect.size.height = title_bar_frame_height;
        rect.origin.y = window_frame.size.height - title_bar_frame_height;
        container.setFrame(rect);

        if !visible {
            return;
        }
        // Re-anchor buttons horizontally and at their natural y.
        let Some(mini) = miniaturize else { return; };
        let Some(zm) = zoom else { return; };

        let cached_origin_y = *NATURAL_BUTTON_ORIGIN_Y.get().unwrap_or(&5.0);
        let close_rect: CGRect = {
            let v: &NSView = &close;
            v.frame()
        };
        let mini_rect: CGRect = {
            let v: &NSView = &mini;
            v.frame()
        };
        // Horizontal spacing between the first two buttons is stable across
        // macOS versions; derive it live so we don't hardcode 20px vs 18px.
        let space_between = mini_rect.origin.x - close_rect.origin.x;
        // Avoid a stale zero spacing on first paint (can happen before layout).
        let space_between = if space_between.abs() < 0.5 { 20.0 } else { space_between };

        for (i, btn) in [&close as &NSButton, &mini as &NSButton, &zm as &NSButton].into_iter().enumerate() {
            let v: &NSView = btn;
            let origin = CGPoint {
                x: TRAFFIC_LIGHT_X_INSET + (i as f64 * space_between),
                y: cached_origin_y,
            };
            v.setFrameOrigin(origin);
        }
    }

    /// Resolve the `NSWindow` behind a Tauri `Window` via `raw-window-handle`.
    fn with_ns_window<F: FnOnce(&NSWindow)>(window: &Window, f: F) {
        let Ok(handle) = window.window_handle() else { return; };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return; };
        // SAFETY: `ns_view` is the window's content view, alive while the
        // window is. We only borrow through it for the duration of `f`.
        let view = unsafe { &*h.ns_view.as_ptr().cast::<NSView>() };
        let Some(ns_window) = view.window() else { return; };
        f(&ns_window);
    }

    pub fn set_traffic_lights(window: Window, visible: bool, header_height: f64) {
        unsafe {
            TRAFFIC_LIGHTS_VISIBLE = visible;
            if header_height > 0.0 {
                TRAFFIC_LIGHT_HEADER_HEIGHT = header_height;
            }
            let h = TRAFFIC_LIGHT_HEADER_HEIGHT;
            with_ns_window(&window, |ns_window| {
                position_traffic_lights(ns_window, visible, h);
            });
        }
    }

    pub fn init() -> TauriPlugin<tauri::Wry> {
        Builder::new("traffic_light")
            .on_window_ready(|window| {
                #[cfg(target_os = "macos")]
                {
                    let w_for_main = window.clone();
                    // Re-apply on the main thread once the window is ready so
                    // the initial `y:25` fallback from `tauri.conf.json` is
                    // immediately overwritten with the centered value.
                    let w_for_closure = w_for_main.clone();
                    let _ = w_for_main.run_on_main_thread(move || {
                        unsafe {
                            let h = TRAFFIC_LIGHT_HEADER_HEIGHT;
                            let v = TRAFFIC_LIGHTS_VISIBLE;
                            with_ns_window(&w_for_closure, |ns| position_traffic_lights(ns, v, h));
                        }
                    });
                }
                let w2 = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::ThemeChanged(_) = event {
                        let w_for_main = w2.clone();
                        let w_for_closure = w_for_main.clone();
                        let _ = w_for_main.run_on_main_thread(move || {
                            unsafe {
                                let h = TRAFFIC_LIGHT_HEADER_HEIGHT;
                                let v = TRAFFIC_LIGHTS_VISIBLE;
                                with_ns_window(&w_for_closure, |ns| position_traffic_lights(ns, v, h));
                            }
                        });
                    }
                });
            })
            .build()
    }


}

#[cfg(target_os = "macos")]
pub use imp::{init, set_traffic_lights};

#[cfg(not(target_os = "macos"))]
pub use super::traffic_light::{init, set_traffic_lights};
