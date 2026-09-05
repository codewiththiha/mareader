//! AI-assisted reading: the floating pill anchored to the reader's text
//! selection and the explanation card it opens.
//!
//!   * `selection_pill` — the pill a selection produces, and its anchor maths.
//!   * `anchor` / `reflow_anchor` — where a selection or a mark is on screen,
//!     per format family.
//!   * `gloss` — the explanation card, its highlight marks and the commands
//!     that drive them.
//!   * `settings` — the AI's appearance knobs, hosted by the settings modal's
//!     Theme tab.

pub mod anchor;
pub mod gloss;
pub mod reflow_anchor;
pub mod selection_pill;
pub mod settings;

/// Fixtures this feature's tests share.
///
/// The anchor watchers and the card interactions both reason about the same
/// question — a gloss origin of some height, some distance down the viewport —
/// and both were building the identical box by hand.
#[cfg(test)]
pub(crate) mod fixture {
    use ai_core::gloss::GlossBox;

    /// A mounted origin: 40 wide, `h` tall, at (100, `y`).
    pub(crate) fn origin(y: f64, h: f64) -> Option<GlossBox> {
        Some(GlossBox { x: 100.0, y, w: 40.0, h, r: 6.0 })
    }
}
