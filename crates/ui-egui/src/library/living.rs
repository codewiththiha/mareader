//! The living grid's state: the hole a carry opens, and card springs.

use std::collections::{HashMap, HashSet};

use egui::{Pos2, Vec2, vec2};

use mareader_library::{BookId, ShelfId};

use mareader_library::picks::Pick;

/// How stiff the springs are: cards settle in about a fifth of a second.
const OMEGA: f32 = 30.0;
/// Closer and slower than these, a card is where it belongs.
const NEAR: f32 = 0.25;
const SLOW: f32 = 4.0;
/// The longest step one integration takes, for steadiness.
const STEP: f32 = 1.0 / 120.0;
/// Rests before the hole moves, a stack forms, or a shelf opens.
const HOLE_DWELL: f64 = 0.18;
const STACK_DWELL: f64 = 0.3;
pub(super) const OPEN_DWELL: f64 = 0.7;

/// What can sit in a slot and move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Card(Pick),
    /// The plus card that ends the grid.
    Add,
}

#[derive(Clone, Copy, Debug)]
struct Spring {
    pos: Vec2,
    vel: Vec2,
    seen: u64,
}

impl Spring {
    /// Critically damped toward `target`; true while still moving.
    fn step(&mut self, target: Vec2, dt: f32) -> bool {
        let mut left = dt;
        while left > 0.0 {
            let h = left.min(STEP);
            let pull = (target - self.pos) * (OMEGA * OMEGA) - self.vel * (2.0 * OMEGA);
            self.vel += pull * h;
            self.pos += self.vel * h;
            left -= h;
        }
        let moving = (target - self.pos).length() > NEAR || self.vel.length() > SLOW;
        if !moving {
            (self.pos, self.vel) = (target, Vec2::ZERO);
        }
        moving
    }
}

/// Where the lifted cards will land: `size` slots from `at` among the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hole {
    pub at: usize,
    pub size: usize,
    /// Where cards lifted here opened it; none for cards from elsewhere.
    pub start: Option<usize>,
}

impl Hole {
    /// What slot `slot` shows: a rest index, or none for the hole.
    pub fn rest_at(&self, slot: usize) -> Option<usize> {
        match slot {
            s if s < self.at => Some(s),
            s if s < self.at + self.size => None,
            s => Some(s - self.size),
        }
    }
}

/// What resting the pointer on a place asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aim {
    /// The hole, at a rest index.
    Hole(usize),
    /// A new shelf of this book and what's carried.
    Stack(BookId),
    /// This shelf opened, to carry on inside it.
    Open(ShelfId),
}

impl Aim {
    fn dwell(self) -> f64 {
        match self {
            Self::Hole(_) => HOLE_DWELL,
            Self::Stack(_) => STACK_DWELL,
            Self::Open(_) => OPEN_DWELL,
        }
    }
}

/// What a rest came to this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rested {
    Nothing,
    /// Still resting; this many seconds to go.
    Wait(f64),
    Open(ShelfId),
}

/// Cards carried over this level, hidden from the grid while held.
#[derive(Debug)]
pub struct Lift {
    /// In the order shown.
    pub moving: Vec<Pick>,
    set: HashSet<Pick>,
    pub hole: Hole,
    /// The level shown when it was lifted; none is the top.
    pub level: Option<ShelfId>,
    /// The book a rest made ready to stack onto.
    pub stack: Option<BookId>,
    /// What the pointer rests on, and since when.
    rest: Option<(Aim, f64)>,
    /// Where the pointer was as a shelf opened; nothing aims till it moves.
    pub hold: Option<Pos2>,
}

impl Lift {
    pub fn holds(&self, pick: Pick) -> bool {
        self.set.contains(&pick)
    }

    /// Whether the cards came from this level, not from elsewhere.
    pub fn native(&self) -> bool {
        self.hole.start.is_some()
    }

    /// The pointer on `want` at `now`; a long enough rest makes it so.
    pub fn aim(&mut self, want: Option<Aim>, now: f64) -> Rested {
        // A stack holds only while the pointer stays on its book.
        if self.stack.map(Aim::Stack) != want {
            self.stack = None;
        }
        let done = match want {
            Some(Aim::Hole(at)) => at == self.hole.at,
            Some(Aim::Stack(book)) => self.stack == Some(book),
            Some(Aim::Open(_)) => false,
            None => true,
        };
        let Some(want) = want.filter(|_| !done) else {
            self.rest = None;
            return Rested::Nothing;
        };
        let since = match self.rest {
            Some((aim, since)) if aim == want => since,
            _ => now,
        };
        let left = want.dwell() - (now - since);
        if left > 0.0 {
            self.rest = Some((want, since));
            return Rested::Wait(left);
        }
        self.rest = None;
        match want {
            Aim::Hole(at) => self.hole.at = at,
            Aim::Stack(book) => self.stack = Some(book),
            Aim::Open(shelf) => return Rested::Open(shelf),
        }
        Rested::Nothing
    }
}

/// The card a drag picked up, and where in it the pointer pressed.
#[derive(Clone, Copy, Debug)]
pub struct Grab {
    pub pick: Pick,
    pub offset: Vec2,
}

/// The grid's moving parts, kept between frames for cards drawn.
#[derive(Debug, Default)]
pub struct Motion {
    springs: HashMap<Slot, Spring>,
    /// Columns, card width and level shown; a change snaps every card.
    layout: (usize, u32, Option<ShelfId>),
    frame: u64,
    dt: f32,
    animate: bool,
    busy: bool,
    pub lift: Option<Lift>,
    pub grab: Option<Grab>,
    /// The grid's content origin this frame, on screen.
    pub origin: Pos2,
    /// A card's size, its text included.
    pub card: Vec2,
    /// Where the ghost was last drawn, in content space.
    pub ghost: Option<Vec2>,
}

impl Motion {
    /// Starts a frame of the grid.
    pub fn begin(&mut self, shape: (usize, f32, Option<ShelfId>), dt: f32, animate: bool) {
        let (cols, card, level) = shape;
        let layout = (cols, card.round() as u32, level);
        if layout != self.layout || !animate {
            self.springs.clear();
            self.layout = layout;
        }
        self.frame += 1;
        (self.dt, self.animate, self.busy) = (dt, animate, false);
    }

    /// Where `slot` is drawn this frame, on its way to `target`.
    pub fn place(&mut self, slot: Slot, target: Vec2) -> Vec2 {
        if !self.animate {
            return target;
        }
        let frame = self.frame;
        let spring = self.springs.entry(slot).or_insert(Spring {
            pos: target,
            vel: Vec2::ZERO,
            seen: frame,
        });
        spring.seen = frame;
        self.busy |= spring.step(target, self.dt);
        spring.pos
    }

    /// Ends a frame: forgets cards not drawn; true while any moves.
    pub fn end(&mut self) -> bool {
        let frame = self.frame;
        self.springs.retain(|_, s| s.seen == frame);
        self.busy
    }

    /// Lifts `moving` over `level`; cards from elsewhere open the hole last.
    pub fn pick_up(
        &mut self,
        mut moving: Vec<Pick>,
        shown: &[Pick],
        level: Option<ShelfId>,
        native: bool,
    ) {
        let set: HashSet<Pick> = moving.iter().copied().collect();
        let first = shown.iter().position(|p| set.contains(p));
        if native && first.is_none() {
            return;
        }
        let place: HashMap<Pick, usize> = shown
            .iter()
            .enumerate()
            .filter(|(_, p)| set.contains(*p))
            .map(|(n, p)| (*p, n))
            .collect();
        if native {
            moving.retain(|p| place.contains_key(p));
        }
        // Shown cards in their order; the rest after, as carried.
        moving.sort_by_key(|p| place.get(p).copied().unwrap_or(usize::MAX));
        let at = first.unwrap_or(shown.len());
        let hole = Hole {
            at,
            size: moving.len(),
            start: native.then_some(at),
        };
        self.lift = Some(Lift {
            moving,
            set,
            hole,
            level,
            stack: None,
            rest: None,
            hold: None,
        });
    }

    /// Drops any carry without a landing, as the list does.
    pub fn forget(&mut self) {
        (self.lift, self.grab, self.ghost) = (None, None, None);
    }

    /// Ends a carry: lifted cards fly from the ghost to their slots.
    pub fn put_down(&mut self) {
        let lift = self.lift.take();
        let ghost = self.ghost.take();
        self.grab = None;
        let (Some(lift), Some(from)) = (lift, ghost) else {
            return;
        };
        if !self.animate {
            return;
        }
        let frame = self.frame;
        for (n, pick) in lift.moving.into_iter().enumerate() {
            // A selection fans out a little as it lands.
            let spring = Spring {
                pos: from + vec2(n as f32 * 4.0, n as f32 * 4.0),
                vel: Vec2::ZERO,
                seen: frame,
            };
            self.springs.insert(Slot::Card(pick), spring);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(n: u128) -> Pick {
        Pick::Book(BookId(n))
    }

    fn lifted(at: usize) -> Lift {
        let mut motion = Motion::default();
        let shown: Vec<Pick> = (0..6).map(book).collect();
        motion.pick_up(vec![book(at as u128)], &shown, None, true);
        motion.lift.unwrap()
    }

    #[test]
    fn the_hole_moves_only_where_the_pointer_rests() {
        let mut lift = lifted(0);
        assert_eq!(lift.aim(Some(Aim::Hole(0)), 0.0), Rested::Nothing);
        // Passing over a place on the way leaves the hole put.
        let pass = [(1, 1.0), (3, 1.1), (3, 1.2)];
        for (at, now) in pass {
            let rested = lift.aim(Some(Aim::Hole(at)), now);
            assert!(matches!(rested, Rested::Wait(_)));
        }
        assert_eq!(lift.hole.at, 0);
        assert_eq!(lift.aim(Some(Aim::Hole(3)), 1.3), Rested::Nothing);
        assert_eq!(lift.hole.at, 3);
        let hole = lift.hole;
        assert_eq!(
            (hole.rest_at(2), hole.rest_at(3), hole.rest_at(4)),
            (Some(2), None, Some(3))
        );
        // Leaving for empty space forgets what was asked.
        lift.aim(Some(Aim::Hole(1)), 2.0);
        lift.aim(None, 2.1);
        let again = lift.aim(Some(Aim::Hole(1)), 2.2);
        assert!(matches!(again, Rested::Wait(_)));
    }

    #[test]
    fn a_longer_rest_stacks_while_held_and_opens_once() {
        let mut lift = lifted(0);
        let on = Some(Aim::Stack(BookId(2)));
        lift.aim(on, 0.0);
        lift.aim(on, 0.2);
        assert_eq!(lift.stack, None, "not yet");
        lift.aim(on, 0.31);
        assert_eq!(lift.stack, Some(BookId(2)));
        lift.aim(on, 0.5);
        assert_eq!(lift.stack, Some(BookId(2)), "held");
        lift.aim(Some(Aim::Hole(4)), 0.6);
        assert_eq!(lift.stack, None, "moved off");
        let shelf = Some(Aim::Open(ShelfId(7)));
        lift.aim(shelf, 1.0);
        assert!(matches!(lift.aim(shelf, 1.5), Rested::Wait(_)));
        assert_eq!(lift.aim(shelf, 1.71), Rested::Open(ShelfId(7)));
    }

    #[test]
    fn springs_settle_on_target_without_flying_past_it() {
        let mut motion = Motion::default();
        let slot = Slot::Card(book(1));
        motion.begin((4, 150.0, None), 1.0 / 60.0, true);
        motion.place(slot, Vec2::ZERO);
        let mut frames = 0;
        loop {
            motion.begin((4, 150.0, None), 1.0 / 60.0, true);
            let at = motion.place(slot, vec2(170.0, 0.0));
            assert!(at.x <= 170.5, "{at:?}");
            frames += 1;
            if !motion.end() {
                break;
            }
            assert!(frames < 60, "still moving after a second");
        }
        assert!(frames > 3, "it eased");
    }

    #[test]
    fn a_new_layout_or_motion_off_puts_cards_straight_in_place() {
        let mut motion = Motion::default();
        let slot = Slot::Add;
        motion.begin((4, 150.0, None), 1.0 / 60.0, true);
        motion.place(slot, Vec2::ZERO);
        motion.begin((5, 120.0, None), 1.0 / 60.0, true);
        assert_eq!(motion.place(slot, vec2(90.0, 0.0)), vec2(90.0, 0.0));
        // Another level is another page.
        motion.begin((5, 120.0, Some(ShelfId(1))), 1.0 / 60.0, true);
        assert_eq!(motion.place(slot, vec2(40.0, 0.0)), vec2(40.0, 0.0));
        motion.begin((5, 120.0, None), 1.0 / 60.0, false);
        assert_eq!(motion.place(slot, vec2(10.0, 0.0)), vec2(10.0, 0.0));
        assert!(!motion.end());
    }

    #[test]
    fn a_lift_keeps_the_order_shown_and_opens_where_the_first_was() {
        let mut motion = Motion::default();
        let shown = [book(1), book(2), book(3), book(4)];
        motion.pick_up(vec![book(4), book(2), book(9)], &shown, None, true);
        let lift = motion.lift.as_ref().unwrap();
        assert_eq!(lift.moving, [book(2), book(4)]);
        assert_eq!((lift.hole.at, lift.hole.size), (1, 2));
        assert!(lift.holds(book(4)) && !lift.holds(book(3)));
        let mut none = Motion::default();
        none.pick_up(vec![book(9)], &shown, None, true);
        assert!(none.lift.is_none());
        // Brought from elsewhere, cards open a hole at the end.
        let mut visit = Motion::default();
        visit.pick_up(vec![book(9), book(8)], &shown, None, false);
        let lift = visit.lift.as_ref().unwrap();
        assert_eq!((lift.hole.at, lift.hole.size, lift.native()), (4, 2, false));
    }
}
