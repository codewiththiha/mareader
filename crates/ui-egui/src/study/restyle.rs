//! A highlight's look, changed from its menu: its pen and colour.

use egui::{CornerRadius, Rect, Sense, Stroke, Ui, WidgetInfo, WidgetType, vec2};
use mareader_appearance::Rgb;
use mareader_library::{MarkId, Pen, Style};
use mareader_settings::{HighlightSettings, Reserved};

use super::bar::pen_icon;
use crate::theme::{Tokens, color};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::study::Action;

const SWATCH: f32 = 22.0;
const PEN_ROW: f32 = 46.0;

/// The colour a style shows: a reserved pick gives way to the pen's.
pub fn shown(style: Style, look: &HighlightSettings) -> Rgb {
    let picked = style.color.map(|[r, g, b]| Rgb(r, g, b));
    picked
        .filter(|c| look.allowed(style.pen, *c))
        .unwrap_or(look.color(style.pen))
}

/// The pen's own colour is kept as none, so it follows Settings.
fn style(pen: Pen, c: Rgb, look: &HighlightSettings) -> Style {
    let color = (c != look.color(pen)).then_some([c.0, c.1, c.2]);
    Style { pen, color }
}

/// Another pen keeps the colour shown, unless that look is reserved.
pub fn with_pen(now: Style, pen: Pen, look: &HighlightSettings) -> Style {
    let c = shown(now, look);
    match look.allowed(pen, c) {
        true => style(pen, c, look),
        false => Style { pen, color: None },
    }
}

/// Another colour, if the pen may take it.
pub fn with_color(now: Style, c: Rgb, look: &HighlightSettings) -> Option<Style> {
    look.allowed(now.pen, c).then(|| style(now.pen, c, look))
}

/// The pen's own colour, the palette, and the colour shown if neither.
pub fn offered(pen: Pen, now: Rgb, look: &HighlightSettings) -> Vec<Rgb> {
    let own = look.color(pen);
    let mut out = vec![own];
    out.extend(HighlightSettings::PALETTE.into_iter().filter(|c| *c != own));
    if !out.contains(&now) {
        out.push(now);
    }
    out
}

/// Why a look can't be used, if it can't.
pub fn kept_for(pen: Pen, c: Rgb, look: &HighlightSettings) -> Option<&'static str> {
    let wanted = Reserved { pen, color: c };
    if look.ai == wanted {
        Some("Kept for AI marks")
    } else if look.cefr == wanted {
        Some("Kept for CEFR shading")
    } else {
        None
    }
}

/// What a section shows as current: one mark's look, or a group's.
#[derive(Clone, Debug, PartialEq)]
pub struct Current {
    /// Each pen in use, and each colour shown, once.
    pens: Vec<Pen>,
    colors: Vec<Rgb>,
    /// The first look, to preview what another pen would show.
    first: Style,
}

impl Current {
    /// The looks of `styles`; none when there are none.
    pub fn of(styles: &[Style], look: &HighlightSettings) -> Option<Self> {
        let first = *styles.first()?;
        let (mut pens, mut colors) = (Vec::new(), Vec::new());
        for s in styles {
            if !pens.contains(&s.pen) {
                pens.push(s.pen);
            }
            let c = shown(*s, look);
            if !colors.contains(&c) {
                colors.push(c);
            }
        }
        Some(Self {
            pens,
            colors,
            first,
        })
    }

    /// The pen, when they share one.
    fn pen(&self) -> Option<Pen> {
        (self.pens.len() == 1).then_some(self.first.pen)
    }

    /// The colour, when they all show one.
    fn color(&self) -> Option<Rgb> {
        match self.colors.as_slice() {
            [c] => Some(*c),
            _ => None,
        }
    }

    /// Why `c` can't be taken by any of them, if it can't.
    fn kept(&self, c: Rgb, look: &HighlightSettings) -> Option<&'static str> {
        let mut why = self.pens.iter().map(|p| kept_for(*p, c, look));
        let first = why.next().flatten()?;
        why.all(|w| w.is_some()).then_some(first)
    }
}

/// A change picked in a section: a pen, or a colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Pen(Pen),
    Color(Rgb),
}

/// One mark's pens and colours; each applies at once.
pub fn section(
    ui: &mut Ui,
    mark: MarkId,
    now: Style,
    look: &HighlightSettings,
    t: &Tokens,
    out: &mut Commands,
) {
    let Some(current) = Current::of(&[now], look) else {
        return;
    };
    let next = match picker(ui, &current, look, t) {
        Some(Pick::Pen(pen)) if pen != now.pen => Some(with_pen(now, pen, look)),
        Some(Pick::Color(c)) => with_color(now, c, look),
        _ => None,
    };
    if let Some(next) = next.filter(|n| *n != now) {
        out.push(Sent::Engine(Command::Study(Action::Restyle(mark, next))));
    }
}

/// Several marks' pens and colours; a pick restyles each it may.
pub fn group(
    ui: &mut Ui,
    marks: &[(MarkId, Style)],
    look: &HighlightSettings,
    t: &Tokens,
    out: &mut Commands,
) {
    let styles: Vec<Style> = marks.iter().map(|(_, s)| *s).collect();
    let Some(current) = Current::of(&styles, look) else {
        return;
    };
    let Some(pick) = picker(ui, &current, look, t) else {
        return;
    };
    let mut skipped = 0;
    let mut changes = Vec::new();
    for (id, now) in marks {
        let next = match pick {
            Pick::Pen(pen) => Some(with_pen(*now, pen, look)),
            Pick::Color(c) => with_color(*now, c, look),
        };
        match next {
            Some(next) if next != *now => changes.push((*id, next)),
            Some(_) => {}
            None => skipped += 1,
        }
    }
    if !changes.is_empty() || skipped > 0 {
        out.push(Sent::Engine(Command::Study(Action::RestyleAll(
            changes, skipped,
        ))));
    }
}

/// The pens as one segmented row, then the colours.
fn picker(ui: &mut Ui, now: &Current, look: &HighlightSettings, t: &Tokens) -> Option<Pick> {
    let pen = pens(ui, now, look, t).map(Pick::Pen);
    ui.add_space(8.0);
    let color = swatches(ui, now, look, t).map(Pick::Color);
    ui.add_space(2.0);
    pen.or(color)
}

/// A pen's short name, to fit three in the row.
fn short(pen: Pen) -> &'static str {
    match pen {
        Pen::Full => "Highlight",
        Pen::Half => "Half",
        Pen::Underline => "Underline",
    }
}

/// The three pens, each with a strip of the colour it would show.
fn pens(ui: &mut Ui, now: &Current, look: &HighlightSettings, t: &Tokens) -> Option<Pen> {
    let (track, _) = ui.allocate_exact_size(vec2(ui.available_width(), PEN_ROW), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect_filled(track, t.radius, t.line.gamma_multiply(0.35));
    let inner = track.shrink(2.0);
    let w = inner.width() / 3.0;
    let mut picked = None;
    for (i, pen) in Pen::ALL.into_iter().enumerate() {
        let min = inner.min + vec2(w * i as f32, 0.0);
        let cell = Rect::from_min_size(min, vec2(w, inner.height()));
        let resp = ui.interact(cell, ui.id().with(("restyle-pen", i)), Sense::click());
        let on = now.pen() == Some(pen);
        if on {
            painter.rect_filled(cell, t.radius - 2.0, t.surface);
            painter.rect_stroke(
                cell,
                t.radius - 2.0,
                Stroke::new(1.0, t.line),
                egui::StrokeKind::Inside,
            );
        } else if resp.hovered() {
            painter.rect_filled(cell, t.radius - 2.0, t.hover);
        }
        let ink = if on || resp.hovered() { t.ink } else { t.muted };
        let icon = Rect::from_center_size(cell.center_top() + vec2(0.0, 14.0), vec2(16.0, 16.0));
        pen_icon(pen).paint(ui, icon, ink);
        let strip = Rect::from_center_size(icon.center_bottom() + vec2(0.0, 3.0), vec2(14.0, 2.5));
        let would = shown(with_pen(now.first, pen, look), look);
        painter.rect_filled(strip, CornerRadius::same(1), color(would));
        let font = egui::FontId::proportional(t.text_small);
        let name = cell.center_bottom() - vec2(0.0, 10.0);
        painter.text(name, egui::Align2::CENTER_CENTER, short(pen), font, ink);
        resp.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, true, on, pen.label()));
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.clicked() {
            picked = Some(pen);
        }
    }
    picked
}

/// The colours on offer to every pen in use, each shown once.
fn on_offer(now: &Current, look: &HighlightSettings) -> Vec<Rgb> {
    let mut out: Vec<Rgb> = Vec::new();
    let pens = now.pens.iter();
    let all = pens.flat_map(|p| offered(*p, now.first_shown(*p, look), look));
    for c in all.chain(now.colors.iter().copied()) {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

impl Current {
    /// The colour shown by the first look with `pen`.
    fn first_shown(&self, pen: Pen, look: &HighlightSettings) -> Rgb {
        let same = self.first.pen == pen;
        let c = same.then(|| shown(self.first, look));
        c.unwrap_or(look.color(pen))
    }
}

/// The colours; a reserved one is struck through and says why.
fn swatches(ui: &mut Ui, now: &Current, look: &HighlightSettings, t: &Tokens) -> Option<Rgb> {
    let shown_now = now.color();
    let own = now.pen().map(|p| look.color(p));
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for c in on_offer(now, look) {
            let kept = now.kept(c, look);
            let sense = if kept.is_some() {
                Sense::hover()
            } else {
                Sense::click()
            };
            let (rect, resp) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), sense);
            let on = shown_now == Some(c);
            paint_swatch(ui, rect, c, on, kept.is_some(), resp.hovered(), t);
            let hex = c.to_hex();
            let name = if own == Some(c) {
                format!("{hex}, the highlighter's own")
            } else {
                hex
            };
            let enabled = kept.is_none();
            resp.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, enabled, on, &name));
            widgets::tip(&resp, kept.unwrap_or(&name), None);
            if kept.is_none() {
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    picked = Some(c);
                }
            }
        }
    });
    picked
}

/// A round swatch: ringed when shown, struck through when reserved.
fn paint_swatch(ui: &Ui, rect: Rect, c: Rgb, on: bool, kept: bool, hovered: bool, t: &Tokens) {
    let painter = ui.painter();
    let centre = rect.center();
    let r = SWATCH * 0.5 - 3.0;
    let fill = if kept {
        color(c).gamma_multiply(0.3)
    } else {
        color(c)
    };
    painter.circle_filled(centre, r, fill);
    painter.circle_stroke(centre, r, Stroke::new(1.0, t.line));
    if on {
        painter.circle_stroke(centre, r + 2.5, Stroke::new(1.5, t.ink));
    } else if hovered && !kept {
        painter.circle_stroke(centre, r + 2.5, Stroke::new(1.0, t.muted));
    }
    if kept {
        let d = vec2(r, -r) * std::f32::consts::FRAC_1_SQRT_2;
        painter.line_segment([centre - d, centre + d], Stroke::new(1.5, t.muted));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIOLET: Rgb = Rgb(0xa5, 0x8a, 0xf0);
    const ORANGE: Rgb = Rgb(0xff, 0xbe, 0x00);

    /// The AI keeps a full violet highlight.
    fn look() -> HighlightSettings {
        let mut look = HighlightSettings {
            ai: Reserved {
                pen: Pen::Full,
                color: VIOLET,
            },
            ..HighlightSettings::default()
        };
        look.sanitize();
        look
    }

    #[test]
    fn a_new_pen_keeps_the_colour_unless_it_is_kept_for_the_ai() {
        let look = look();
        let orange = Style {
            pen: Pen::Half,
            color: Some([ORANGE.0, ORANGE.1, ORANGE.2]),
        };
        let full = with_pen(orange, Pen::Full, &look);
        assert_eq!(shown(full, &look), ORANGE);
        let violet = Style {
            pen: Pen::Half,
            color: Some([VIOLET.0, VIOLET.1, VIOLET.2]),
        };
        let full = with_pen(violet, Pen::Full, &look);
        assert_eq!(full.color, None, "the pen's own colour instead");
        assert!(look.allowed(Pen::Full, shown(full, &look)));
    }

    #[test]
    fn a_reserved_colour_is_offered_but_never_taken() {
        let look = look();
        let full = Style {
            pen: Pen::Full,
            color: None,
        };
        assert!(offered(Pen::Full, shown(full, &look), &look).contains(&VIOLET));
        assert_eq!(
            kept_for(Pen::Full, VIOLET, &look),
            Some("Kept for AI marks")
        );
        assert_eq!(with_color(full, VIOLET, &look), None);
        let cefr = (look.cefr.pen, look.cefr.color);
        assert_eq!(
            kept_for(cefr.0, cefr.1, &look),
            Some("Kept for CEFR shading")
        );
    }

    #[test]
    fn a_group_strikes_a_colour_only_when_every_pen_keeps_it() {
        let look = look();
        let full = Style {
            pen: Pen::Full,
            color: None,
        };
        let under = Style {
            pen: Pen::Underline,
            color: None,
        };
        let mixed = Current::of(&[full, under], &look).unwrap();
        assert_eq!((mixed.pen(), mixed.color()), (None, None));
        assert_eq!(mixed.kept(VIOLET, &look), None, "the underline may");
        let both = Current::of(&[full, full], &look).unwrap();
        assert_eq!(both.kept(VIOLET, &look), Some("Kept for AI marks"));
        assert!(on_offer(&mixed, &look).contains(&look.underline));
    }

    #[test]
    fn the_pen_s_own_colour_is_kept_as_none_to_follow_settings() {
        let look = look();
        let red = Style {
            pen: Pen::Full,
            color: Some([0xff, 0, 0]),
        };
        let own = with_color(red, look.full, &look);
        assert_eq!(own.map(|s| s.color), Some(None));
    }
}
