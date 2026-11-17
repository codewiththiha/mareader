//! Placement, toast dodging, and bottom-edge parking for the player bar.

use std::time::Duration;

use egui::{Context, Id, Rect};
use mareader_settings::ReadAloudSettings;

use super::bar::{Motion, Placement};
use super::control::Fit;
use super::orb::orb_rect;
use super::{Shape, Stage, card, control, ease_in_out};

/// Computes the bar rect after toast dodging and bottom-only parking.
pub(super) fn place_bar(
    ctx: &Context,
    shape: &mut Shape,
    m: &Motion,
    stage: &Stage<'_>,
    settings: &ReadAloudSettings,
    (now, escape): (f64, bool),
) -> Placement {
    let (narrowest, widest) = ReadAloudSettings::PLAYER_WIDTH;
    let parts = narrowest.max(Fit::new(0.0, escape).width(escape));
    let set = shape.dragged.or(settings.player_width);
    let pill = set.map_or(parts, |w| w.clamp(narrowest, widest));
    let open = card::lerp(pill, card::FULL_LEAST.max(pill), m.f);
    let most = (m.room.width() - 16.0).max(0.0);
    let mut width = card::lerp(pill, open, m.k).min(most);
    let tall = card::lerp(card::COMPACT_HEIGHT, card::HEIGHT, m.f);
    let height = card::lerp(control::HEIGHT, tall, m.k);
    let bottom = m.at.y + (1.0 - m.shown) * 10.0;
    let mut rect = Rect::from_min_size(
        egui::pos2(m.at.x - width / 2.0, bottom - height),
        egui::vec2(width, height),
    );
    // Idling across an active toast bed: it narrows, easing both ways.
    let toasts_in_way = stage
        .toasts
        .filter(|t| shape.dragged.is_none() && rect.intersects(*t));
    if let Some(toasts) = toasts_in_way {
        let open_parts = card::lerp(parts, card::FULL_LEAST.max(parts), m.f);
        let min_width = card::lerp(parts, open_parts, m.k).min(most);
        let clear = ((toasts.left() - 8.0 - m.at.x) * 2.0).max(narrowest);
        shape.dodge_toast = Some(min_width.min(clear));
    }
    let dodging = toasts_in_way.is_some();
    let dodge_k = ctx.animate_bool_with_time(Id::new("read-aloud-toast-dodge"), dodging, m.morph);
    if let Some(dodged) = shape.dodge_toast {
        width = card::lerp(width, dodged, ease_in_out(dodge_k));
        rect = Rect::from_min_size(
            egui::pos2(m.at.x - width / 2.0, bottom - height),
            egui::vec2(width, height),
        );
    }
    if !dodging && dodge_k <= 0.0 {
        shape.dodge_toast = None;
    }
    let (rect, orb_k, parked, tucked_rest) = resolve_obstruction(ctx, shape, rect, (m, stage, now));
    shape.rect = Some(rect);
    shape.rise = if parked {
        0.0
    } else {
        m.pane.map_or(0.0, |p| (p.bottom() - rect.top()).max(0.0))
    };
    let least = if m.f >= 0.5 {
        card::FULL_LEAST
    } else {
        narrowest
    };
    Placement {
        rect,
        width,
        span: (width, least.min(most), widest.min(most)),
        orb_k,
        parked,
        tucked_rest,
    }
}

/// Summoned popups overlapping the player, excluding its own skip pill.
fn keeper_obstacle(ctx: &Context, rect: Rect) -> Option<Rect> {
    let own_ask = ctx.memory(|m| m.area_rect(Id::new("read-aloud-ask")));
    crate::toast::kept_clear(ctx)
        .into_iter()
        .filter(|foe| own_ask != Some(*foe) && foe.intersects(rect))
        .reduce(Rect::union)
}

/// Downward-only offset to sit under `foe` within `room`, never moving up.
fn under_drop(rect: Rect, foe: Rect, room: Rect) -> f32 {
    let floor = (room.bottom() - rect.height() - 4.0).max(rect.top());
    let below = (foe.bottom() + 8.0).clamp(rect.top(), floor);
    (below - rect.top()).max(0.0)
}

/// Applies bottom-only avoidance: corner orb, downward drop, or tuck.
fn resolve_obstruction(
    ctx: &Context,
    shape: &mut Shape,
    mut rect: Rect,
    (m, stage, now): (&Motion, &Stage<'_>, f64),
) -> (Rect, f32, bool, bool) {
    let keeper = keeper_obstacle(ctx, rect);
    let asked_up = shape.player_priority || shape.unparked_until.is_some_and(|until| now < until);
    if stage.dict.is_some()
        && let Some(until) = shape.unparked_until.filter(|until| now < *until)
    {
        ctx.request_repaint_after(Duration::from_secs_f64(until - now));
    }
    let dict_foe = stage
        .dict
        .filter(|dict| !asked_up && rect.intersects(*dict));
    let obstacle = keeper.or(dict_foe);
    let obstructed = obstacle.is_some();
    let orb = obstructed && stage.parking == mareader_settings::EdgeParking::Tab;
    let parking = obstructed && stage.parking == mareader_settings::EdgeParking::Sliver;
    let orb_k = ctx.animate_bool_with_time(Id::new("read-aloud-orb-move"), orb, m.morph);
    if orb && let Some(foe) = obstacle {
        shape.orb_target = Some(orb_rect(m.room, foe, stage.toasts));
    }
    if !orb && orb_k <= 0.0 {
        shape.orb_target = None;
    }
    let drop_target = if stage.parking == mareader_settings::EdgeParking::Off
        && let Some(foe) = obstacle
    {
        under_drop(rect, foe, m.room)
    } else {
        0.0
    };
    let dropped =
        ctx.animate_value_with_time(Id::new("read-aloud-under-drop"), drop_target, m.morph);
    rect = rect.translate(egui::vec2(0.0, dropped.max(0.0)));
    let park_stage = m.room;
    if shape.parked.is_none() && parking {
        shape.parked = Some(crate::park::Tuck::new(
            crate::park::Edge::Bottom,
            rect,
            rect,
            park_stage,
        ));
    }
    let tucked = crate::park::factor(
        ctx,
        Id::new("read-aloud-park"),
        parking,
        shape.parked.is_some(),
    );
    if let Some(mut tuck) = shape.parked {
        if parking {
            tuck.revealing = false;
        }
        rect = tuck.rect(tucked);
        if !parking && tucked <= 0.0 {
            rect = tuck.resume;
            shape.parked = None;
        } else {
            shape.parked = Some(tuck);
        }
    }
    let active = parking && tucked >= 1.0 && shape.parked.is_some_and(|tuck| !tuck.revealing);
    let lift = crate::park::magnetic_offset(
        ctx,
        Id::new("read-aloud-park-magnet"),
        shape.parked,
        rect,
        park_stage,
        active,
    );
    rect = rect.translate(lift);
    let parked = shape.parked.is_some();
    let tucked_rest = parked && parking && tucked >= 1.0;
    (rect, orb_k, parked, tucked_rest)
}

#[cfg(test)]
mod tests {
    use egui::{RawInput, pos2, vec2};
    use mareader_settings::EdgeParking;

    use super::*;
    use crate::theme::Tokens;

    fn motion(room: Rect) -> Motion {
        Motion {
            pane: Some(room),
            room,
            k: 0.0,
            f: 0.0,
            shown: 1.0,
            at: pos2(room.center().x, room.bottom() - 48.0),
            morph: 0.0,
            colours: Tokens::new(&Default::default()),
        }
    }

    fn stage(room: Rect, dict: Option<Rect>, parking: EdgeParking) -> Stage<'static> {
        Stage {
            reader: true,
            panes: &[],
            content: room,
            shown: Some(true),
            title: "Book",
            book: None,
            tint: None,
            toasts: None,
            dict,
            parking,
        }
    }

    fn step_place(
        ctx: &Context,
        time: f64,
        bubble: Option<Rect>,
        shape: &mut Shape,
        stage: &Stage<'_>,
        m: &Motion,
    ) -> Placement {
        let mut out = None;
        let input = RawInput {
            time: Some(time),
            screen_rect: Some(m.room),
            ..RawInput::default()
        };
        let mut full = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx();
            if let Some(rect) = bubble {
                crate::toast::keep_clear(ctx, rect);
            }
            out = Some(place_bar(
                ctx,
                shape,
                m,
                stage,
                &ReadAloudSettings::default(),
                (time, false),
            ));
        });
        full.textures_delta.clear();
        out.unwrap_or(Placement {
            rect: Rect::NOTHING,
            width: 0.0,
            span: (0.0, 0.0, 0.0),
            orb_k: 0.0,
            parked: false,
            tucked_rest: false,
        })
    }

    #[test]
    fn bubble_overlap_tucks_below_even_with_player_priority() {
        let ctx = Context::default();
        let room = Rect::from_min_size(pos2(0.0, 40.0), vec2(900.0, 760.0));
        let m = motion(room);
        let mut shape = Shape {
            player_priority: true,
            ..Shape::default()
        };
        let st = stage(room, None, EdgeParking::Sliver);
        let home = step_place(&ctx, 1.0, None, &mut shape, &st, &m);
        let bubble = Rect::from_center_size(
            pos2(home.rect.center().x, home.rect.top() - 40.0),
            vec2(300.0, 140.0),
        );
        assert!(bubble.intersects(home.rect));
        let _ = step_place(&ctx, 2.0, Some(bubble), &mut shape, &st, &m);
        let tucked = step_place(&ctx, 2.5, Some(bubble), &mut shape, &st, &m);
        assert!(tucked.parked, "bubble must tuck the bar to the bottom edge");
        assert!(
            tucked.rect.top() > home.rect.top(),
            "bar must move downward, never above: {:?} vs {:?}",
            tucked.rect,
            home.rect
        );
        assert!(
            (room.bottom() - tucked.rect.top() - crate::park::PEEK).abs() < 1.0,
            "bar should peek from the bottom edge: {:?}",
            tucked.rect
        );
        // Once the bubble closes and passes expire, the bar returns home.
        let _ = step_place(&ctx, 3.0, None, &mut shape, &st, &m);
        let _ = step_place(&ctx, 3.1, None, &mut shape, &st, &m);
        let back = step_place(&ctx, 3.6, None, &mut shape, &st, &m);
        assert!(!back.parked, "bar should unpark after the bubble closes");
        assert!(
            (back.rect.top() - home.rect.top()).abs() < 1.0,
            "bar should return to home: {:?} vs {:?}",
            back.rect,
            home.rect
        );
    }

    #[test]
    fn parking_off_drops_under_and_never_rises_above_home() {
        let ctx = Context::default();
        let room = Rect::from_min_size(pos2(0.0, 40.0), vec2(900.0, 760.0));
        let m = motion(room);
        let mut shape = Shape::default();
        let base = stage(room, None, EdgeParking::Off);
        let home = step_place(&ctx, 1.0, None, &mut shape, &base, &m);
        let dict = Rect::from_min_size(
            pos2(home.rect.left() - 20.0, home.rect.top() - 180.0),
            vec2(440.0, 210.0),
        );
        let with_dict = stage(room, Some(dict), EdgeParking::Off);
        let dropped = step_place(&ctx, 2.0, None, &mut shape, &with_dict, &m);
        assert!(
            dropped.rect.top() >= home.rect.top(),
            "bar must never rise above home: {:?} vs {:?}",
            dropped.rect,
            home.rect
        );
        assert!(
            dropped.rect.bottom() <= room.bottom(),
            "bar stays inside room: {:?}",
            dropped.rect
        );
        let tall_bubble = Rect::from_min_size(
            pos2(home.rect.left(), home.rect.top() - 200.0),
            vec2(360.0, 260.0),
        );
        let under = step_place(&ctx, 3.0, Some(tall_bubble), &mut shape, &base, &m);
        assert!(
            under.rect.top() >= home.rect.top(),
            "tall bubble must never push bar upward: {:?} vs {:?}",
            under.rect,
            home.rect
        );
    }

    #[test]
    fn under_drop_is_strictly_non_negative() {
        let room = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let bar = Rect::from_min_size(pos2(200.0, 500.0), vec2(320.0, 44.0));
        let high_foe = Rect::from_min_size(pos2(200.0, 300.0), vec2(300.0, 150.0));
        assert_eq!(under_drop(bar, high_foe, room), 0.0);
        let low_foe = Rect::from_min_size(pos2(200.0, 420.0), vec2(300.0, 100.0));
        let dy = under_drop(bar, low_foe, room);
        assert!(dy > 0.0);
        assert!(bar.bottom() + dy <= room.bottom() - 4.0 + 0.01);
    }
}
