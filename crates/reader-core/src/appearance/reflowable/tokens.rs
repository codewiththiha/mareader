//! The `--tx-*` CSS custom properties for the text page palette — what
//! `styles/text.css` resolves the paper, ink, accents and chrome of a
//! text/Markdown page through.
//!
//! Written on `<html>` alongside the PDF pipeline's `--canvas-*` /
//! `--color-*` variables; the two namespaces are disjoint, so painting
//! both every appearance change costs nothing and lets a format swap
//! repaint with no extra wiring.
//!
//! The ink dial resolves HERE, not in the stylesheet: `--tx-ink` is the
//! palette ink mixed toward the paper by the dial's percentage, and the
//! markdown ink tints (blockquote rule, code chips, table borders…)
//! are precomposed over the paper at their fixed percentages. A slider
//! drag therefore writes N flat custom properties — the stylesheet no
//! longer re-evaluates a chain of live `color-mix()` rules across every
//! mounted block on every tick.

use crate::appearance::Appearance;
use super::palette::{TextPalette, mix_toward_paper};

/// The ink dial as a fraction of the palette ink retained (0..=1).
fn ink_keep(ink_contrast: f64) -> f64 {
    ink_contrast.clamp(0.0, 100.0) / 100.0
}

/// The palette, flattened to the names the stylesheet consumes, with the
/// ink dial (0..=100, 100 = the palette's full ink) applied.
pub fn css_variables(a: &Appearance, ink_contrast: f64) -> Vec<(&'static str, String)> {
    let p = TextPalette::compute(a);
    let ink = mix_toward_paper(&p.ink, &p.paper, ink_keep(ink_contrast));
    // The markdown ink tints, composited over the paper at the same
    // percentages the old color-mix() rules used.
    let ink_soft = mix_toward_paper(&ink, &p.paper, 0.78);
    let ink_border = mix_toward_paper(&ink, &p.paper, 0.25);
    let ink_code_bg = mix_toward_paper(&ink, &p.paper, 0.07);
    let ink_pre_bg = mix_toward_paper(&ink, &p.paper, 0.05);
    let ink_pre_border = mix_toward_paper(&ink, &p.paper, 0.12);
    let ink_table_border = mix_toward_paper(&ink, &p.paper, 0.18);
    let ink_table_head = mix_toward_paper(&ink, &p.paper, 0.06);
    let ink_hr = mix_toward_paper(&ink, &p.paper, 0.20);

    vec![
        ("--tx-paper", p.paper.clone()),
        ("--tx-ink", ink),
        ("--tx-muted", p.muted.clone()),
        ("--tx-surface", p.surface.clone()),
        ("--tx-line", p.line.clone()),
        ("--tx-accent", p.accent.clone()),
        ("--tx-accent-soft", p.accent_soft.clone()),
        ("--tx-ink-soft", ink_soft),
        ("--tx-ink-border", ink_border),
        ("--tx-ink-code-bg", ink_code_bg),
        ("--tx-ink-pre-bg", ink_pre_bg),
        ("--tx-ink-pre-border", ink_pre_border),
        ("--tx-ink-table-border", ink_table_border),
        ("--tx-ink-table-head", ink_table_head),
        ("--tx-ink-hr", ink_hr),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::Appearance;

    #[test]
    fn the_variables_carry_the_palette_in_a_stable_order() {
        let a = Appearance::default();
        let p = TextPalette::compute(&a);
        let vars = css_variables(&a, 100.0);
        assert_eq!(vars.len(), 15);
        assert_eq!(vars[0], ("--tx-paper", p.paper.clone()));
        assert_eq!(vars[1], ("--tx-ink", p.ink.clone()), "full contrast = the palette ink");
        assert_eq!(vars[2], ("--tx-muted", p.muted.clone()));
        assert_eq!(vars[3], ("--tx-surface", p.surface.clone()));
        assert_eq!(vars[4], ("--tx-line", p.line.clone()));
        assert_eq!(vars[5], ("--tx-accent", p.accent.clone()));
        assert_eq!(vars[6], ("--tx-accent-soft", p.accent_soft.clone()));
        for name in [
            "--tx-ink-soft",
            "--tx-ink-border",
            "--tx-ink-code-bg",
            "--tx-ink-pre-bg",
            "--tx-ink-pre-border",
            "--tx-ink-table-border",
            "--tx-ink-table-head",
            "--tx-ink-hr",
        ] {
            assert!(vars.iter().any(|(k, _)| *k == name), "{name} missing");
        }
    }

    #[test]
    fn the_ink_dial_mixes_the_ink_toward_the_paper() {
        let a = Appearance::default();
        let p = TextPalette::compute(&a);
        let full = css_variables(&a, 100.0);
        let half = css_variables(&a, 50.0);
        let ink = |vars: &Vec<(&'static str, String)>| vars.iter().find(|(k, _)| *k == "--tx-ink").unwrap().1.clone();
        // Half contrast sits between the full ink and the paper (both are
        // literals; parse lightness out of the oklch() forms and the hex).
        let full_ink = ink(&full);
        let half_ink = ink(&half);
        let l_of = |v: &str| {
            if let Ok(v) = parse_oklch(v) {
                v.0
            } else {
                crate::appearance::shared::oklch::hex_to_oklch(v).unwrap().0
            }
        };
        let full_l = l_of(&full_ink);
        let half_l = l_of(&half_ink);
        let paper_l = parse_oklch(&p.paper).unwrap().0;
        assert!(full_l < half_l, "full {full_l} vs half {half_l}");
        assert!(half_l < paper_l, "half {half_l} vs paper {paper_l}");
        assert_eq!(full_ink, p.ink);
    }

    #[test]
    fn the_ink_dial_clamps_out_of_range_values() {
        let a = Appearance::default();
        assert_eq!(
            css_variables(&a, 500.0)[1].1,
            css_variables(&a, 100.0)[1].1,
            "over-100 is the palette ink"
        );
        // 0% flattens the ink into the paper: its lightness and chroma
        // land on the paper's (the hue is kept, and irrelevant at C=0).
        let zeroed = css_variables(&a, 0.0);
        let (l, c, _) = parse_oklch(&zeroed[1].1).unwrap();
        let paper = TextPalette::compute(&a).paper;
        let (pl, pc, _) = parse_oklch(&paper).unwrap();
        assert!((l - pl).abs() < 1e-9, "0% ink L {l} vs paper L {pl}");
        assert!(c < 0.001 && pc < 0.001, "0% ink C {c}, paper C {pc}");
    }

    /// (L, C, H) out of an `oklch(...)` literal.
    fn parse_oklch(value: &str) -> Result<(f64, f64, f64), ()> {
        let inner = value.trim().strip_prefix("oklch(").ok_or(())?.strip_suffix(')').ok_or(())?;
        let mut it = inner.split_whitespace().map(|x| x.parse::<f64>());
        Ok((
            it.next().ok_or(())?.map_err(|_| ())?,
            it.next().ok_or(())?.map_err(|_| ())?,
            it.next().ok_or(())?.map_err(|_| ())?,
        ))
    }
}
