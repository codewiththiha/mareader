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
//! The last requested state is kept process-wide and re-applied on
//! `Resized` and `ThemeChanged` (AppKit re-lays out the button container on
//! both), and a hide also hides the buttons themselves, so a relayout can
//! never surface them on its own.
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
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::OnceLock;

    use objc2::rc::Retained;
    use objc2_app_kit::{NSButton, NSView, NSWindow, NSWindowButton};
    use objc2_core_foundation::{CGPoint, CGRect};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use tauri::{
        plugin::{Builder, TauriPlugin},
        Window, WindowEvent,
    };

    // The wasm side's canonical source is `app_chrome::TITLE_BAR_H` (h-12);
    // the shell cannot depend on wasm crates, so it keeps this mirror.
    const DEFAULT_HEADER_HEIGHT: f64 = 48.0;
    const TRAFFIC_LIGHT_X_INSET: f64 = 20.0; // matches tauri.conf.json x:20
    /// AppKit's rest origin for a standard button when nothing better has
    /// been measured yet (Sonoma's value; Tahoe measures ~7).
    const FALLBACK_BUTTON_ORIGIN_Y: f64 = 5.0;
    const FALLBACK_BUTTON_HEIGHT: f64 = 14.0;

    // The last requested state, so `Resized` / `ThemeChanged` can re-apply
    // it without a round trip through the frontend.
    static VISIBLE: AtomicBool = AtomicBool::new(true);
    static HEADER_HEIGHT_BITS: AtomicU64 = AtomicU64::new(DEFAULT_HEADER_HEIGHT.to_bits());
    static NATURAL_BUTTON_ORIGIN_Y: OnceLock<f64> = OnceLock::new();

    fn header_height() -> f64 {
        f64::from_bits(HEADER_HEIGHT_BITS.load(Ordering::Relaxed))
    }

    fn natural_origin_y() -> f64 {
        *NATURAL_BUTTON_ORIGIN_Y.get().unwrap_or(&FALLBACK_BUTTON_ORIGIN_Y)
    }

    /// `y` = distance from the title-bar container's top to the button's top.
    /// After AppKit applies it, the button's window-top position is
    /// `y - button_origin_y` because `origin.y` (AppKit's rest) is preserved.
    fn compute_traffic_light_y(header_height: f64, button_height: f64, button_origin_y: f64) -> f64 {
        ((header_height - button_height) / 2.0 + button_origin_y).max(0.0)
    }

    /// The close button's height, caching its rest `origin.y` along the way.
    ///
    /// Only a laid-out frame is remembered: this runs on every `Resized`,
    /// and the first can arrive before AppKit has placed the buttons at all
    /// (origin 0). The rest position is cached for the life of the process,
    /// so a bogus read would pin the lights off-centre for good — a real
    /// rest is a small positive inset, anything else keeps the fallback.
    fn measure_close_button(close: &NSView) -> (f64, f64) {
        let frame: CGRect = close.frame();
        let laid_out = frame.size.height > 0.0;
        if laid_out && frame.origin.y > 0.0 && frame.origin.y < 20.0 {
            let _ = NATURAL_BUTTON_ORIGIN_Y.set(frame.origin.y);
        }
        let h = if laid_out { frame.size.height } else { FALLBACK_BUTTON_HEIGHT };
        (h, natural_origin_y())
    }

    /// The three standard buttons, as views, close → miniaturize → zoom.
    fn standard_buttons(ns_window: &NSWindow) -> Option<[Retained<NSButton>; 3]> {
        Some([
            ns_window.standardWindowButton(NSWindowButton::CloseButton)?,
            ns_window.standardWindowButton(NSWindowButton::MiniaturizeButton)?,
            ns_window.standardWindowButton(NSWindowButton::ZoomButton)?,
        ])
    }

    /// Owns both the container size and the button origins. This is the
    /// sole authority for traffic-light layout — we do not call Tauri's
    /// `set_traffic_light_position` (it would ping-pong via tao's
    /// `inset_traffic_lights` on every `drawRect`).
    fn position_traffic_lights(ns_window: &NSWindow, visible: bool, header_height: f64) {
        let Some(buttons) = standard_buttons(ns_window) else { return };
        let views: [&NSView; 3] = [&buttons[0], &buttons[1], &buttons[2]];
        let close = views[0];

        // The container that hosts all three lights is the superview of the
        // close button's superview. `superview()` is unsafe (unretained).
        let container: Option<Retained<NSView>> =
            unsafe { close.superview().and_then(|v| v.superview()) };
        let Some(container) = container else { return };

        // A hide also hides the buttons themselves: collapsing the container
        // alone is not durable, because AppKit re-lays it out on every
        // window resize and hands its natural height back — which used to
        // pop the lights onto a hidden bar with nothing left to hide them
        // again. Toggle BEFORE measuring: a hidden button is no ruler.
        for v in views {
            v.setHidden(!visible);
        }

        let container_height = if visible {
            let (button_height, button_origin_y) = measure_close_button(close);
            button_height + compute_traffic_light_y(header_height, button_height, button_origin_y)
        } else {
            0.0
        };

        // Resize the container and pin it to the window's top edge.
        let window_height = ns_window.frame().size.height;
        let mut rect: CGRect = container.frame();
        rect.size.height = container_height;
        rect.origin.y = window_height - container_height;
        container.setFrame(rect);

        if !visible {
            return;
        }

        // Re-anchor the buttons horizontally and at their natural y. The
        // spacing between the first two is stable across macOS versions;
        // derive it live rather than hardcode 20px vs 18px, falling back
        // only when a pre-layout read gives a stale zero.
        let spacing = views[1].frame().origin.x - close.frame().origin.x;
        let spacing = if spacing.abs() < 0.5 { 20.0 } else { spacing };
        let y = natural_origin_y();
        for (i, v) in views.into_iter().enumerate() {
            v.setFrameOrigin(CGPoint { x: TRAFFIC_LIGHT_X_INSET + i as f64 * spacing, y });
        }
    }

    /// Resolve the `NSWindow` behind a Tauri `Window` via `raw-window-handle`.
    fn with_ns_window<F: FnOnce(&NSWindow)>(window: &Window, f: F) {
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
        // SAFETY: `ns_view` is the window's content view, alive while the
        // window is. We only borrow through it for the duration of `f`.
        let view = unsafe { &*h.ns_view.as_ptr().cast::<NSView>() };
        let Some(ns_window) = view.window() else { return };
        f(&ns_window);
    }

    /// Re-apply the last requested state on the main thread (AppKit's).
    fn reapply(window: &Window) {
        let target = window.clone();
        let _ = window.run_on_main_thread(move || {
            let (visible, h) = (VISIBLE.load(Ordering::Relaxed), header_height());
            with_ns_window(&target, |ns| position_traffic_lights(ns, visible, h));
        });
    }

    pub fn set_traffic_lights(window: Window, visible: bool, header_height: f64) {
        VISIBLE.store(visible, Ordering::Relaxed);
        if header_height > 0.0 {
            HEADER_HEIGHT_BITS.store(header_height.to_bits(), Ordering::Relaxed);
        }
        let effective = self::header_height();
        with_ns_window(&window, |ns| position_traffic_lights(ns, visible, effective));
    }

    pub fn init() -> TauriPlugin<tauri::Wry> {
        Builder::new("traffic_light")
            .on_window_ready(|window| {
                // Overwrite the pre-mount `tauri.conf.json` fallback with the
                // centred value as soon as the window exists...
                reapply(&window);
                // ...and again whenever AppKit may have re-laid the button
                // container out from under us: a theme change, and — the case
                // that used to leak the lights back onto a hidden bar — every
                // window resize.
                let w = window.clone();
                window.on_window_event(move |event| {
                    if matches!(event, WindowEvent::ThemeChanged(_) | WindowEvent::Resized(_)) {
                        reapply(&w);
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
