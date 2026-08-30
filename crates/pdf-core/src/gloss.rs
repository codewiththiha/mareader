//! Gloss card geometry: box math, viewport placement, spring stepping.
//!
//! Pure domain math for the AI word-card morph (the Gloss reference's
//! `src/lib/geometry.ts`, ported). No wasm, no DOM, no leptos — unit-testable
//! on the host via `cargo test -p pdf-core gloss`.
//!
//! The card is one rectangle whose `left/top/width/height` and corner
//! `radius` are all driven by a single critically-damped spring, so the chip's
//! pill radius morphs into the card radius in the same motion that grows the
//! box. Named `GlossBox` to avoid colliding with `std::boxed::Box`.

/// A positioned, sized, rounded rectangle — the five fields the spring drives.
///
/// Serializable because [`GlossMark`] persists one of these to localStorage
/// (and ships one through a `CustomEvent` detail when a mark is clicked).
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct GlossBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Corner radius — animated alongside the box so the chip's pill radius
    /// morphs into the card radius.
    pub r: f64,
}

/// A persisted gloss highlight: the word that was explained, plus WHERE it is
/// on the page in a form that survives everything the viewport does to it.
///
/// The rect is deliberately in **page space** — unscaled CSS px measured from
/// the `.pdf-page` host's origin — not in viewport space. A native
/// `Selection` cannot be persisted (it is cleared when the card opens, it dies
/// with the text-layer spans the virtualizer unmounts, and there is only ever
/// one of it), so the mark is re-projected onto the screen as
/// `host_rect.origin + rect * display_scale` every time the page mounts. That
/// is what makes the highlight survive scroll, zoom, remounts and sessions.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GlossMark {
    pub id: String,
    pub page: u32,
    pub word: String,
    pub context: String,
    /// Page space (unscaled CSS px from the page origin). Screen = rect * display_scale.
    pub rect: GlossBox,
}

/// A point of interest in *page* space (unscaled page coordinates). Unlike a
/// screen rect it survives scroll, zoom and view-mode flips: the live screen
/// box is re-derived from the page host element whenever anything moves.
///
/// Shared by the selection Info pill and the gloss card so both glue to the
/// page without each inventing its own coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct PageAnchor {
    pub page: u32,
    pub rect: GlossBox,
}

impl PageAnchor {
    pub fn from_mark(m: &GlossMark) -> Self {
        Self {
            page: m.page,
            rect: m.rect,
        }
    }
}

impl GlossMark {
    /// Whether two marks denote the same glossed spot: same page, same word,
    /// and rects starting within a CSS px of each other. The single identity
    /// definition shared by capture-time dedup and re-click toggle-to-close.
    pub fn same_spot(&self, other: &GlossMark) -> bool {
        self.page == other.page
            && self.word == other.word
            && (self.rect.x - other.rect.x).abs() < 1.0
            && (self.rect.y - other.rect.y).abs() < 1.0
    }
}

/// Longest selection still treated as a word lookup (chars, not bytes).
/// This feature is a dictionary: a word or a short phrase. Beyond this, the
/// single-POS / single-meaning `WordInfo` shape stops making sense.
pub const MAX_GLOSS_CHARS: usize = 60;

/// Soft edge of the length gate: selections up to twice [`MAX_GLOSS_CHARS`]
/// still earn a (muted, explaining) Info pill; past this the menu hides.
pub const MAX_GLOSS_HINT_CHARS: usize = MAX_GLOSS_CHARS * 2;

/// Whether `text` can be looked up as a word.
///
/// A dictionary look-up is a single token, not a phrase: the edges are
/// trimmed, then the token must be non-empty, within the length cap, and free
/// of ANY whitespace — an interior space means the reader selected several
/// words (e.g. "quick brown"), which is not a word to explain. (Surrounding
/// spaces the user grabbed by accident trim away and still count.)
pub fn is_glossable(text: &str) -> bool {
    let t = text.trim();
    !t.is_empty() && t.chars().count() <= MAX_GLOSS_CHARS && !t.chars().any(char::is_whitespace)
}

/// Whether `text` stays inside the menu's visible range. Callers use
/// [`is_glossable`] to distinguish the enabled pill from the muted hint band.
pub fn is_hintable(text: &str) -> bool {
    let t = text.trim();
    !t.is_empty() && t.chars().count() <= MAX_GLOSS_HINT_CHARS
}

/// `f64::clamp` lifted to a free fn so call sites read as the reference.
/// Whether two boxes are equal to within `epsilon` on all five fields — the
/// spring's "settled" and "already snapped" tests.
pub fn boxes_close(a: GlossBox, b: GlossBox, epsilon: f64) -> bool {
    FloatBox::from(a).close(&FloatBox::from(b), epsilon)
}

/// Smallest the card may shrink to before content stops being readable.
pub const MIN_CARD_W: f64 = 260.0;
/// Minimum card body height.
pub const MIN_CARD_H: f64 = 140.0;
/// The card never grows taller than this fraction of the viewport.
pub const MAX_CARD_H_FRAC: f64 = 0.8;

/// Gap-aware, side-aware card placement: the card goes on whichever side of
/// the anchor has more free space (never covering the stroke), sits a little
/// BELOW the mark's midline (`y_bias` — dead-centre reads as pasted onto the
/// line; a hand's-width below reads as attached to it, the way a footnote
/// hangs off its word), and clamped into the viewport margin. Shrinks the
/// card when the viewport is too small to host it at the requested size.
///
/// Pure: unit-testable on the host via `cargo test -p pdf-core gloss`.
#[allow(clippy::too_many_arguments)]
pub fn place_card(
    anchor: GlossBox,
    size_w: f64,
    size_h: f64,
    view_w: f64,
    view_h: f64,
    radius: f64,
    gap: f64,
    margin: f64,
    y_bias: f64,
) -> GlossBox {
    let w = size_w.min((view_w - margin * 2.0).max(MIN_CARD_W));
    // Guard against min > max panics on degenerate viewports.
    let h = size_h.clamp(MIN_CARD_H, (view_h * MAX_CARD_H_FRAC).max(MIN_CARD_H));
    let space_right = view_w - (anchor.x + anchor.w);
    let x = if space_right >= anchor.x {
        anchor.x + anchor.w + gap
    } else {
        anchor.x - gap - w
    };
    let x = x.clamp(margin, (view_w - w - margin).max(margin));
    let y = (anchor.y + anchor.h * 0.5 - h * 0.5 + y_bias)
        .clamp(margin, (view_h - h - margin).max(margin));
    GlossBox { x, y, w, h, r: radius }
}

/// The gloss box's escape hatch into the primitive motion layer: `GlossBox`
/// converts into [`FloatBox`] at this boundary so the domain type stays the
/// single source of truth for persisted marks while the spring mechanics
/// (including the stiffness/damping constants) stay reusable in
/// [`crate::floating`].
pub use crate::floating::FloatBox;

impl From<GlossBox> for FloatBox {
    fn from(b: GlossBox) -> Self {
        FloatBox {
            x: b.x,
            y: b.y,
            w: b.w,
            h: b.h,
            r: b.r,
        }
    }
}

impl From<FloatBox> for GlossBox {
    fn from(b: FloatBox) -> Self {
        GlossBox {
            x: b.x,
            y: b.y,
            w: b.w,
            h: b.h,
            r: b.r,
        }
    }
}

/// One spring step over all five box fields. Returns `(next_box, next_velocity)`.
pub fn step_spring(cur: GlossBox, vel: GlossBox, target: GlossBox, dt: f64) -> (GlossBox, GlossBox) {
    let (next, next_vel) = FloatBox::from(cur).step(&FloatBox::from(vel), &FloatBox::from(target), dt);
    (GlossBox::from(next), GlossBox::from(next_vel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_glossable_gate_counts_chars_and_trims() {
        assert!(is_glossable("palimpsest"));
        assert!(is_glossable("  spaced  "));
        assert!(is_glossable(&"a".repeat(MAX_GLOSS_CHARS)));
        assert!(!is_glossable(""));
        assert!(!is_glossable("   "));
        assert!(!is_glossable(&"a".repeat(MAX_GLOSS_CHARS + 1)));
        // Character count, not byte count: 60 emoji pass the gate.
        assert!(is_glossable(&"🙂".repeat(MAX_GLOSS_CHARS)));
        // Interior whitespace is a phrase, not a word — rejected.
        assert!(!is_glossable("quick brown"));
        assert!(!is_glossable("  quick brown  "));
        assert!(!is_glossable("note\ttab"));
        assert!(!is_glossable("line\nbreak"));
    }

    #[test]
    fn the_hint_band_runs_to_twice_the_cap() {
        let first_hint = "a".repeat(MAX_GLOSS_CHARS + 1);
        assert!(!is_glossable(&first_hint));
        assert!(is_hintable(&first_hint));
        assert!(is_hintable(&"a".repeat(MAX_GLOSS_HINT_CHARS)));
        assert!(!is_hintable(&"a".repeat(MAX_GLOSS_HINT_CHARS + 1)));
        assert!(!is_hintable("   "));
    }

    #[test]
    fn same_spot_tolerates_sub_pixel_drift_but_not_a_new_word() {
        let base = GlossMark {
            id: "g1".into(),
            page: 1,
            word: "palimpsest".into(),
            context: String::new(),
            rect: GlossBox { x: 100.0, y: 40.0, w: 60.0, h: 12.0, r: 0.0 },
        };

        let mut drifted = base.clone();
        drifted.id = "g2".into();
        drifted.rect.x += 0.4;
        assert!(base.same_spot(&drifted), "sub-pixel drift is the same spot");

        let mut other_word = base.clone();
        other_word.word = "palimpsests".into();
        assert!(!base.same_spot(&other_word));

        let mut other_page = base.clone();
        other_page.page = 2;
        assert!(!base.same_spot(&other_page));

        let mut moved = base.clone();
        moved.rect.y += 2.0;
        assert!(!base.same_spot(&moved));
    }

    #[test]
    fn place_card_prefers_the_roomier_side() {
        // Anchor near the left edge: plenty of room on the right.
        let anchor = GlossBox { x: 100.0, y: 400.0, w: 60.0, h: 16.0, r: 0.0 };
        let card = place_card(anchor, 360.0, 300.0, 1920.0, 1080.0, 12.0, 16.0, 12.0, 0.0);
        assert!((card.x - (anchor.x + anchor.w + 16.0)).abs() < 1e-9);
    }

    #[test]
    fn place_card_flips_left_when_the_right_edge_is_closer() {
        // space_right = 1920 - 1860 = 60 < anchor.x = 1800 → left side.
        let anchor = GlossBox { x: 1800.0, y: 400.0, w: 60.0, h: 16.0, r: 0.0 };
        let card = place_card(anchor, 360.0, 300.0, 1920.0, 1080.0, 12.0, 16.0, 12.0, 0.0);
        assert!((card.x - (anchor.x - 16.0 - card.w)).abs() < 1e-9);
    }

    #[test]
    fn place_card_stays_inside_the_viewport_margin_and_shrinks_to_fit() {
        // Tiny viewport, oversized request: everything clamps inboard.
        let anchor = GlossBox { x: 0.0, y: 0.0, w: 40.0, h: 12.0, r: 0.0 };
        let card = place_card(anchor, 800.0, 2000.0, 500.0, 400.0, 12.0, 16.0, 12.0, 0.0);
        assert!(card.x >= 12.0 - 1e-9);
        assert!(card.y >= 12.0 - 1e-9);
        assert!(card.x + card.w <= 500.0 - 12.0 + 1e-6);
        assert!(card.y + card.h <= 400.0 - 12.0 + 1e-6);
        // Height caps at 80% of the viewport, width at viewport minus margins.
        assert!((card.h - 400.0 * 0.8).abs() < 1e-9);
        assert!((card.w - (500.0 - 24.0)).abs() < 1e-9);
    }

    #[test]
    fn place_card_hangs_below_the_anchor_midline() {
        // Dead-centre read as pasted onto the line; the bias drops the card a
        // touch so it hangs off the word like a footnote. The clamp still owns
        // the last word near the edges.
        let anchor = GlossBox { x: 400.0, y: 500.0, w: 80.0, h: 20.0, r: 0.0 };
        let card = place_card(anchor, 360.0, 300.0, 1920.0, 1080.0, 12.0, 16.0, 12.0, 12.0);
        let anchor_mid = anchor.y + anchor.h * 0.5;
        let card_mid = card.y + card.h * 0.5;
        assert!((card_mid - anchor_mid - 12.0).abs() < 1e-9);
        // …and a bias that would push the card out the bottom stops at the
        // viewport margin instead of leaving the screen.
        let low = GlossBox { x: 400.0, y: 1000.0, w: 80.0, h: 20.0, r: 0.0 };
        let card = place_card(low, 360.0, 300.0, 1920.0, 1080.0, 12.0, 16.0, 12.0, 12.0);
        assert!(card.y + card.h <= 1080.0 - 12.0 + 1e-9);
    }

    #[test]
    fn a_gloss_mark_round_trips_through_json() {
        // The persistence contract: what localStorage holds must come back
        // byte-identical, because the rect IS the anchor. A silently dropped
        // field would put the highlight on the wrong word after a restart.
        let mark = GlossMark {
            id: "g3-1700000000000".to_string(),
            page: 3,
            word: "palimpsest".to_string(),
            context: "a manuscript page, a palimpsest, scraped clean".to_string(),
            rect: GlossBox { x: 120.5, y: 44.25, w: 62.0, h: 13.5, r: 0.0 },
        };
        let json = serde_json::to_string(&mark).expect("serialize");
        let back: GlossMark = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(mark, back);
    }

    #[test]
    fn boxes_close_is_field_wise_within_epsilon() {
        let a = GlossBox { x: 1.0, y: 2.0, w: 3.0, h: 4.0, r: 5.0 };
        let b = GlossBox { x: 1.1, y: 2.1, w: 3.1, h: 4.1, r: 5.1 };
        assert!(boxes_close(a, b, 0.2));
        assert!(!boxes_close(a, b, 0.05));
    }

    #[test]
    fn the_spring_converges_to_its_target_within_about_two_seconds() {
        // THE point of the spring: starting from the (small) chip box and at
        // rest, ~120 60-fps frames bring every field to within half a pixel of
        // the target. If this regresses the card either never settles (battery
        // drain from an endless rAF loop) or snaps (no morph).
        let target = GlossBox { x: 200.0, y: 150.0, w: 320.0, h: 420.0, r: 24.0 };
        let mut cur = GlossBox { x: 40.0, y: 30.0, w: 30.0, h: 20.0, r: 10.0 };
        let mut vel = GlossBox::default();
        let dt = 1.0 / 60.0;
        for _ in 0..200 {
            let (next, next_vel) = step_spring(cur, vel, target, dt);
            cur = next;
            vel = next_vel;
        }
        assert!(boxes_close(cur, target, 0.5), "did not settle: {cur:?}");
    }

    #[test]
    fn the_spring_is_stable_on_a_dropped_frame() {
        // A long frame (dt clamped at the caller) must not launch the box off to
        // infinity. Stepping at the stability ceiling stays bounded and still
        // approaches the target.
        let target = GlossBox { x: 0.0, y: 0.0, w: 300.0, h: 300.0, r: 20.0 };
        let mut cur = GlossBox::default();
        let mut vel = GlossBox::default();
        // Several successive "dropped" frames at the caller's clamp (0.032s).
        for _ in 0..400 {
            let (next, next_vel) = step_spring(cur, vel, target, 0.032);
            cur = next;
            vel = next_vel;
            assert!(cur.x.is_finite() && cur.w.is_finite(), "blew up: {cur:?}");
        }
        assert!(boxes_close(cur, target, 0.5), "did not settle on long frames: {cur:?}");
    }
}
