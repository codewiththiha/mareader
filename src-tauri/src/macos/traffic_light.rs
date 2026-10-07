//! Dynamic traffic-light layout for macOS, ported to `objc2`.

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use objc2::rc::Retained;
    use objc2_app_kit::{NSButton, NSView, NSWindow, NSWindowButton};
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use tauri::{
        Window, WindowEvent,
        plugin::{Builder, TauriPlugin},
    };

    // A mirror of `app_chrome::TITLE_BAR_H`, policed by check-chrome-contracts.
    const DEFAULT_HEADER_HEIGHT: f64 = 48.0;
    const TRAFFIC_LIGHT_X_INSET: f64 = 20.0;
    /// tauri.conf.json's `trafficLightPosition.y`, which tao re-applies.
    const TRAFFIC_LIGHT_Y_INSET: f64 = 25.0;
    const FALLBACK_BUTTON_SIZE: (f64, f64) = (14.0, 16.0);

    // The last requested state, so window events can re-apply it.
    static VISIBLE: AtomicBool = AtomicBool::new(true);
    static HEADER_HEIGHT_BITS: AtomicU64 = AtomicU64::new(DEFAULT_HEADER_HEIGHT.to_bits());
    static NATURAL_BUTTON_SIZE: OnceLock<(f64, f64)> = OnceLock::new();

    fn header_height() -> f64 {
        f64::from_bits(HEADER_HEIGHT_BITS.load(Ordering::Relaxed))
    }

    /// The button's natural size, read once and kept for the process.
    fn natural_button_size(close: &NSView) -> (f64, f64) {
        let frame: CGRect = close.frame();
        if frame.size.width > 0.0 && frame.size.height > 0.0 {
            let _ = NATURAL_BUTTON_SIZE.set((frame.size.width, frame.size.height));
        }
        *NATURAL_BUTTON_SIZE.get().unwrap_or(&FALLBACK_BUTTON_SIZE)
    }

    /// The buttons' `origin.y` centring them on a `header_height` bar.
    fn button_origin_y(header_height: f64, button_height: f64, container_height: f64) -> f64 {
        let top = ((header_height - button_height) / 2.0).round().max(0.0);
        (container_height - button_height - top).max(0.0)
    }

    /// The three standard buttons, as views, close → miniaturize → zoom.
    fn standard_buttons(ns_window: &NSWindow) -> Option<[Retained<NSButton>; 3]> {
        Some([
            ns_window.standardWindowButton(NSWindowButton::CloseButton)?,
            ns_window.standardWindowButton(NSWindowButton::MiniaturizeButton)?,
            ns_window.standardWindowButton(NSWindowButton::ZoomButton)?,
        ])
    }

    /// Lay the lights out, agreeing with tao's own container geometry.
    fn position_traffic_lights(ns_window: &NSWindow, visible: bool, header_height: f64) {
        let Some(buttons) = standard_buttons(ns_window) else {
            return;
        };
        let views: [&NSView; 3] = [&buttons[0], &buttons[1], &buttons[2]];
        let close = views[0];

        // The container is the superview of the close button's superview.
        let container: Option<Retained<NSView>> =
            unsafe { close.superview().and_then(|v| v.superview()) };
        let Some(container) = container else { return };

        // Measure BEFORE hiding: a hidden button is no ruler.
        let (button_width, button_height) = natural_button_size(close);
        for v in views {
            v.setHidden(!visible);
        }
        if !visible {
            return;
        }

        let container_height = button_height + TRAFFIC_LIGHT_Y_INSET;
        let window_height = ns_window.frame().size.height;
        let mut rect: CGRect = container.frame();
        let pinned_y = window_height - container_height;
        let stale = (rect.size.height - container_height).abs() > 0.01
            || (rect.origin.y - pinned_y).abs() > 0.01;
        if stale {
            rect.size.height = container_height;
            rect.origin.y = pinned_y;
            container.setFrame(rect);
        }

        // Derive the spacing live rather than hardcoding 20px vs 18px.
        let spacing = views[1].frame().origin.x - close.frame().origin.x;
        let spacing = if spacing.abs() < 0.5 { 20.0 } else { spacing };
        let y = button_origin_y(header_height, button_height, container_height);
        let size = CGSize {
            width: button_width,
            height: button_height,
        };
        for (i, v) in views.into_iter().enumerate() {
            let frame = v.frame();
            if (frame.size.width - size.width).abs() > 0.01
                || (frame.size.height - size.height).abs() > 0.01
            {
                v.setFrameSize(size);
            }
            v.setFrameOrigin(CGPoint {
                x: TRAFFIC_LIGHT_X_INSET + i as f64 * spacing,
                y,
            });
        }
    }

    /// Resolve the `NSWindow` behind a Tauri `Window` via `raw-window-handle`.
    fn with_ns_window<F: FnOnce(&NSWindow)>(window: &Window, f: F) {
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else {
            return;
        };
        // SAFETY: `ns_view` is the window's content view, alive while it is.
        let view = unsafe { &*h.ns_view.as_ptr().cast::<NSView>() };
        let Some(ns_window) = view.window() else {
            return;
        };
        f(&ns_window);
    }

    /// Re-apply the last state on the main thread, right now when there.
    fn reapply(window: &Window) {
        if objc2::MainThreadMarker::new().is_some() {
            let (visible, h) = (VISIBLE.load(Ordering::Relaxed), header_height());
            with_ns_window(window, |ns| position_traffic_lights(ns, visible, h));
            return;
        }
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
        with_ns_window(&window, |ns| {
            position_traffic_lights(ns, visible, effective)
        });
    }

    pub fn init() -> TauriPlugin<tauri::Wry> {
        Builder::new("traffic_light")
            .on_window_ready(|window| {
                // Overwrite the pre-mount `tauri.conf.json` fallback with the
                // centred value as soon as the window exists...
                reapply(&window);
                // ...and again on a theme change and on every resize.
                let w = window.clone();
                window.on_window_event(move |event| {
                    if matches!(
                        event,
                        WindowEvent::ThemeChanged(_) | WindowEvent::Resized(_)
                    ) {
                        reapply(&w);
                    }
                });
            })
            .build()
    }
}

#[cfg(target_os = "macos")]
pub use imp::{init, set_traffic_lights};
