//! Tokens derived from the appearance, applied to egui once per change.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke,
};
use mareader_appearance::{Appearance, PageTone, Rgb};

/// Chrome geometry, not a theme choice: the hover band follows it.
pub const TITLE_BAR_H: f32 = 48.0;

/// Named families; only usable from the frame after [`install_fonts`].
pub const MEDIUM: &str = "inter-medium";
pub const SEMIBOLD: &str = "inter-semibold";
/// Literata, the reading face: drawn covers set their initial in it.
pub const SERIF: &str = "literata";

/// The glass wash's alpha; more hid the blurred pages under the bars.
const FROST: u8 = 150;

/// Everything a view or widget may ask the theme for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub paper: Color32,
    /// How the page pass themes a page.
    pub page: PageTone,
    /// A book's page: paper and ink, no filter.
    pub book: PageTone,
    pub ink: Color32,
    pub muted: Color32,
    pub surface: Color32,
    pub line: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    /// The fill that marks a pick; clear of the card in every look.
    pub selected: Color32,
    /// Grouped rows' fill: a pale wash of the look, off the surface.
    pub card: Color32,
    /// A switch's track while off, so it reads on every ground.
    pub track: Color32,
    /// Errors: a toast's icon and edge.
    pub danger: Color32,
    /// The title bar fill without a blurred backdrop.
    pub glass: Color32,
    /// The bar over its blurred backdrop: mareader's 72 % surface.
    pub frost: Color32,
    pub shadow: Color32,
    pub dark: bool,
    /// Buttons, fields, menu rows.
    pub radius: f32,
    /// Popovers, windows, cards.
    pub radius_lg: f32,
    /// Square toolbar button.
    pub control: f32,
    pub icon: f32,
    pub text: f32,
    pub text_small: f32,
    pub gap: f32,
}

/// An appearance colour as egui's type.
/// Amber for a heads-up, not an error.
pub fn caution(t: &Tokens) -> Color32 {
    match t.dark {
        true => Color32::from_rgb(0xfb, 0xbf, 0x24),
        false => Color32::from_rgb(0xb4, 0x53, 0x09),
    }
}

pub fn color(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

impl Tokens {
    /// Pure: the same appearance always gives the same tokens.
    pub fn new(a: &Appearance) -> Self {
        let t = a.ui_tokens();
        let (surface, line, ink) = (color(t.surface), color(t.line), color(t.ink));
        let dark = a.base.is_dark();
        let (paper, accent_soft) = (color(t.paper), color(t.accent_soft));
        let marks = a.ui_marks();
        Self {
            paper,
            page: a.page_tone(),
            book: a.text_tone(),
            ink,
            muted: color(t.muted),
            surface,
            line,
            hover: color(marks.hover),
            pressed: line.lerp_to_gamma(ink, 0.12),
            accent: color(t.accent),
            accent_soft,
            selected: color(marks.selected),
            card: color(marks.card),
            track: line.lerp_to_gamma(ink, if dark { 0.18 } else { 0.22 }),
            // Light error text reads on every ground a tint colours.
            danger: match dark {
                true => Color32::from_rgb(0xf8, 0x71, 0x71),
                false => color(mareader_appearance::legible(
                    Rgb(0xdc, 0x26, 0x26),
                    &[t.paper, t.surface, marks.card, marks.selected],
                )),
            },
            glass: Color32::from_rgba_unmultiplied(surface.r(), surface.g(), surface.b(), 245),
            // Thin enough that pages under a bar read as soft shapes.
            frost: Color32::from_rgba_unmultiplied(surface.r(), surface.g(), surface.b(), FROST),
            shadow: Color32::from_black_alpha(if dark { 120 } else { 60 }),
            dark,
            radius: 8.0,
            radius_lg: 12.0,
            control: 36.0,
            icon: 18.0,
            text: 14.0,
            text_small: 12.0,
            gap: 8.0,
        }
    }

    /// The tokens [`apply`] last stored, or the default look's.
    pub fn get(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<Tokens>(egui::Id::new(TOKENS_ID)))
            .unwrap_or_else(|| Self::new(&Appearance::default()))
    }

    /// Runs `f` in this look, menus too; then the window's comes back.
    pub fn lend<R>(self, ctx: &egui::Context, f: impl FnOnce() -> R) -> R {
        let window = Self::get(ctx);
        if window == self {
            return f();
        }
        let kept = ctx.global_style().visuals.clone();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(TOKENS_ID), self));
        ctx.set_visuals(visuals(&self));
        let out = f();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(TOKENS_ID), window));
        ctx.set_visuals(kept);
        out
    }

    /// Colours `k` of the way from these to `to`'s; the rest are `to`'s.
    pub fn toward(&self, to: &Self, k: f32) -> Self {
        let mix = |a: Color32, b: Color32| a.lerp_to_gamma(b, k.clamp(0.0, 1.0));
        Self {
            paper: mix(self.paper, to.paper),
            ink: mix(self.ink, to.ink),
            muted: mix(self.muted, to.muted),
            surface: mix(self.surface, to.surface),
            line: mix(self.line, to.line),
            hover: mix(self.hover, to.hover),
            pressed: mix(self.pressed, to.pressed),
            accent: mix(self.accent, to.accent),
            accent_soft: mix(self.accent_soft, to.accent_soft),
            selected: mix(self.selected, to.selected),
            card: mix(self.card, to.card),
            track: mix(self.track, to.track),
            danger: mix(self.danger, to.danger),
            glass: mix(self.glass, to.glass),
            frost: mix(self.frost, to.frost),
            shadow: mix(self.shadow, to.shadow),
            ..*to
        }
    }
}

const TOKENS_ID: &str = "mareader-tokens";

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // The reader's copy: embedding Inter again here cost 1.25 MB.
    let [regular, medium, semibold] = mareader_reflow::inter();
    let faces = [
        ("inter-regular", regular),
        (MEDIUM, medium),
        (SEMIBOLD, semibold),
        (SERIF, mareader_reflow::literata()),
    ];
    for (name, bytes) in faces {
        fonts.font_data.insert(
            name.into(),
            std::sync::Arc::new(FontData::from_static(bytes)),
        );
    }
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter-regular".into());
    for name in [MEDIUM, SEMIBOLD, SERIF] {
        let mut chain = vec![name.to_owned()];
        chain.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), chain);
    }
    ctx.set_fonts(fonts);
}

/// Black on the light accents, white on the deep ones: 4.58:1 at worst.
pub fn ink_on(fill: Color32) -> Color32 {
    let ground = Rgb(fill.r(), fill.g(), fill.b());
    if ground.contrast(Rgb(0, 0, 0)) > ground.contrast(Rgb(0xff, 0xff, 0xff)) {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn serif(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SERIF.into()))
}

fn radius(r: f32) -> CornerRadius {
    CornerRadius::same(r.round().clamp(0.0, 255.0) as u8)
}

/// Restyles every stock widget from `t` and stores it for [`Tokens::get`].
pub fn apply(ctx: &egui::Context, t: &Tokens) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(TOKENS_ID), *t));
    ctx.set_visuals(visuals(t));
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(t.gap, t.gap * 0.75);
        s.spacing.button_padding = egui::vec2(t.gap + 2.0, t.gap * 0.75);
        s.spacing.interact_size.y = (t.control * 0.75).round();
        s.spacing.menu_margin = Margin::same((t.gap * 0.75) as i8);
        s.spacing.window_margin = Margin::same((t.gap * 2.0) as i8);
        s.spacing.scroll = egui::style::ScrollStyle::floating();
        s.text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(t.text));
        s.text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(t.text));
        s.text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(t.text_small));
        s.text_styles
            .insert(egui::TextStyle::Heading, medium(t.text + 4.0));
        s.interaction.tooltip_delay = 0.35;
    });
}

/// The stock widgets' look from `t`; a pane may scope its own.
pub fn visuals(t: &Tokens) -> egui::Visuals {
    let mut v = if t.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = t.surface;
    v.window_fill = t.paper;
    v.window_stroke = Stroke::new(1.0, t.line);
    v.extreme_bg_color = t.surface;
    v.faint_bg_color = t.surface;
    v.code_bg_color = t.surface;
    v.selection.bg_fill = t.selected;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent;
    v.override_text_color = Some(t.ink);
    v.window_corner_radius = radius(t.radius_lg);
    v.menu_corner_radius = radius(t.radius_lg);
    let float = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: t.shadow,
    };
    v.window_shadow = float;
    v.popup_shadow = float;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.text_options.subpixel_binning = false;
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.surface;
    w.noninteractive.weak_bg_fill = t.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.ink);
    w.noninteractive.corner_radius = radius(t.radius);
    // Inactive buttons stay frameless; fields and sliders use bg_fill.
    for (state, bg, weak) in [
        (&mut w.inactive, t.line, Color32::TRANSPARENT),
        (&mut w.hovered, t.hover, t.hover),
        (&mut w.active, t.pressed, t.pressed),
        (&mut w.open, t.hover, t.hover),
    ] {
        state.bg_fill = bg;
        state.weak_bg_fill = weak;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.5, t.ink);
        state.corner_radius = radius(t.radius);
        state.expansion = 0.0;
    }
    v
}

/// Debug builds: `MAREADER_TOKENS=tokens.json` overrides tokens live.
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
pub mod live {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use egui::Color32;

    use super::Tokens;

    const POLL: Duration = Duration::from_millis(400);

    /// Polls the file only when the variable is set, so idle stays idle.
    #[derive(Default)]
    pub struct LiveTokens {
        path: Option<PathBuf>,
        stamp: Option<SystemTime>,
        overrides: serde_json::Map<String, serde_json::Value>,
    }

    impl LiveTokens {
        pub fn from_env() -> Self {
            Self {
                path: std::env::var_os("MAREADER_TOKENS").map(PathBuf::from),
                ..Self::default()
            }
        }

        /// True when the file changed since the last call.
        pub fn poll(&mut self, ctx: &egui::Context) -> bool {
            let Some(path) = &self.path else {
                return false;
            };
            ctx.request_repaint_after(POLL);
            let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            if stamp == self.stamp {
                return false;
            }
            self.stamp = stamp;
            let text = std::fs::read_to_string(path).unwrap_or_default();
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(serde_json::Value::Object(map)) => self.overrides = map,
                _ => eprintln!("MAREADER_TOKENS: expected a JSON object"),
            }
            true
        }

        pub fn overlay(&self, t: &mut Tokens) {
            for name in overlay(t, &self.overrides) {
                eprintln!("MAREADER_TOKENS: unknown or bad key {name}");
            }
        }
    }

    fn hex(v: &serde_json::Value) -> Option<Color32> {
        let s = v.as_str()?.trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Color32::from_rgba_unmultiplied(
                byte(0)?,
                byte(2)?,
                byte(4)?,
                byte(6)?,
            )),
            _ => None,
        }
    }

    macro_rules! fields {
        ($t:ident, $key:ident, $val:ident; colours: $($c:ident),*; numbers: $($n:ident),*) => {
            match $key.as_str() {
                $(stringify!($c) => hex($val).map(|c| $t.$c = c).is_some(),)*
                $(stringify!($n) => $val.as_f64().map(|x| $t.$n = x as f32).is_some(),)*
                _ => false,
            }
        };
    }

    /// Applies `map` onto `t`; returns the keys it could not use.
    pub fn overlay(
        t: &mut Tokens,
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Vec<String> {
        let mut bad = Vec::new();
        for (key, val) in map {
            let used = fields!(t, key, val;
                colours: paper, ink, muted, surface, line, hover, pressed, accent,
                    accent_soft, selected, glass, frost, shadow;
                numbers: radius, radius_lg, control, icon, text, text_small, gap);
            if !used {
                bad.push(key.clone());
            }
        }
        bad
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_buttons_read_in_every_preset() {
        let contrast =
            |a: Color32, b: Color32| Rgb(a.r(), a.g(), a.b()).contrast(Rgb(b.r(), b.g(), b.b()));
        for p in mareader_appearance::builtin_presets() {
            let t = Tokens::new(&p.appearance);
            for fill in [t.accent, t.danger] {
                let ratio = contrast(ink_on(fill), fill);
                assert!(ratio >= 4.5, "{}: {ratio:.2} on {fill:?}", p.name);
            }
        }
    }

    #[test]
    fn error_text_reads_on_every_light_tint() {
        let rgb = |c: Color32| Rgb(c.r(), c.g(), c.b());
        for hue in (0..360).step_by(10) {
            let a = Appearance {
                tint_hue: hue,
                tint_strength: 50,
                ..Appearance::default()
            };
            let t = Tokens::new(&a);
            for ground in [t.paper, t.surface, t.card, t.selected] {
                let ratio = rgb(t.danger).contrast(rgb(ground));
                assert!(ratio >= 4.5, "{hue}: {ratio:.2} on {ground:?}");
            }
        }
    }

    #[test]
    fn picks_stand_off_the_card_in_every_preset() {
        let rgb = |c: Color32| Rgb(c.r(), c.g(), c.b());
        for p in mareader_appearance::builtin_presets() {
            let t = Tokens::new(&p.appearance);
            let ratio = rgb(t.selected).contrast(rgb(t.card));
            assert!(ratio >= 1.1, "{}: {ratio:.2}", p.name);
        }
    }

    #[test]
    fn apply_styles_every_widget_state_from_the_tokens() {
        let ctx = egui::Context::default();
        let mut t = Tokens::new(&Appearance::default());
        t.radius = 3.0;
        t.hover = Color32::from_rgb(1, 2, 3);
        apply(&ctx, &t);
        let v = ctx.global_style().visuals.clone();
        assert_eq!(v.widgets.hovered.bg_fill, t.hover);
        for w in [
            v.widgets.noninteractive,
            v.widgets.inactive,
            v.widgets.hovered,
            v.widgets.active,
            v.widgets.open,
        ] {
            assert_eq!(w.corner_radius, CornerRadius::same(3));
        }
        assert_eq!(Tokens::get(&ctx), t);
    }

    #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
    #[test]
    fn live_overrides_set_known_keys_and_report_the_rest() {
        let mut t = Tokens::new(&Appearance::default());
        let json = r##"{"accent": "#ff0000", "radius": 4, "nope": 1, "ink": "red"}"##;
        let map = match serde_json::from_str(json) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        let mut bad = live::overlay(&mut t, &map);
        bad.sort();
        assert_eq!(t.accent, Color32::from_rgb(255, 0, 0));
        assert_eq!(t.radius, 4.0);
        assert_eq!(bad, ["ink", "nope"]);
    }
}
