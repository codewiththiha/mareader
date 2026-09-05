//! The base palettes: what Light / Dark / Dim mean as seven raw colours,
//! before any tint or transform. Extracted out of the old tint module so
//! both format pipelines — the PDF raster filter and the text page
//! palette — start from the same table.
//!
//! Mirrors the `:root[data-base=...]` blocks in styles/tokens.css; keep
//! the two in sync.

use crate::appearance::{Appearance, BaseMode};

/// The seven raw colours of a base mode. Each format pipeline applies its
/// own transformation on top: PDF through a CSS filter chain over the
/// raster plus UI-token overrides, text by shifting these values directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaseTokens {
    pub paper: &'static str,
    pub ink: &'static str,
    pub muted: &'static str,
    pub surface: &'static str,
    pub line: &'static str,
    pub accent: &'static str,
    pub accent_soft: &'static str,
}

impl BaseTokens {
    /// The palette as `--base-*` token pairs — the shape the preset
    /// thumbnail preview consumes.
    pub fn entries(&self) -> [(&'static str, &'static str); 7] {
        [
            ("--base-paper", self.paper),
            ("--base-ink", self.ink),
            ("--base-muted", self.muted),
            ("--base-surface", self.surface),
            ("--base-line", self.line),
            ("--base-accent", self.accent),
            ("--base-accent-soft", self.accent_soft),
        ]
    }
}

/// The raw palette for a base mode.
pub fn base_tokens(mode: BaseMode) -> BaseTokens {
    match mode {
        BaseMode::Light => BaseTokens {
            paper: "#ffffff",
            ink: "#1f2937",
            muted: "#6b7280",
            surface: "#f3f4f6",
            line: "#e5e7eb",
            accent: "#2563eb",
            accent_soft: "#dbeafe",
        },
        BaseMode::Dark => BaseTokens {
            paper: "#131316",
            ink: "#e5e7eb",
            muted: "#9ca3af",
            surface: "#1a1a1e",
            line: "#2b2b31",
            accent: "#60a5fa",
            accent_soft: "#1d2b3a",
        },
        BaseMode::Dim => BaseTokens {
            paper: "#1a1c1f",
            ink: "#c3c6cb",
            muted: "#8b8f96",
            surface: "#202328",
            line: "#2e3238",
            accent: "#7a9bd4",
            accent_soft: "#232b36",
        },
    }
}

impl Appearance {
    /// The base palette for a mode, as `(token, value)` pairs.
    ///
    /// Mirrors the `:root[data-base=...]` blocks in input.css; only used by
    /// preset thumbnails, which must carry their own look rather than inherit
    /// the live tokens. Keep the two in sync.
    pub(crate) fn base_palette(&self) -> [(&'static str, &'static str); 7] {
        base_tokens(self.base).entries()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_resolves_its_seven_tokens() {
        let light = base_tokens(BaseMode::Light);
        assert_eq!(light.paper, "#ffffff");
        assert_eq!(light.ink, "#1f2937");
        assert_eq!(light.accent, "#2563eb");

        let dark = base_tokens(BaseMode::Dark);
        assert_eq!(dark.paper, "#131316");
        assert_eq!(dark.ink, "#e5e7eb");

        let dim = base_tokens(BaseMode::Dim);
        assert_eq!(dim.paper, "#1a1c1f");
        assert_eq!(dim.accent, "#7a9bd4");
    }

    #[test]
    fn entries_emit_the_base_namespace_in_a_stable_order() {
        let names: Vec<_> = base_tokens(BaseMode::Light)
            .entries()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            names,
            [
                "--base-paper",
                "--base-ink",
                "--base-muted",
                "--base-surface",
                "--base-line",
                "--base-accent",
                "--base-accent-soft",
            ]
        );
    }
}
