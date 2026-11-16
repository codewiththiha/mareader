//! macOS traffic lights: centred in the bar, fading with it (docs/design.md).

/// The close button's left edge, as in mareader.
const INSET_X: f64 = 20.0;
/// AppKit's gap between button origins, if it can't be measured.
const FALLBACK_SPACING: f64 = 20.0;

/// Where the buttons go, in AppKit's bottom-up coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The titlebar container: its bottom edge and height.
    pub container_y: f64,
    pub container_h: f64,
    /// Each button's origin inside the container.
    pub xs: [f64; 3],
    pub y: f64,
}

/// Centres buttons of `button_h` in a bar of `bar_h`, `shift` from home.
pub fn layout(window_h: f64, bar_h: f64, button_h: f64, spacing: f64, shift: f64) -> Layout {
    let finite = |v: f64, or: f64| if v.is_finite() { v } else { or };
    let bar_h = finite(bar_h, 0.0).max(0.0);
    let button_h = finite(button_h, 0.0).clamp(0.0, bar_h);
    let spacing = finite(spacing, 0.0);
    let shift = finite(shift, 0.0);
    // A zero or reversed gap means the buttons weren't laid out yet.
    let spacing = if spacing < 1.0 {
        FALLBACK_SPACING
    } else {
        spacing
    };
    Layout {
        container_y: finite(window_h, 0.0) - bar_h,
        container_h: bar_h,
        xs: [0.0, 1.0, 2.0].map(|i| INSET_X + shift + i * spacing),
        // Half points: whole Retina pixels, even in a zoomed bar.
        y: (bar_h - button_h).round() / 2.0,
    }
}

/// Container y inside its superview, honouring flipped coordinates.
pub fn container_origin_y(want: Layout, superview_flipped: bool) -> f64 {
    if superview_flipped {
        0.0
    } else {
        want.container_y
    }
}

#[cfg(all(target_os = "macos", feature = "traffic-lights"))]
pub use mac::place;

#[cfg(all(target_os = "macos", feature = "traffic-lights"))]
#[allow(unsafe_code)]
mod mac {
    use std::cell::{Cell, RefCell};

    use mareader_ui_egui::platform::{LightSpots, NativeLights};
    use objc2::rc::{Retained, Weak};
    use objc2::runtime::{AnyObject, NSObject, Sel};
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{
        NSButton, NSView, NSViewFrameDidChangeNotification, NSWindow, NSWindowButton,
        NSWindowDidBecomeKeyNotification, NSWindowDidChangeBackingPropertiesNotification,
        NSWindowDidDeminiaturizeNotification, NSWindowDidEndLiveResizeNotification,
        NSWindowDidExitFullScreenNotification, NSWindowDidResignKeyNotification,
        NSWindowDidResizeNotification, NSWindowDidUpdateNotification, NSWindowStyleMask,
        NSWindowWillCloseNotification,
    };
    use objc2_foundation::{
        NSNotification, NSNotificationCenter, NSNotificationName, NSPoint, NSProcessInfo, NSRect,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[derive(Clone, Copy)]
    struct Desired {
        shift: f64,
        zoom: f64,
        opacity: f64,
        fullscreen: bool,
    }

    struct Ivars {
        window: Weak<NSWindow>,
        desired: Cell<Desired>,
        metrics: Cell<Option<(f64, f64)>>,
        views: Cell<Option<[usize; 5]>>,
        arranging: Cell<bool>,
        closed: Cell<bool>,
    }

    struct ArrangeGuard<'a>(&'a Cell<bool>);

    impl Drop for ArrangeGuard<'_> {
        fn drop(&mut self) {
            self.0.set(false);
        }
    }

    define_class!(
        // SAFETY: NSObject has no subclassing rules; there is no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "MareaderTrafficLights"]
        #[ivars = Ivars]
        struct Observer;

        impl Observer {
            #[unsafe(method(relayout:))]
            fn relayout(&self, _note: &NSNotification) {
                self.apply();
            }

            #[unsafe(method(windowWillClose:))]
            fn window_will_close(&self, _note: &NSNotification) {
                self.ivars().closed.set(true);
                let center = NSNotificationCenter::defaultCenter();
                // SAFETY: `self` was registered on this centre.
                unsafe { center.removeObserver(self) };
            }
        }
    );

    thread_local! {
        static OBSERVERS: RefCell<Vec<Retained<Observer>>> = const { RefCell::new(Vec::new()) };
    }

    impl Observer {
        fn new(window: &NSWindow, mtm: MainThreadMarker, desired: Desired) -> Retained<Self> {
            let ivars = Ivars {
                window: Weak::new(window),
                desired: Cell::new(desired),
                metrics: Cell::new(None),
                views: Cell::new(None),
                arranging: Cell::new(false),
                closed: Cell::new(false),
            };
            let this = Self::alloc(mtm).set_ivars(ivars);
            // SAFETY: NSObject's plain initialiser.
            let this: Retained<Self> = unsafe { msg_send![super(this), init] };
            this.register_window(window);
            this
        }

        fn observe(
            &self,
            center: &NSNotificationCenter,
            action: Sel,
            name: &NSNotificationName,
            target: &AnyObject,
        ) {
            // SAFETY: `self` answers `action`; `target` is a live object.
            unsafe {
                center.addObserver_selector_name_object(self, action, Some(name), Some(target));
            }
        }

        fn register_window(&self, window: &NSWindow) {
            let center = NSNotificationCenter::defaultCenter();
            let target: &AnyObject = window;
            // SAFETY: AppKit notification names are immutable constants.
            let names: [&NSNotificationName; 8] = unsafe {
                [
                    NSWindowDidResizeNotification,
                    NSWindowDidEndLiveResizeNotification,
                    NSWindowDidExitFullScreenNotification,
                    NSWindowDidChangeBackingPropertiesNotification,
                    NSWindowDidDeminiaturizeNotification,
                    NSWindowDidBecomeKeyNotification,
                    NSWindowDidResignKeyNotification,
                    NSWindowDidUpdateNotification,
                ]
            };
            for name in names {
                self.observe(&center, sel!(relayout:), name, target);
            }
            // SAFETY: AppKit notification name is an immutable constant.
            let close = unsafe { NSWindowWillCloseNotification };
            self.observe(&center, sel!(windowWillClose:), close, target);
        }

        fn watch_views(
            &self,
            titlebar: &NSView,
            container: &NSView,
            buttons: &[Retained<NSButton>; 3],
        ) {
            let views: [&NSView; 5] = [container, titlebar, &buttons[0], &buttons[1], &buttons[2]];
            let current = views.map(|v| std::ptr::from_ref(v).addr());
            if self.ivars().views.get() == Some(current) {
                return;
            }
            let center = NSNotificationCenter::defaultCenter();
            // SAFETY: AppKit notification name is an immutable constant.
            let frame_changed = unsafe { NSViewFrameDidChangeNotification };
            // SAFETY: removes only our view frame observers on this centre.
            unsafe {
                center.removeObserver_name_object(self, Some(frame_changed), None);
            }
            for view in views {
                view.setPostsFrameChangedNotifications(true);
                self.observe(&center, sel!(relayout:), frame_changed, view);
            }
            self.ivars().views.set(Some(current));
        }

        fn apply(&self) -> bool {
            let ivars = self.ivars();
            if ivars.closed.get() || ivars.arranging.get() {
                return false;
            }
            let Some(window) = ivars.window.load() else {
                return false;
            };
            let Some(buttons) = buttons(&window) else {
                return false;
            };
            let desired = ivars.desired.get();
            let fullscreen =
                desired.fullscreen || window.styleMask().contains(NSWindowStyleMask::FullScreen);
            let opacity = if fullscreen { 1.0 } else { desired.opacity };
            fade(&buttons, opacity);
            if fullscreen || opacity <= 0.0 {
                return false;
            }
            let Some((titlebar, container)) = titlebar_views(&window, &buttons) else {
                return false;
            };
            ivars.arranging.set(true);
            let _guard = ArrangeGuard(&ivars.arranging);
            self.watch_views(&titlebar, &container, &buttons);
            let metrics = measure_metrics(&buttons, ivars.metrics.get());
            if let Some(m) = metrics {
                ivars.metrics.set(Some(m));
            }
            arrange(&window, (&titlebar, &container, &buttons), desired, metrics);
            true
        }
    }

    fn ensure_observer(
        window: &NSWindow,
        mtm: MainThreadMarker,
        desired: Desired,
    ) -> Retained<Observer> {
        let same_window = |obs: &Observer| {
            obs.ivars()
                .window
                .load()
                .as_deref()
                .is_some_and(|w| std::ptr::eq(w, window))
        };
        let alive = |o: &Observer| !o.ivars().closed.get() && o.ivars().window.load().is_some();
        OBSERVERS.with(|list| {
            let mut list = list.borrow_mut();
            list.retain(|o| alive(o));
            if let Some(obs) = list.iter().find(|o| same_window(o)) {
                obs.ivars().desired.set(desired);
                return obs.clone();
            }
            let obs = Observer::new(window, mtm, desired);
            list.push(obs.clone());
            obs
        })
    }

    /// Places and fades the lights in a bar `zoom` times taller; returns
    /// where they landed.
    pub fn place(
        frame: &eframe::Frame,
        lights: NativeLights,
        fullscreen: bool,
        zoom: f32,
    ) -> Option<LightSpots> {
        // AppKit objects may only be touched on the main thread.
        let main = MainThreadMarker::new()?;
        let window = ns_window(frame)?;
        let zoom = f64::from(zoom).max(0.1);
        let shift = f64::from(lights.shift) * zoom;
        let opacity = f64::from(lights.opacity.clamp(0.0, 1.0));
        let desired = Desired {
            shift,
            zoom,
            opacity,
            fullscreen,
        };
        if !ensure_observer(&window, main, desired).apply() {
            return None;
        }
        let buttons = buttons(&window)?;
        spots(&window, &buttons, shift, zoom)
    }

    /// The buttons' centres at rest and their size, measured from AppKit.
    fn spots(
        window: &NSWindow,
        buttons: &[Retained<NSButton>; 3],
        shift: f64,
        zoom: f64,
    ) -> Option<LightSpots> {
        let height = window.frame().size.height;
        let centre = |b: &NSButton| {
            // SAFETY: a main-thread getter.
            let parent = unsafe { b.superview() }?;
            let r = parent.convertRect_toView(b.frame(), None);
            let x = r.origin.x + r.size.width / 2.0;
            Some((x, height - r.origin.y - r.size.height / 2.0, r.size.width))
        };
        let (x, y, size) = centre(&buttons[0])?;
        let (last, ..) = centre(&buttons[2])?;
        let points = |v: f64| (v / zoom) as f32;
        Some(LightSpots {
            first: egui::pos2(points(x - shift), points(y)),
            pitch: points((last - x) / 2.0),
            size: points(size),
            glass: NSProcessInfo::processInfo()
                .operatingSystemVersion()
                .majorVersion
                >= 26,
        })
    }

    fn ns_window(frame: &eframe::Frame) -> Option<Retained<NSWindow>> {
        let handle = frame.window_handle().ok()?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return None;
        };
        // SAFETY: `ns_view` lives while borrowed; main thread checked.
        let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        view.window()
    }

    fn buttons(window: &NSWindow) -> Option<[Retained<NSButton>; 3]> {
        Some([
            window.standardWindowButton(NSWindowButton::CloseButton)?,
            window.standardWindowButton(NSWindowButton::MiniaturizeButton)?,
            window.standardWindowButton(NSWindowButton::ZoomButton)?,
        ])
    }

    fn titlebar_views(
        window: &NSWindow,
        buttons: &[Retained<NSButton>; 3],
    ) -> Option<(Retained<NSView>, Retained<NSView>)> {
        // SAFETY: main-thread getters; titlebar sits inside the container.
        let titlebar = unsafe { buttons[0].superview() }?;
        // SAFETY: main-thread getter for the titlebar container view.
        let container = unsafe { titlebar.superview() }?;
        let same = container
            .window()
            .as_deref()
            .is_some_and(|w| std::ptr::eq(w, window));
        same.then_some((titlebar, container))
    }

    fn measure_metrics(
        buttons: &[Retained<NSButton>; 3],
        cached: Option<(f64, f64)>,
    ) -> Option<(f64, f64)> {
        if cached.is_some() {
            return cached;
        }
        let close = buttons[0].frame();
        let spacing = buttons[1].frame().origin.x - close.origin.x;
        let button_h = close.size.height;
        (spacing.is_finite() && spacing >= 1.0 && button_h.is_finite() && button_h >= 1.0)
            .then_some((spacing, button_h))
    }

    /// Hidden at zero, so invisible buttons can't take clicks.
    fn fade(buttons: &[Retained<NSButton>; 3], alpha: f64) {
        for b in buttons {
            if (b.alphaValue() - alpha).abs() > 0.001 {
                b.setAlphaValue(alpha);
            }
            if b.isHidden() != (alpha <= 0.0) {
                b.setHidden(alpha <= 0.0);
            }
        }
    }

    fn rect_differs(a: NSRect, b: NSRect) -> bool {
        (a.origin.x - b.origin.x).abs() > 0.5
            || (a.origin.y - b.origin.y).abs() > 0.5
            || (a.size.width - b.size.width).abs() > 0.5
            || (a.size.height - b.size.height).abs() > 0.5
    }

    /// Centres them in the bar as egui draws it, `zoom` times its height.
    fn arrange(
        window: &NSWindow,
        views: (&NSView, &NSView, &[Retained<NSButton>; 3]),
        desired: Desired,
        metrics: Option<(f64, f64)>,
    ) {
        let (titlebar, container, buttons) = views;
        let close = buttons[0].frame();
        let fallback = (
            buttons[1].frame().origin.x - close.origin.x,
            close.size.height,
        );
        let (spacing, button_h) = metrics.unwrap_or(fallback);
        let want = super::layout(
            window.frame().size.height,
            f64::from(mareader_ui_egui::theme::TITLE_BAR_H) * desired.zoom,
            button_h,
            spacing,
            desired.shift,
        );
        // SAFETY: main-thread getter; checks whether theme frame is flipped.
        let flipped = unsafe { container.superview() }.is_some_and(|s| s.isFlipped());
        let target_y = super::container_origin_y(want, flipped);
        // AppKit resets these on resize, so write only what moved.
        let mut rect = container.frame();
        if (rect.origin.y - target_y).abs() > 0.5
            || (rect.size.height - want.container_h).abs() > 0.5
        {
            rect.origin.y = target_y;
            rect.size.height = want.container_h;
            container.setFrame(rect);
        }
        let want_bar = container.bounds();
        if rect_differs(titlebar.frame(), want_bar) {
            titlebar.setFrame(want_bar);
        }
        for (b, x) in buttons.iter().zip(want.xs) {
            let at = b.frame().origin;
            if (at.x - x).abs() > 0.5 || (at.y - want.y).abs() > 0.5 {
                b.setFrameOrigin(NSPoint { x, y: want.y });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lights_sit_centred_in_the_bar() {
        let l = layout(800.0, 48.0, 16.0, 20.0, 0.0);
        assert_eq!(l.container_y, 752.0);
        assert_eq!(l.container_h, 48.0);
        assert_eq!(l.y, 16.0);
        assert_eq!(l.xs, [20.0, 40.0, 60.0]);
    }

    #[test]
    fn a_zoomed_bar_keeps_them_centred() {
        // egui at 110% draws the 48-point bar 52.8 points tall.
        let l = layout(800.0, 48.0 * 1.1, 14.0, 23.0, 0.0);
        let centre = l.container_h - l.y - 7.0;
        assert!((centre - 26.4).abs() <= 0.25, "{centre}");
        assert_eq!(l.y * 2.0, (l.y * 2.0).round(), "on a half point");
    }

    #[test]
    fn they_fit_the_room_the_bar_leaves() {
        let l = layout(800.0, 48.0, 16.0, 20.0, 0.0);
        let right = l.xs[2] + 14.0;
        assert!(right < f64::from(mareader_ui_egui::platform::Platform::Mac.leading_inset()));
    }

    #[test]
    fn an_unmeasured_gap_falls_back() {
        for gap in [0.0, -5.0, f64::NAN] {
            assert_eq!(layout(800.0, 48.0, 16.0, gap, 0.0).xs[1], 40.0);
        }
    }

    #[test]
    fn bad_sizes_stay_finite() {
        let l = layout(f64::NAN, 48.0, f64::INFINITY, 20.0, f64::NAN);
        assert!(l.container_y.is_finite() && l.y.is_finite() && l.xs[0].is_finite());
        assert_eq!(layout(800.0, 48.0, 90.0, 20.0, 0.0).y, 0.0);
    }

    #[test]
    fn a_closing_rail_carries_them_off_the_left_edge() {
        let l = layout(800.0, 48.0, 16.0, 20.0, -288.0);
        assert!(l.xs[2] + 14.0 < 0.0, "past the window edge, out of sight");
        assert_eq!(layout(800.0, 48.0, 16.0, 20.0, -100.0).xs[0], -80.0);
    }

    #[test]
    fn flipped_superview_places_container_at_zero() {
        let l = layout(800.0, 48.0, 16.0, 20.0, 0.0);
        assert_eq!(container_origin_y(l, false), 752.0);
        assert_eq!(container_origin_y(l, true), 0.0);
    }
}
