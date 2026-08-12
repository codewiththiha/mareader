//! Appearance presets: the built-in looks, plus user-saved combinations
//! organised into named groups.
//!
//! Sepia, Green and Night used to be hard-coded themes. They are now just
//! points in the `Appearance` space, which is the whole argument for the
//! refactor: if a preset can reproduce them exactly, then presets are
//! expressive enough to be the only mechanism, and users can build their own
//! Sepia-but-cooler without anyone writing CSS.
//!
//! CONTRACT: `Preset`/`PresetGroup` field names are the serde schema persisted
//! inside `pdfreader.settings.v1`.

use serde::{Deserialize, Serialize};

use super::appearance::{Appearance, BaseMode, NoiseMode, TextureMode};

/// A named look. `id` is stable and used for selection/highlighting; user
/// presets get a generated id so two presets may share a label without the UI
/// confusing them.
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

/// The presets that ship with the app.
///
/// The first three are the plain bases (no tint) — these are what the old
/// Light/Dark/Dim themes were. The next three reproduce the retired Sepia,
/// Green and Night themes as tints, which is the compatibility guarantee that
/// lets those CSS blocks be deleted.
pub fn builtin_presets() -> Vec<Preset> {
    vec![
        preset("light", "Light", "Basic", Appearance {
            base: BaseMode::Light,
            tint_strength: 0,
            ..Default::default()
        }),
        preset("dark", "Dark", "Basic", Appearance {
            base: BaseMode::Dark,
            tint_strength: 0,
            ..Default::default()
        }),
        preset("dim", "Dim", "Basic", Appearance {
            base: BaseMode::Dim,
            tint_strength: 0,
            ..Default::default()
        }),
        // --- the retired themes, reconstructed -------------------------------
        // Sepia was `sepia(0.35) contrast(0.95) saturate(0.9)` on light paper:
        // a warm brown at sepia()'s own hue, so no rotation and a mid strength.
        preset("sepia", "Sepia", "Classic", Appearance {
            base: BaseMode::Light,
            tint_hue: 34,
            tint_strength: 45,
            ..Default::default()
        }),
        // Green was sepia+hue-rotate(70deg) => 34 + 70 ≈ 104, a soft leaf green.
        preset("green", "Green", "Classic", Appearance {
            base: BaseMode::Light,
            tint_hue: 104,
            tint_strength: 40,
            ..Default::default()
        }),
        // Night was the dark invert with a green cast layered over it.
        preset("night", "Night", "Classic", Appearance {
            base: BaseMode::Dark,
            tint_hue: 110,
            tint_strength: 35,
            ..Default::default()
        }),
        // --- a couple that show off the new axes -----------------------------
        preset("parchment", "Parchment", "Classic", Appearance {
            base: BaseMode::Light,
            tint_hue: 40,
            tint_strength: 55,
            texture: TextureMode::Paper,
            texture_opacity: 85,
            texture_scale: 110,
            noise: NoiseMode::Static,
            noise_intensity: 18,
            ..Default::default()
        }),
        preset("cinema", "Cinema", "Classic", Appearance {
            base: BaseMode::Dim,
            tint_hue: 220,
            tint_strength: 30,
            texture: TextureMode::None,
            noise: NoiseMode::Animated,
            noise_intensity: 30,
            ..Default::default()
        }),
    ]
}

pub fn is_builtin(id: &str) -> bool {
    builtin_presets().iter().any(|p| p.id == id)
}

/// Group presets for display, preserving first-seen group order so the menu
/// does not reshuffle when a user renames or adds one.
///
/// Ungrouped presets collect under "Custom" rather than floating loose: a menu
/// with a few headed sections and a pile of unheaded rows reads as broken.
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
                out.push(PresetGroup { name, presets: vec![p.clone()] });
            }
        }
    }
    out
}

/// A URL-safe-ish slug for a user preset id, with a numeric suffix when the
/// slug is already taken so ids stay unique even for duplicate names.
pub fn make_preset_id(name: &str, existing: &[Preset]) -> String {
    // Collapse RUNS of separators, not just map them: "Café / Nuit" has three
    // non-alphanumerics in a row, and one dash per character would give
    // "caf----nuit".
    let mut slug = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-').to_string();
    let base = if slug.is_empty() { "preset".to_string() } else { slug };
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
        builtin_presets().into_iter().find(|p| p.id == id).expect(id)
    }

    #[test]
    fn the_retired_themes_survive_as_presets() {
        // This is the compatibility contract that allowed the sepia/green/night
        // CSS blocks to be deleted. If any of these disappear, users lose looks
        // they had selected.
        for id in ["sepia", "green", "night"] {
            assert!(is_builtin(id), "missing reconstructed theme {id}");
        }
        // And the plain bases are still reachable as presets too.
        for id in ["light", "dark", "dim"] {
            assert!(is_builtin(id), "missing base preset {id}");
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

        let green = find("green");
        assert_eq!(green.appearance.base, BaseMode::Light);
        // The old CSS was sepia + hue-rotate(70deg) == 34 + 70.
        assert_eq!(green.appearance.tint_hue, 104);

        let night = find("night");
        assert_eq!(night.appearance.base, BaseMode::Dark);
        assert!(night.appearance.has_tint(), "Night is dark WITH a green cast");
        assert!(night.appearance.canvas_filter().contains("invert"));
    }

    #[test]
    fn the_plain_bases_carry_no_tint() {
        for id in ["light", "dark", "dim"] {
            let p = find(id);
            assert!(!p.appearance.has_tint(), "{id} must be untinted");
            assert_eq!(p.appearance.texture, TextureMode::None);
            assert_eq!(p.appearance.noise, NoiseMode::Off);
        }
    }

    #[test]
    fn presets_capture_every_axis_not_just_colour() {
        // A preset has to restore the WHOLE look or switching to one leaves
        // stray texture/grain from whatever was set before.
        let p = find("parchment");
        assert_eq!(p.appearance.texture, TextureMode::Paper);
        assert_eq!(p.appearance.noise, NoiseMode::Static);
        assert!(p.appearance.tint_strength > 0);

        let c = find("cinema");
        assert_eq!(c.appearance.noise, NoiseMode::Animated);
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
