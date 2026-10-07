//! The raster pipeline's appearance tokens and its engine dispatch; this
//! crate never names the engine.

use leptos::prelude::request_animation_frame;
use reader_core::appearance::Appearance;

thread_local! {
    /// One next-frame broadcast reads all final pane inputs, however many
    /// roots repainted.
    static PANE_REFRESH_PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The seven `--color-*` tokens the tint may override, cleared as a set.
pub const UI_TOKENS: [&str; 7] = [
    "--color-paper",
    "--color-surface",
    "--color-line",
    "--color-ink",
    "--color-muted",
    "--color-accent",
    "--color-accent-soft",
];

/// The variables the PDF pipeline paints: the canvas pair and the tint.
pub fn token_vars(a: &Appearance) -> Vec<(&'static str, String)> {
    let mut vars = vec![
        ("--canvas-filter", a.canvas_filter()),
        ("--canvas-blend", a.canvas_blend().to_string()),
    ];
    vars.extend(a.ui_overrides());
    vars
}

/// Re-bake the theme into every raster the host's engine holds.
pub fn refresh_theme() {
    app_chrome::appearance_hooks::refresh_theme();
}

/// Refresh after a pane-local token paint, coalesced per frame.
pub fn refresh_after_pane_paint() {
    let schedule = PANE_REFRESH_PENDING.with(|pending| !pending.replace(true));
    if !schedule {
        return;
    }
    request_animation_frame(|| {
        PANE_REFRESH_PENDING.with(|pending| pending.set(false));
        app_chrome::appearance_hooks::refresh_theme();
    });
}

/// Enter/leave scrub: raw rasters under live CSS while a drag repaints.
pub fn set_scrub_mode(on: bool) {
    app_chrome::appearance_hooks::set_scrub_mode(on);
}

/// Whether the appearance menu is open, so raws are retained.
pub fn set_appearance_menu_open(on: bool) {
    app_chrome::appearance_hooks::set_appearance_menu_open(on);
}
