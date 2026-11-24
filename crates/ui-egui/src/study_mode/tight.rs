//! The deck in a short or narrow pane: how its height is shared.
//!
//! A pane tall and wide enough keeps the roomy deck, its card shrinking a
//! little first. Shorter or narrower, the deck turns compact: the row over
//! the card goes, the time left and the count move onto the card, the steps
//! onto its edges, and the card is as tall as its longest back wants, as
//! far as the pane allows. Shorter still, the compact deck scrolls.

use egui::{Context, Id};
use mareader_workspace::PaneId;

/// Room kept under the deck, clear of the pane's corner marks.
pub const FOOT: f32 = 12.0;
/// The deck's least drop below the pane's top, clear of its label; less
/// when compact.
const HEAD: f32 = 22.0;
const LOW: f32 = 8.0;
/// The tallest a card grows for a long back.
pub const TALLEST: f32 = 320.0;
/// The shortest compact card, and the least a roomy deck's card shrinks to.
const LEAST: f32 = 150.0;
const ROOMY: f32 = 190.0;

/// How the deck lays out in its pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// No row over the card, and one row for the steps and the known button.
    pub compact: bool,
    /// The card's height.
    pub card: f32,
    /// Even compact, the deck is taller than the pane.
    pub scrolls: bool,
}

/// What the deck takes besides its card, per layout, as last measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chrome {
    pub roomy: f32,
    pub compact: f32,
}

impl Default for Chrome {
    fn default() -> Self {
        Self {
            roomy: 250.0,
            compact: 100.0,
        }
    }
}

/// The layout for `room` points of height, the card wanting `card` points;
/// a `narrow` deck is always compact.
pub fn plan(room: f32, card: f32, chrome: Chrome, narrow: bool) -> Fit {
    let roomy = (room - HEAD - chrome.roomy).min(card);
    if !narrow && roomy >= ROOMY.min(card) {
        let (compact, scrolls) = (false, false);
        return Fit {
            compact,
            card: roomy,
            scrolls,
        };
    }
    let least = LEAST.min(card);
    let left = room - LOW - chrome.compact;
    let (compact, scrolls) = (true, left < least);
    Fit {
        compact,
        card: left.clamp(least, card),
        scrolls,
    }
}

/// The deck's drop below the pane's top under `fit`.
pub fn head(fit: Fit) -> f32 {
    if fit.compact { LOW } else { HEAD }
}

fn chrome_id(pane: PaneId) -> Id {
    Id::new(("study-deck-chrome", pane))
}

/// `pane`'s deck as last measured.
pub fn chrome(ctx: &Context, pane: PaneId) -> Chrome {
    ctx.data(|d| d.get_temp(chrome_id(pane)))
        .unwrap_or_default()
}

/// Keeps what the deck took besides its card, `used` in all, under `fit`.
pub fn measured(ctx: &Context, pane: PaneId, fit: Fit, used: f32) {
    let mut kept = chrome(ctx, pane);
    let side = match fit.compact {
        true => &mut kept.compact,
        false => &mut kept.roomy,
    };
    let took = used - fit.card;
    if (*side - took).abs() > 0.5 {
        *side = took;
        ctx.data_mut(|d| d.insert_temp(chrome_id(pane), kept));
        ctx.request_discard("the deck's room changed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tall_pane_keeps_the_roomy_deck() {
        let fit = plan(800.0, 300.0, Chrome::default(), false);
        assert_eq!((fit.compact, fit.card, fit.scrolls), (false, 300.0, false));
        let shrunk = plan(480.0, 300.0, Chrome::default(), false);
        assert!(!shrunk.compact && shrunk.card < 300.0, "{shrunk:?}");
    }

    #[test]
    fn a_short_pane_turns_compact_and_then_scrolls() {
        let short = plan(380.0, 300.0, Chrome::default(), false);
        assert!(short.compact && !short.scrolls, "{short:?}");
        assert!((short.card - 272.0).abs() < 1e-3, "{short:?}");
        let tiny = plan(200.0, 300.0, Chrome::default(), false);
        assert!(
            tiny.compact && tiny.scrolls && tiny.card == LEAST,
            "{tiny:?}"
        );
    }

    #[test]
    fn a_narrow_deck_is_always_compact() {
        let fit = plan(1000.0, 200.0, Chrome::default(), true);
        assert!(fit.compact && fit.card == 200.0, "{fit:?}");
    }

    #[test]
    fn a_narrow_card_never_grows_past_its_shape() {
        let fit = plan(1000.0, 150.0, Chrome::default(), false);
        assert_eq!(fit.card, 150.0);
        let short = plan(200.0, 100.0, Chrome::default(), false);
        assert!(short.card <= 100.0, "{short:?}");
    }
}
