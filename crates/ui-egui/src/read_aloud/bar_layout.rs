//! Placement, toast dodging, keeper rise, and edge parking for the player bar.

use std::time::Duration;

use egui::{Context, Id, Rect};
use mareader_settings::ReadAloudSettings;

use super::bar::{Motion, Placement};
use super::control::Fit;
use super::orb::orb_rect;
use super::{Shape, Stage, card, control, ease_in_out};

/// Computes the bar rect after toast dodging, keeper rise, and edge parking.
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
    // Something summoned sits beneath it: it glides above, and back.
    let under = crate::toast::kept_clear(ctx)
        .into_iter()
        .filter(|foe| foe.intersects(rect))
        .map(|foe| rect.bottom() - foe.top() + 8.0)
        .fold(0.0_f32, f32::max);
    let risen = ctx.animate_value_with_time(Id::new("read-aloud-keeper-rise"), under, m.morph);
    rect = rect.translate(egui::vec2(0.0, -risen));
    let (rect, orb_k, parked, tucked_rest) =
        resolve_obstruction(ctx, shape, rect, (m, stage, now, under));
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

/// Applies dictionary avoidance: orb morph, rise above, or bottom-edge tuck.
fn resolve_obstruction(
    ctx: &Context,
    shape: &mut Shape,
    mut rect: Rect,
    (m, stage, now, under): (&Motion, &Stage<'_>, f64, f32),
) -> (Rect, f32, bool, bool) {
    let asked_up = shape.player_priority || shape.unparked_until.is_some_and(|until| now < until);
    if stage.dict.is_some()
        && let Some(until) = shape.unparked_until.filter(|until| now < *until)
    {
        ctx.request_repaint_after(Duration::from_secs_f64(until - now));
    }
    let covered = stage.dict.is_some_and(|dict| rect.intersects(dict));
    let obstructed = covered && !asked_up && under <= 0.0;
    let orb = obstructed && stage.parking == mareader_settings::EdgeParking::Tab;
    let parking = obstructed && stage.parking == mareader_settings::EdgeParking::Sliver;
    let orb_k = ctx.animate_bool_with_time(Id::new("read-aloud-orb-move"), orb, m.morph);
    if orb && let Some(dict) = stage.dict {
        shape.orb_target = Some(orb_rect(m.room, dict, stage.toasts));
    }
    if !orb && orb_k <= 0.0 {
        shape.orb_target = None;
    }
    if obstructed
        && stage.parking == mareader_settings::EdgeParking::Off
        && let Some(dict) = stage.dict
    {
        let above = dict.top() - 8.0 - rect.height();
        rect = rect.translate(egui::vec2(
            0.0,
            (above - rect.top()).max(m.room.top() - rect.top()),
        ));
    }
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
