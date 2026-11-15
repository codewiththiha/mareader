//! Split panes' own text; the window's mirrors the focused pane's.

use std::collections::BTreeMap;

use mareader_settings::TextSettings;
use mareader_workspace::PaneId;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaneTexts {
    own: BTreeMap<PaneId, TextSettings>,
    /// The pane the window's text mirrors.
    mirrored: Option<PaneId>,
}

impl PaneTexts {
    /// Fits entries to the panes; `window` follows the focused pane's.
    pub fn reconcile(
        &mut self,
        on: bool,
        (placed, active): (&[PaneId], PaneId),
        window: &mut TextSettings,
    ) {
        // Edits since the last call belong to the mirrored pane.
        let mirrored = self.mirrored.and_then(|p| self.own.get_mut(&p));
        if let Some(text) = mirrored
            && text != window
        {
            *text = window.clone();
        }
        if !on || placed.len() < 2 {
            if let Some(text) = self.own.remove(&active) {
                *window = text;
            }
            self.own.clear();
            self.mirrored = None;
            return;
        }
        self.own.retain(|p, _| placed.contains(p));
        // A new pane starts from the focused pane's text.
        for pane in placed {
            self.own.entry(*pane).or_insert_with(|| window.clone());
        }
        if self.mirrored != Some(active) {
            if let Some(text) = self.own.get(&active) {
                *window = text.clone();
            }
            self.mirrored = Some(active);
        }
    }

    /// Each pane with text of its own, the mirrored one as `window`.
    pub fn own<'a>(&'a self, window: &'a TextSettings) -> Vec<(PaneId, &'a TextSettings)> {
        let of = |(pane, text): (&PaneId, &'a TextSettings)| {
            let text = if self.mirrored == Some(*pane) {
                window
            } else {
                text
            };
            (*pane, text)
        };
        self.own.iter().map(of).collect()
    }

    /// `pane`'s text: its own, or the window's.
    pub fn of<'a>(&'a self, pane: PaneId, window: &'a TextSettings) -> &'a TextSettings {
        match self.own.get(&pane) {
            Some(text) if self.mirrored != Some(pane) => text,
            _ => window,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sized(px: f32) -> TextSettings {
        TextSettings {
            font_size: px,
            ..TextSettings::default()
        }
    }

    const A: PaneId = PaneId(1);
    const B: PaneId = PaneId(2);

    #[test]
    fn each_pane_keeps_its_own_text_while_split() {
        let mut texts = PaneTexts::default();
        let mut window = sized(20.0);
        texts.reconcile(true, (&[A, B], A), &mut window);
        // Both start from the window's text; editing A leaves B.
        window.font_size = 30.0;
        texts.reconcile(true, (&[A, B], B), &mut window);
        assert_eq!(window.font_size, 20.0, "focusing B loads its text");
        assert_eq!(texts.of(A, &window).font_size, 30.0);
        window.font_size = 24.0;
        assert_eq!(texts.of(B, &window).font_size, 24.0, "B mirrors the window");
        texts.reconcile(true, (&[A, B], A), &mut window);
        assert_eq!(window.font_size, 30.0);
        assert_eq!(texts.of(B, &window).font_size, 24.0);
    }

    #[test]
    fn the_focused_pane_hands_its_text_to_the_window() {
        let mut texts = PaneTexts::default();
        let mut window = sized(20.0);
        texts.reconcile(true, (&[A, B], A), &mut window);
        window.font_size = 30.0;
        texts.reconcile(true, (&[A, B], B), &mut window);
        // A closes: B survives with its own text, and the mode ends.
        texts.reconcile(true, (&[B], B), &mut window);
        assert_eq!(window.font_size, 20.0);
        assert!(texts.own(&window).is_empty());
        // Turned off while split, the focused pane's text stays.
        texts.reconcile(true, (&[A, B], A), &mut window);
        window.font_size = 26.0;
        texts.reconcile(false, (&[A, B], A), &mut window);
        assert_eq!(window.font_size, 26.0);
        assert_eq!(texts.of(B, &window).font_size, 26.0);
    }

    #[test]
    fn off_every_pane_shares_the_window_text() {
        let mut texts = PaneTexts::default();
        let mut window = sized(20.0);
        texts.reconcile(false, (&[A, B], A), &mut window);
        assert!(texts.own(&window).is_empty());
        assert_eq!(texts.of(B, &window).font_size, 20.0);
    }
}
