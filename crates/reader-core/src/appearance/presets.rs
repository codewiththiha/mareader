//! Appearance presets: the built-in looks plus user-saved ones.

use serde::{Deserialize, Serialize};

use super::{Appearance, BaseMode, NoiseMode, TextureMode};

/// A named look; `id` is stable for selection and highlighting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    /// User-defined section, e.g. "Night reading". Empty = ungrouped.
    #[serde(default)]
    pub group: String,
    pub appearance: Appearance,
}

/// A group header plus its presets, ready to render as a menu section.
pub struct PresetGroup {
    pub name: String,
    pub presets: Vec<Preset>,
}

fn preset(id: &str, name: &str, group: &str, appearance: Appearance) -> Preset {
    Preset {
        id: id.to_string(),
        name: name.to_string(),
        group: group.to_string(),
        appearance,
    }
}

/// The presets that ship with the app; the plain bases are not among
/// them.
pub fn builtin_presets() -> Vec<Preset> {
    // Strengths are on the doubled tint curve, full effect at 50.
    vec![
        // Sepia was a warm brown at sepia()'s own hue, mid strength.
        preset(
            "sepia",
            "Sepia",
            "Classic",
            Appearance {
                base: BaseMode::Light,
                tint_hue: 34,
                tint_strength: 23,
                ..Default::default()
            },
        ),
        // Green was a soft leaf green at hue ~104.
        preset(
            "green",
            "Green",
            "Classic",
            Appearance {
                base: BaseMode::Light,
                tint_hue: 104,
                tint_strength: 20,
                ..Default::default()
            },
        ),
        // Night was the dark invert with a green cast layered over it.
        preset(
            "night",
            "Night",
            "Classic",
            Appearance {
                base: BaseMode::Dark,
                tint_hue: 110,
                tint_strength: 18,
                ..Default::default()
            },
        ),
        preset(
            "parchment",
            "Parchment",
            "Classic",
            Appearance {
                base: BaseMode::Light,
                tint_hue: 40,
                tint_strength: 28,
                texture: TextureMode::Paper,
                texture_opacity: 85,
                texture_scale: 110,
                noise: NoiseMode::Static,
                noise_intensity: 18,
            },
        ),
        preset(
            "cinema",
            "Cinema",
            "Classic",
            Appearance {
                base: BaseMode::Dim,
                tint_hue: 220,
                tint_strength: 15,
                texture: TextureMode::None,
                noise: NoiseMode::Animated,
                noise_intensity: 30,
                ..Default::default()
            },
        ),
    ]
}

pub fn is_builtin(id: &str) -> bool {
    builtin_presets().iter().any(|p| p.id == id)
}

/// Group presets for display, preserving first-seen group order.
pub fn group_presets(presets: &[Preset]) -> Vec<PresetGroup> {
    let mut order: Vec<String> = Vec::new();
    let mut out: Vec<PresetGroup> = Vec::new();
    for p in presets {
        let name = if p.group.trim().is_empty() {
            "Custom".to_string()
        } else {
            p.group.trim().to_string()
        };
        match order.iter().position(|g| g == &name) {
            Some(i) => out[i].presets.push(p.clone()),
            None => {
                order.push(name.clone());
                out.push(PresetGroup {
                    name,
                    presets: vec![p.clone()],
                });
            }
        }
    }
    out
}

/// A slug for a user preset id, with a numeric suffix when taken.
pub fn make_preset_id(name: &str, existing: &[Preset]) -> String {
    // Collapse runs of separators, not just map them: café----nuit.
    let mut slug = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-').to_string();
    let base = if slug.is_empty() {
        "preset".to_string()
    } else {
        slug
    };
    let taken = |id: &str| existing.iter().any(|p| p.id == id) || is_builtin(id);
    if !taken(&base) {
        return base;
    }
    for n in 2..10_000 {
        let cand = format!("{base}-{n}");
        if !taken(&cand) {
            return cand;
        }
    }
    format!("{base}-x")
}

/// Every group name currently in use, for the "add to existing section"
/// dropdown when saving.
pub fn user_group_names(presets: &[Preset]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for p in presets {
        let g = p.group.trim();
        if !g.is_empty() && !seen.iter().any(|s| s == g) {
            seen.push(g.to_string());
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(id: &str) -> Preset {
        builtin_presets()
            .into_iter()
            .find(|p| p.id == id)
            .expect(id)
    }

    #[test]
    fn the_retired_themes_survive_as_presets() {
        // The compatibility contract: these keep the retired looks.
        for id in ["sepia", "green", "night"] {
            assert!(is_builtin(id), "missing reconstructed theme {id}");
        }
        // The plain bases are the Mode section's buttons, not presets.
        for id in ["light", "dark", "dim"] {
            assert!(!is_builtin(id), "{id} is a mode button, not a preset");
        }
    }

    #[test]
    fn reconstructed_themes_have_the_right_structure() {
        // Sepia/Green are LIGHT bases with a tint; Night is a DARK base.
        let sepia = find("sepia");
        assert_eq!(sepia.appearance.base, BaseMode::Light);
        assert!(sepia.appearance.has_tint());
        // Sepia sits at sepia()'s own hue, so it needs no rotation.
        assert_eq!(sepia.appearance.tint_hue, 34);
        // Halved strengths: the doubled curve renders the classic looks
        // from half the number (Appearance::tint_amount).
        assert_eq!(sepia.appearance.tint_strength, 23);

        let green = find("green");
        assert_eq!(green.appearance.base, BaseMode::Light);
        // The old CSS was sepia + hue-rotate(70deg) == 34 + 70.
        assert_eq!(green.appearance.tint_hue, 104);
        assert_eq!(green.appearance.tint_strength, 20);

        let night = find("night");
        assert_eq!(night.appearance.base, BaseMode::Dark);
        assert!(
            night.appearance.has_tint(),
            "Night is dark WITH a green cast"
        );
        assert_eq!(night.appearance.tint_strength, 18);
        assert!(night.appearance.canvas_filter().contains("invert"));
    }

    #[test]
    fn presets_capture_every_axis_not_just_colour() {
        // A preset must restore the WHOLE look.
        let p = find("parchment");
        assert_eq!(p.appearance.texture, TextureMode::Paper);
        assert_eq!(p.appearance.noise, NoiseMode::Static);
        assert_eq!(p.appearance.tint_strength, 28);

        let c = find("cinema");
        assert_eq!(c.appearance.noise, NoiseMode::Animated);
        assert_eq!(c.appearance.tint_strength, 15);
    }

    #[test]
    fn grouping_preserves_first_seen_order_and_names_the_ungrouped() {
        let ps = vec![
            preset("a", "A", "Night reading", Appearance::default()),
            preset("b", "B", "", Appearance::default()),
            preset("c", "C", "Night reading", Appearance::default()),
            preset("d", "D", "Daylight", Appearance::default()),
        ];
        let gs = group_presets(&ps);
        assert_eq!(gs.len(), 3);
        assert_eq!(gs[0].name, "Night reading");
        assert_eq!(gs[0].presets.len(), 2, "same group must collect together");
        assert_eq!(gs[1].name, "Custom", "ungrouped gets a real header");
        assert_eq!(gs[2].name, "Daylight");
    }

    #[test]
    fn ids_stay_unique_even_for_duplicate_names() {
        let mut ps: Vec<Preset> = Vec::new();
        let a = make_preset_id("My Look", &ps);
        assert_eq!(a, "my-look");
        ps.push(preset(&a, "My Look", "", Appearance::default()));
        let b = make_preset_id("My Look", &ps);
        assert_ne!(a, b, "a duplicate name must not collide");
        assert_eq!(b, "my-look-2");
    }

    #[test]
    fn user_ids_never_shadow_a_builtin() {
        // Otherwise saving a preset called "Sepia" would make the built-in
        // unreachable.
        let id = make_preset_id("Sepia", &[]);
        assert_ne!(id, "sepia");
    }

    #[test]
    fn odd_names_still_produce_a_usable_id() {
        assert_eq!(make_preset_id("  ***  ", &[]), "preset");
        assert_eq!(make_preset_id("Café / Nuit!", &[]), "caf-nuit");
    }

    #[test]
    fn group_names_are_collected_without_duplicates() {
        let ps = vec![
            preset("a", "A", "Night", Appearance::default()),
            preset("b", "B", "Night", Appearance::default()),
            preset("c", "C", "", Appearance::default()),
        ];
        assert_eq!(user_group_names(&ps), vec!["Night".to_string()]);
    }
}
