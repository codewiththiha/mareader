//! A card anchored to words on the page, its caret pointing at them.

use egui::{Align2, Context, Frame, Id, Margin, Order, Rect, Shape, Stroke, Ui, pos2};

use crate::motion::entrance;
use crate::theme::Tokens;

/// From the words to the card's edge; the caret spans most of it.
pub const GAP: f32 = 11.0;
/// How far the caret reaches out of the card.
const CARET: f32 = 8.0;
/// The caret keeps this far from the card's rounded corners.
const CORNER: f32 = 16.0;
/// The card keeps this far inside its room.
const MARGIN: f32 = 8.0;
/// Unmeasured, the preferred side wins while this much is left there.
const ROOM_FIRST: f32 = 240.0;
/// How far the card rises into place as it opens.
const LIFT: f32 = 4.0;
/// Seconds the opening takes.
const ENTER: f32 = 0.16;

/// Which side of its words the card sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Below,
    Above,
}

/// Where a card points and how wide it is.
pub struct Bubble {
    /// One per opened card: a new id measures before it shows.
    pub id: Id,
    /// The words pointed at.
    pub at: Rect,
    /// Where the card must stay, as its pane.
    pub room: Rect,
    /// The content's width inside the margins; the most, when snug.
    pub width: f32,
    pub margin: Margin,
    /// As wide as the content, as a toolbar, instead of `width` exactly.
    pub snug: bool,
    /// The side tried first.
    pub prefer: Side,
}

/// Last frame's height and side, and when the card opened.
#[derive(Clone, Copy, Debug, Default)]
struct Memo {
    height: f32,
    /// Last frame's whole width, for centring a snug card.
    width: f32,
    side: Option<Side>,
    opened: f64,
    /// The frame it was last drawn in; a gap means it reopened.
    pass: u64,
    /// The words and room it was placed for.
    placed: Option<(Rect, Rect)>,
}

impl Bubble {
    /// Draws the card; `add` is told the tallest its content may grow.
    pub fn show<R>(
        self,
        ctx: &Context,
        t: &Tokens,
        add: impl FnOnce(&mut Ui, f32) -> R,
    ) -> (Rect, R) {
        let memo_id = self.id.with("memo");
        let now = ctx.input(|i| i.time);
        let pass = ctx.cumulative_pass_nr();
        let fresh = Memo {
            opened: now,
            ..Memo::default()
        };
        let kept: Option<Memo> = ctx.data(|d| d.get_temp(memo_id));
        let mut memo = kept.filter(|m| m.pass + 1 >= pass).unwrap_or(fresh);
        let (at, room) = (self.at, self.room);
        let rooms = (
            room.bottom() - at.bottom() - GAP - MARGIN,
            at.top() - room.top() - GAP - MARGIN,
        );
        let need = (memo.height > 0.0).then_some(memo.height);
        // A card keeps its side only while its words and room stay put.
        let still = |a: Rect, b: Rect| (a.min - b.min).length() + (a.max - b.max).length() < 1.0;
        let unmoved = |(a, r): (Rect, Rect)| still(a, at) && still(r, room);
        let was = memo.side.filter(|_| memo.placed.is_some_and(unmoved));
        let side = side(rooms, need, was, self.prefer);
        let space = match side {
            Side::Below => rooms.0,
            Side::Above => rooms.1,
        };
        // Unmeasured, the content may grow as tall as the roomier side.
        let space = if need.is_some() {
            space
        } else {
            rooms.0.max(rooms.1)
        };
        let sides = self.margin.sum() + egui::vec2(2.0, 2.0);
        let cap = (space - sides.y).max(0.0);
        let width = self
            .width
            .min(room.width() - 2.0 * MARGIN - sides.x)
            .max(0.0);
        let snug = (self.snug && memo.width > 0.0).then_some(memo.width);
        let half = snug.unwrap_or(width + sides.x) / 2.0;
        let lo = room.left() + MARGIN + half;
        let x = at
            .center()
            .x
            .clamp(lo, (room.right() - MARGIN - half).max(lo));
        let lift = (1.0 - entrance(ctx, memo.opened, now, ENTER)) * LIFT;
        let (pos, pivot) = match side {
            Side::Below => (pos2(x, at.bottom() + GAP + lift), Align2::CENTER_TOP),
            Side::Above => (pos2(x, at.top() - GAP - lift), Align2::CENTER_BOTTOM),
        };
        let shown = egui::Area::new(self.id)
            .order(Order::Foreground)
            .pivot(pivot)
            .fixed_pos(pos)
            .constrain(false)
            .sizing_pass(need.is_none())
            .show(ctx, |ui| {
                let frame = Frame::popup(ui.style())
                    .fill(t.surface)
                    .stroke(Stroke::new(1.0, t.line))
                    .corner_radius(t.radius_lg)
                    .inner_margin(self.margin)
                    .show(ui, |ui| {
                        if self.snug {
                            ui.set_max_width(width);
                        } else {
                            ui.set_width(width);
                        }
                        // Areas offer last frame's size; streamed text grows.
                        ui.set_max_height(cap);
                        add(ui, cap)
                    });
                caret(ui, frame.response.rect, at.center().x, side, t);
                frame.inner
            });
        // Summoned over the page: toasts give it the room.
        crate::toast::keep_clear(ctx, shown.response.rect);
        memo.height = shown.response.rect.height();
        memo.width = shown.response.rect.width();
        // A side picked unmeasured is a guess, not one to keep.
        memo.side = need.is_some().then_some(side);
        memo.pass = pass;
        memo.placed = Some((at, room));
        ctx.data_mut(|d| d.insert_temp(memo_id, memo));
        (shown.response.rect, shown.inner)
    }
}

/// Where the card fits, `prefer` first; a placed card stays while it fits.
pub fn side(
    (below, above): (f32, f32),
    need: Option<f32>,
    was: Option<Side>,
    prefer: Side,
) -> Side {
    let room = |s: Side| match s {
        Side::Below => below,
        Side::Above => above,
    };
    let other = match prefer {
        Side::Below => Side::Above,
        Side::Above => Side::Below,
    };
    let Some(need) = need else {
        let first = room(prefer) >= ROOM_FIRST || room(prefer) >= room(other);
        return if first { prefer } else { other };
    };
    let fits = |s: Side| room(s) >= need;
    match was {
        Some(s) if fits(s) => s,
        _ if fits(prefer) => prefer,
        _ if fits(other) => other,
        _ if room(prefer) >= room(other) => prefer,
        _ => other,
    }
}

/// The caret on the card's near edge, under the words' middle.
fn caret(ui: &Ui, card: Rect, x: f32, side: Side, t: &Tokens) {
    let lo = card.left() + CORNER;
    let x = x.clamp(lo, (card.right() - CORNER).max(lo));
    // Outward from the card, and the edge the caret grows from.
    let (edge, out) = match side {
        Side::Below => (card.top(), -1.0),
        Side::Above => (card.bottom(), 1.0),
    };
    let tip = pos2(x, edge + out * CARET);
    let left = pos2(x - CARET, edge);
    let right = pos2(x + CARET, edge);
    // The fill dips into the card to hide its border under the caret.
    let mut fill = vec![
        pos2(x - CARET - 1.0, edge - out),
        left,
        tip,
        right,
        pos2(x + CARET + 1.0, edge - out),
    ];
    if side == Side::Above {
        fill.reverse();
    }
    let painter = ui.painter().with_clip_rect(ui.ctx().content_rect());
    painter.add(Shape::convex_polygon(fill, t.surface, Stroke::NONE));
    painter.add(Shape::line(
        vec![left, tip, right],
        Stroke::new(1.0, t.line),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_takes_the_side_it_fits() {
        let below = |rooms, need| side(rooms, need, None, Side::Below);
        // Burmese lines run tall: 400 points fit above, not below.
        assert_eq!(below((300.0, 500.0), Some(400.0)), Side::Above);
        assert_eq!(below((500.0, 300.0), Some(400.0)), Side::Below);
        // Neither fits: the roomier side, the content scrolling.
        assert_eq!(below((350.0, 300.0), Some(600.0)), Side::Below);
        assert_eq!(below((300.0, 350.0), Some(600.0)), Side::Above);
        // Unmeasured, below wins while it leaves room.
        assert_eq!(below((300.0, 500.0), None), Side::Below);
        assert_eq!(below((100.0, 500.0), None), Side::Above);
    }

    /// One frame of a card whose scrolling body holds `rows` lines.
    fn frame(ctx: &Context, rows: usize) -> Rect {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(800.0, 800.0));
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let mut card = Rect::NOTHING;
        let mut out = ctx.run_ui(input, |ui| {
            let t = Tokens::get(ui.ctx());
            let bubble = Bubble {
                id: Id::new("grows"),
                at: Rect::from_min_size(pos2(300.0, 100.0), egui::vec2(40.0, 16.0)),
                room: screen,
                width: 200.0,
                margin: Margin::same(8),
                snug: false,
                prefer: Side::Below,
            };
            card = bubble
                .show(ui.ctx(), &t, |ui, cap| {
                    egui::ScrollArea::vertical()
                        .max_height(cap)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            (0..rows).for_each(|i| _ = ui.label(format!("row {i}")))
                        });
                })
                .0;
        });
        out.textures_delta.clear();
        card
    }

    /// The card's height once a few frames have shown `rows` lines.
    fn settled(ctx: &Context, rows: usize) -> f32 {
        for _ in 0..3 {
            frame(ctx, rows);
        }
        frame(ctx, rows).height()
    }

    #[test]
    fn a_card_grows_as_its_answer_streams_in() {
        let ctx = Context::default();
        let small = settled(&ctx, 1);
        let big = settled(&ctx, 12);
        assert!(big > small + 100.0, "{small} grew only to {big}");
    }

    #[test]
    fn a_placed_card_keeps_its_side_while_it_fits() {
        let was = Some(Side::Above);
        assert_eq!(
            side((500.0, 300.0), Some(200.0), was, Side::Below),
            Side::Above
        );
        // Grown past its side, it moves where it fits.
        assert_eq!(
            side((500.0, 300.0), Some(400.0), was, Side::Below),
            Side::Below
        );
    }

    #[test]
    fn a_toolbar_goes_above_its_selection_unless_there_is_no_room() {
        let above = |rooms| side(rooms, Some(44.0), None, Side::Above);
        assert_eq!(above((500.0, 300.0)), Side::Above);
        // At the top of the pane it drops under the words.
        assert_eq!(above((500.0, 20.0)), Side::Below);
    }
}
