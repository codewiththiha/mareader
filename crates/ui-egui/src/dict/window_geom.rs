//! Geometry, fold morphing, and obstacle avoidance for the Dictionary window.

use egui::{Context, Id, Rect, Vec2, pos2, vec2};
use mareader_settings::WINDOW_MIN;

/// Its size on first opening.
pub(super) const START: Vec2 = vec2(380.0, 520.0);
pub(super) const HEAD: f32 = 38.0;
/// The band along each edge that resizes.
pub(super) const GRIP: f32 = 6.0;
pub(super) const PILL: Vec2 = vec2(240.0, HEAD);
const SHORT_MIN: f32 = HEAD + 24.0;
/// Room kept between the window and the stage's edges.
const INSET: f32 = 12.0;
const FOLD: f32 = 0.24;

/// The window turning into its pill: both ends, and `fold` from 0 to 1.
pub(super) struct Morph {
    pub open: Rect,
    pub pill: Rect,
    pub fold: f32,
}

impl Morph {
    pub(super) fn new(ctx: &Context, rect: Rect, folded: bool, stage: Rect, short: bool) -> Self {
        let secs = crate::motion::secs(ctx, FOLD);
        let id = Id::new("dict-window-fold");
        let fold = ctx.animate_bool_with_time(id, folded, secs);
        // Each end is kept as last seen, so the turn starts where it was.
        let kept = |which: &str| ctx.data(|d| d.get_temp::<Rect>(id.with(which)));
        let (open, pill) = match folded {
            true => {
                let pill = Rect::from_min_size(rect.min, PILL);
                (kept("open").unwrap_or_else(|| from_pill(rect, stage)), pill)
            }
            false => {
                let pill = Rect::from_min_size(to_pill(rect, stage).min, PILL);
                (rect, kept("pill").unwrap_or(pill))
            }
        };
        let open_size = if short {
            rect.size()
        } else {
            rect.size().max(Vec2::from(WINDOW_MIN))
        };
        let open = Rect::from_min_size(open.min, open_size);
        ctx.data_mut(|d| {
            if fold <= 0.0 {
                d.insert_temp(id.with("open"), open);
            }
            if fold >= 1.0 {
                d.insert_temp(id.with("pill"), pill);
            }
        });
        Self { open, pill, fold }
    }

    pub(super) fn shown(&self) -> Rect {
        let e = ease(self.fold);
        Rect::from_min_max(
            self.open.min.lerp(self.pill.min, e),
            self.open.max.lerp(self.pill.max, e),
        )
    }
}

/// Fast out, soft landing.
pub(super) fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

/// `r` with the named sides moved by `d`, kept at least the minimum.
pub(super) fn stretch(mut r: Rect, dx: i8, dy: i8, d: Vec2) -> Rect {
    let [min_w, min_h] = WINDOW_MIN;
    match dx {
        -1 => r.min.x = (r.min.x + d.x).min(r.max.x - min_w),
        1 => r.max.x = (r.max.x + d.x).max(r.min.x + min_w),
        _ => {}
    }
    match dy {
        -1 => r.min.y = (r.min.y + d.y).min(r.max.y - min_h),
        1 => r.max.y = (r.max.y + d.y).max(r.min.y + min_h),
        _ => {}
    }
    r
}

/// First opened: at the stage's top right, beside the page.
pub(super) fn start(stage: Rect) -> Rect {
    let min = pos2(stage.right() - START.x - INSET, stage.top() + INSET);
    Rect::from_min_size(min, START)
}

/// Folded, the rect starts at the pill: top right on the right half.
pub fn to_pill(r: Rect, stage: Rect) -> Rect {
    let right = r.center().x > stage.center().x;
    let x = if right { r.right() - PILL.x } else { r.left() };
    Rect::from_min_size(pos2(x, r.top()), r.size())
}

/// Unfolded, it grows from the pill away from the nearer edges.
pub fn from_pill(r: Rect, stage: Rect) -> Rect {
    let pill = Rect::from_min_size(r.min, PILL);
    let x = match pill.center().x > stage.center().x {
        true => pill.right() - r.width(),
        false => pill.left(),
    };
    let y = match pill.center().y > stage.center().y {
        true => pill.bottom() - r.height(),
        false => pill.top(),
    };
    fit(Rect::from_min_size(pos2(x, y), r.size()), stage)
}

/// The pill wholly on the stage, by its own size; the window's size kept.
pub fn keep_pill(r: Rect, stage: Rect) -> Rect {
    let room = stage.shrink(INSET);
    let far = (room.max - PILL).max(room.min);
    let min = r.min.clamp(room.min, far);
    Rect::from_min_size(min, r.size())
}

/// `r` no larger than the stage and wholly on it.
pub fn fit(r: Rect, stage: Rect) -> Rect {
    fit_min(r, stage, Vec2::from(WINDOW_MIN))
}

fn fit_min(r: Rect, stage: Rect, minimum: Vec2) -> Rect {
    let room = stage.shrink(INSET);
    let room_size = room.size().max(Vec2::ZERO);
    let floor = minimum.min(room_size);
    let size = r.size().max(floor).min(room_size.max(floor));
    let far_x = (room.max.x - size.x).max(room.min.x);
    let far_y = (room.max.y - size.y).max(room.min.y);
    let x = r.min.x.clamp(room.min.x, far_x);
    let y = r.min.y.clamp(room.min.y, far_y);
    Rect::from_min_size(pos2(x, y), size)
}

/// Like `fit`, but a dragged window may hang beyond either side.
pub(super) fn hang(r: Rect, stage: Rect, folded: bool) -> Rect {
    let room = stage.shrink(INSET);
    let height = if folded { PILL.y } else { r.height() };
    let far_y = (room.bottom() - height).max(room.top());
    Rect::from_min_size(pos2(r.left(), r.top().clamp(room.top(), far_y)), r.size())
}

/// The blocks in `obstacles` touching `r`, united into one area.
pub(super) fn covering(r: Rect, obstacles: [Option<Rect>; 2]) -> Option<Rect> {
    let mut mass = None;
    for block in obstacles.into_iter().flatten() {
        if block.intersects(r) {
            mass = Some(mass.map_or(block, |m: Rect| m.union(block)));
        }
    }
    mass
}

/// Steps beside a toast or above the player, shrinking only as a last resort.
pub(super) fn aside(home: Rect, block: Rect, stage: Rect, prefer_up: bool) -> Rect {
    let room = stage.shrink(INSET);
    let clear = |r: &Rect| room.contains_rect(*r) && !r.intersects(block);
    let dy = (home.bottom() - (block.top() - 8.0)).max(0.0);
    let up = fit(home.translate(vec2(0.0, -dy)), stage);
    let dx = (home.right() - (block.left() - 8.0)).max(0.0);
    let left = fit(home.translate(vec2(-dx, 0.0)), stage);
    let first = if prefer_up { up } else { left };
    let second = if prefer_up { left } else { up };
    if clear(&first) {
        return first;
    }
    if clear(&second) {
        return second;
    }
    let min_w = WINDOW_MIN[0];
    let narrow = (block.left() - 8.0 - home.left() >= min_w).then(|| {
        fit(
            Rect::from_min_max(home.min, pos2(block.left() - 8.0, home.bottom())),
            stage,
        )
    });
    let height = (block.top() - 8.0 - home.top()).min(home.height());
    let short = (height >= SHORT_MIN).then(|| {
        fit_min(
            Rect::from_min_size(home.min, vec2(home.width(), height)),
            stage,
            vec2(min_w, SHORT_MIN),
        )
    });
    [narrow, short]
        .into_iter()
        .flatten()
        .filter(|r| !r.intersects(block))
        .max_by(|a, b| (a.width() * a.height()).total_cmp(&(b.width() * b.height())))
        .unwrap_or(up)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_stays_on_the_stage_and_never_below_its_minimum() {
        let stage = Rect::from_min_size(pos2(0.0, 40.0), vec2(1000.0, 700.0));
        let off = Rect::from_min_size(pos2(900.0, -50.0), vec2(500.0, 100.0));
        let r = fit(off, stage);
        assert!(stage.contains_rect(r), "{r:?}");
        assert_eq!(r.height(), WINDOW_MIN[1]);
        let shrunk = stretch(r, -1, 1, vec2(400.0, -400.0));
        assert_eq!(shrunk.size(), Vec2::from(WINDOW_MIN));
        assert_eq!(shrunk.max.x, r.max.x, "the right edge held");
    }

    #[test]
    fn the_dictionary_shortens_when_the_player_leaves_no_full_height() {
        let stage = Rect::from_min_size(pos2(0.0, 40.0), vec2(900.0, 700.0));
        let home = Rect::from_min_size(pos2(150.0, 200.0), vec2(600.0, 400.0));
        let player = Rect::from_min_size(pos2(260.0, 416.0), vec2(380.0, 224.0));
        let short = aside(home, player, stage, true);
        assert!(short.height() < WINDOW_MIN[1], "{short:?}");
        assert_eq!(short.bottom(), player.top() - 8.0);
        assert!(!short.intersects(player));
    }

    #[test]
    fn the_pill_reaches_every_edge_and_unfolds_onto_the_stage() {
        let stage = Rect::from_min_size(pos2(0.0, 40.0), vec2(1000.0, 700.0));
        let room = stage.shrink(INSET);
        let window = Rect::from_min_size(pos2(600.0, 60.0), START);
        // Folding on the right half keeps the pill under the − button.
        let folded = to_pill(window, stage);
        assert_eq!(folded.min.x + PILL.x, window.right());
        // Dragged into the bottom right corner, the pill gets there.
        let far = keep_pill(folded.translate(vec2(2000.0, 2000.0)), stage);
        assert_eq!(far.min + PILL, room.max);
        // And grows up and to the left from there, wholly on the stage.
        let open = from_pill(far, stage);
        assert!(stage.contains_rect(open), "{open:?}");
        assert_eq!(open.size(), START);
        assert_eq!(open.max, room.max);
    }
}
