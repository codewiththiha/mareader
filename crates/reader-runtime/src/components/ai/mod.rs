//! AI-assisted reading: the pill a selection produces and the card it opens.

pub mod anchor;
pub mod gloss;
pub mod reflow_anchor;
pub mod selection_pill;
pub mod settings;

/// Fixtures this feature's tests share: the gloss origin box.
#[cfg(test)]
pub(crate) mod fixture {
    use ai_core::gloss::GlossBox;

    /// A mounted origin: 40 wide, `h` tall, at (100, `y`).
    pub(crate) fn origin(y: f64, h: f64) -> Option<GlossBox> {
        Some(GlossBox {
            x: 100.0,
            y,
            w: 40.0,
            h,
            r: 6.0,
        })
    }
}
