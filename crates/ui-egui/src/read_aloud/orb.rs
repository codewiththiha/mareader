//! Compact floating orb mode for the read-aloud player while obstructed.

use egui::{Context, Id, Rect};
use mareader_engine::read_aloud::player::{Lyrics, View};

use crate::theme::Tokens;
use crate::z_order::{self, Tier};

use super::{card, control};

/// Picks the least-obstructed bottom corner for the compact read-aloud orb.
pub(super) fn orb_rect(room: Rect, obstacle: Rect, toast: Option<Rect>) -> Rect {
    let size = egui::vec2(
        room.width().clamp(0.0, 160.0),
        room.height().clamp(0.0, 44.0),
    );
    let left = (room.left() + 12.0).min((room.right() - size.x).max(room.left()));
    let right = (room.right() - size.x - 12.0).max(room.left());
    let y = (room.bottom() - size.y - 12.0).max(room.top());
    let first = if obstacle.center().x > room.center().x {
        left
    } else {
        right
    };
    let second = if first == left { right } else { left };
    let candidates = [
        Rect::from_min_size(egui::pos2(first, y), size),
        Rect::from_min_size(egui::pos2(second, y), size),
    ];
    candidates
        .into_iter()
        .min_by(|a, b| {
            let overlap = |r: Rect| {
                let area = |other| {
                    let hit = r.intersect(other);
                    hit.width().max(0.0) * hit.height().max(0.0)
                };
                area(obstacle) + toast.map_or(0.0, area)
            };
            overlap(*a).total_cmp(&overlap(*b))
        })
        .unwrap_or(Rect::from_min_size(egui::pos2(first, y), size))
}

/// Picks the widest phrase slice in `lyrics` that passes `fits`.
pub(super) fn fit_orb_text(lyrics: &Lyrics, mut fits: impl FnMut(&str) -> bool) -> &str {
    let get = |range: &std::ops::Range<usize>| {
        let first = lyrics.words.get(range.start)?;
        let last = lyrics.words.get(range.end.checked_sub(1)?)?;
        if range.start >= range.end {
            return None;
        }
        lyrics.now.get(first.start..last.end)
    };
    let word = lyrics.heard..lyrics.heard.saturating_add(1);
    for range in [&lyrics.preferred, &lyrics.lit, &word] {
        if let Some(text) = get(range)
            && fits(text)
        {
            return text;
        }
    }
    get(&word).unwrap_or(lyrics.now.as_str())
}

/// Paints the compact orb player; returns true when play/pause is clicked.
pub(super) fn show_orb(
    ctx: &Context,
    rect: Rect,
    view: &View<'_>,
    lyrics: Option<&Lyrics>,
) -> bool {
    z_order::area(ctx, Tier::FloatingTool, Id::new("read-aloud-orb"))
        .fixed_pos(rect.min)
        .constrain(false)
        .movable(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect.expand(12.0));
            card::frame(ui, rect, 0.0);
            ui.set_clip_rect(rect);
            let slot = Rect::from_min_size(
                rect.min + egui::vec2(4.0, 0.0),
                egui::vec2(40.0, rect.height()),
            );
            let t = Tokens::get(ctx);
            if let Some(progress) = view.at.filter(|p| p.is_finite()) {
                let count = (progress.clamp(0.0, 1.0) * 48.0) as usize;
                let points: Vec<_> = (0..=count)
                    .map(|i| {
                        let a =
                            (i as f32 / 48.0) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                        slot.center() + egui::vec2(a.cos(), a.sin()) * 20.0
                    })
                    .collect();
                if points.len() > 1 {
                    ui.painter()
                        .add(egui::Shape::line(points, egui::Stroke::new(2.0, t.accent)));
                }
            }
            let available = (rect.width() - 58.0).max(0.0);
            let font = egui::FontId::proportional(12.0);
            let text = lyrics
                .map(|lyrics| {
                    fit_orb_text(lyrics, |text| {
                        ui.painter()
                            .layout_no_wrap(text.to_owned(), font.clone(), t.muted)
                            .size()
                            .x
                            <= available
                    })
                })
                .filter(|text| !text.is_empty())
                .unwrap_or("Read aloud");
            let text_area = Rect::from_min_max(
                egui::pos2((rect.left() + 50.0).min(rect.right()), rect.top()),
                rect.max,
            );
            ui.painter().with_clip_rect(text_area.intersect(rect)).text(
                egui::pos2(text_area.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                text,
                font,
                t.muted,
            );
            let label = ui.interact(
                text_area.intersect(rect),
                Id::new("read-aloud-orb-words"),
                egui::Sense::hover(),
            );
            label.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
            control::play_button(ui, slot, view)
        })
        .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orb_text_uses_the_largest_fitting_group_and_tracks_the_heard_word() {
        let mut lyrics = Lyrics {
            before: None,
            now: "one two three four".to_owned(),
            words: vec![0..3, 4..7, 8..13, 14..18],
            lit: 1..4,
            preferred: 0..4,
            heard: 2,
            after: None,
            key: 1,
        };
        assert_eq!(
            fit_orb_text(&lyrics, |s| s.len() <= 20),
            "one two three four"
        );
        assert_eq!(fit_orb_text(&lyrics, |s| s.len() <= 14), "two three four");
        assert_eq!(fit_orb_text(&lyrics, |s| s.len() <= 5), "three");
        lyrics.heard = 3;
        assert_eq!(fit_orb_text(&lyrics, |s| s.len() <= 5), "four");
        assert_eq!(fit_orb_text(&lyrics, |_| false), "four");
        lyrics.heard = usize::MAX;
        lyrics.preferred = lyrics.words.len()..1;
        lyrics.lit = 9..10;
        assert_eq!(fit_orb_text(&lyrics, |_| true), "one two three four");
    }

    #[test]
    fn orb_rect_stays_along_the_bottom_edge_under_wide_obstacles() {
        let room = Rect::from_min_size(egui::pos2(0.0, 40.0), egui::vec2(900.0, 760.0));
        // Even a full-width obstacle near the bottom keeps the orb edged below.
        let wide = Rect::from_min_size(egui::pos2(0.0, 620.0), egui::vec2(900.0, 160.0));
        let orb = orb_rect(room, wide, None);
        let expected_y = room.bottom() - orb.height() - 12.0;
        assert!((orb.top() - expected_y).abs() < 0.5, "orb: {orb:?}");
        assert!(orb.top() > wide.top(), "orb must not jump above: {orb:?}");
    }
}
