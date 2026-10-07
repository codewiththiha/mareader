//! Dynamic traffic-light layout — port of `readest/.../traffic_light.rs` to
//! `objc2`.
//!
//! TWO WRITERS, ONE GEOMETRY. `tauri.conf.json:trafficLightPosition` is not
//! only a pre-mount fallback: tao keeps it and re-applies it from its view's
//! `drawRect:` (`inset_traffic_lights`) — container height `button_h + y`,
//! button `x` — on every redraw, which a live resize produces continuously.
//! This module used to write a DIFFERENT container height (and collapse it
//! to zero on a hide), so every resize frame ping-ponged the container
//! between the two: the lights blinked while the window was dragged, and the
//! buttons' autoresizing squeezed them into ovals when the container shrank
//! under them. Now both writers agree on the container (`button_h +`
//! [`TRAFFIC_LIGHT_Y_INSET`], the config's `y`) and on `x`; the only thing
//! this module adds is the one coordinate tao never touches — the buttons'
//! `origin.y` inside that container — which is what centres them on the
//! measured header:
//!
//! ```text
//! top      = round((header_height - button_h) / 2)       // from the window top
//! container.height = button_h + TRAFFIC_LIGHT_Y_INSET   // tao's own value
//! button.origin    = (x_inset + i*spacing, container.height - button_h - top)
//! button.size      = the natural size, measured once     // never squeezed
//! ```
//!
//! A hide hides the buttons and leaves the container alone (tao would put it
//! back on the next redraw anyway). The last requested state is kept
//! process-wide and re-applied on `Resized` and `ThemeChanged` — in the same
//! main-thread turn as the event, so AppKit never draws a frame with its own
//! rest layout in between.

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

    // The wasm side's canonical source is `app_chrome::TITLE_BAR_H` (h-12);
    // the shell cannot depend on wasm crates, so it keeps this mirror. Both
    // mirrors — this height and the inset below, which is tauri.conf.json's
    // `trafficLightPosition.x` — are policed by
    // `tools/check-chrome-contracts.ts`.
    const DEFAULT_HEADER_HEIGHT: f64 = 48.0;
    const TRAFFIC_LIGHT_X_INSET: f64 = 20.0;
    /// tauri.conf.json's `trafficLightPosition.y`: the value tao re-applies
    /// from `drawRect:`, so the container height this module writes must be
    /// built from the same number or the two fight on every redraw.
    const TRAFFIC_LIGHT_Y_INSET: f64 = 25.0;
    const FALLBACK_BUTTON_SIZE: (f64, f64) = (14.0, 16.0);

    // The last requested state, so `Resized` / `ThemeChanged` can re-apply
    // it without a round trip through the frontend.
    static VISIBLE: AtomicBool = AtomicBool::new(true);
    static HEADER_HEIGHT_BITS: AtomicU64 = AtomicU64::new(DEFAULT_HEADER_HEIGHT.to_bits());
    static NATURAL_BUTTON_SIZE: OnceLock<(f64, f64)> = OnceLock::new();

    fn header_height() -> f64 {
        f64::from_bits(HEADER_HEIGHT_BITS.load(Ordering::Relaxed))
    }

    /// The standard button's natural size, read once from a laid-out frame
    /// and kept for the process: a later read could catch a button that
    /// autoresizing has squeezed, and writing that back would make the
    /// squeeze permanent.
    fn natural_button_size(close: &NSView) -> (f64, f64) {
        let frame: CGRect = close.frame();
        if frame.size.width > 0.0 && frame.size.height > 0.0 {
            let _ = NATURAL_BUTTON_SIZE.set((frame.size.width, frame.size.height));
        }
        *NATURAL_BUTTON_SIZE.get().unwrap_or(&FALLBACK_BUTTON_SIZE)
    }

    /// The buttons' `origin.y` inside a container `container_height` tall
    /// that centres a `button_height` button on a `header_height` bar.
    /// Whole points, so the lights never land on a half pixel.
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

    /// Lay the lights out (see the module docs). Agrees with tao's
    /// `inset_traffic_lights` on everything tao writes, so a redraw between
    /// two calls changes nothing.
    fn position_traffic_lights(ns_window: &NSWindow, visible: bool, header_height: f64) {
        let Some(buttons) = standard_buttons(ns_window) else {
            return;
        };
        let views: [&NSView; 3] = [&buttons[0], &buttons[1], &buttons[2]];
        let close = views[0];

        // The container that hosts all three lights is the superview of the
        // close button's superview. `superview()` is unsafe (unretained).
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

        // The spacing between the first two is stable across macOS
        // versions; derive it live rather than hardcode 20px vs 18px,
        // falling back only when a pre-layout read gives a stale zero.
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
        // SAFETY: `ns_view` is the window's content view, alive while the
        // window is. We only borrow through it for the duration of `f`.
        let view = unsafe { &*h.ns_view.as_ptr().cast::<NSView>() };
        let Some(ns_window) = view.window() else {
            return;
        };
        f(&ns_window);
    }

    /// Re-apply the last requested state on the main thread (AppKit's) —
    /// right now when already there, which is where window events arrive:
    /// a hop through the queue would let AppKit draw its own rest layout
    /// first, and that frame is the blink.
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
                // ...and again whenever AppKit may have re-laid the button
                // container out from under us: a theme change, and — the case
                // that used to leak the lights back onto a hidden bar — every
                // window resize.
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
