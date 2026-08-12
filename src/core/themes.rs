//! Theme registry for the UI menu + persistence.
//!
//! CONTRACT: `id`s here MUST match the `:root[data-theme="..."]` blocks in
//! styles/input.css. The actual colors/filters live ONLY in CSS — Rust stores
//! just {id, label, is_dark}.

#[derive(Debug, Clone, Copy)]
pub struct ThemeDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub is_dark: bool,
}

pub const THEMES: &[ThemeDefinition] = &[
    ThemeDefinition { id: "light", label: "Light", is_dark: false },
    ThemeDefinition { id: "dark", label: "Dark", is_dark: true },
    ThemeDefinition { id: "sepia", label: "Sepia", is_dark: false },
    ThemeDefinition { id: "green", label: "Green", is_dark: false },
    ThemeDefinition { id: "night", label: "Night", is_dark: true },
    ThemeDefinition { id: "dim", label: "Dim", is_dark: true },
];

pub fn theme_by_id(id: &str) -> &'static ThemeDefinition {
    THEMES.iter().find(|t| t.id == id).unwrap_or(&THEMES[0])
}

pub fn is_valid_theme(id: &str) -> bool {
    THEMES.iter().any(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_table_matches_the_stylesheet() {
        // These ids are hardcoded in styles/input.css `:root[data-theme=...]`.
        for id in ["light", "dark", "sepia", "green", "night", "dim"] {
            assert!(is_valid_theme(id), "missing theme {id}");
        }
        // Unknown ids fall back rather than panicking.
        assert_eq!(theme_by_id("neon").id, "light");
        // The dark flag drives the blend/filter families.
        assert!(theme_by_id("dark").is_dark);
        assert!(theme_by_id("night").is_dark);
        assert!(!theme_by_id("sepia").is_dark);
    }
}
