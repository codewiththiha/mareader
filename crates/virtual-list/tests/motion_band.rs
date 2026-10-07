//! The motion model's promises as numbers.

use virtual_list::{BandWindow, FillPriority, Motion, MotionConfig, Pipeline, Window};

const DT: f64 = 16.0;
const VIEWPORT: f64 = 1_400.0;

fn win(first: usize, last: usize) -> Window {
    Window { first, last }
}

/// Feed `frames` samples of a scroll at `px_s`; return the band.
fn scroll_at(
    motion: &mut Motion,
    px_s: f64,
    frames: usize,
    ctx: &Pipeline,
    viewport: f64,
    overscan: f64,
    clock: &mut f64,
) -> BandWindow {
    let per_frame = px_s * DT / 1_000.0;
    let mut offset = motion.offset();
    let mut band = motion.band(offset, viewport, overscan, ctx);
    for _ in 0..frames {
        offset += per_frame;
        *clock += DT;
        motion.update(offset, *clock);
        band = motion.band(offset, viewport, overscan, ctx);
    }
    band
}

/// A fast pipeline: 1000 px pages, two lanes, 120 ms a fill.
const QUICK: Pipeline = Pipeline {
    fill_ms: 120.0,
    pitch: 1_000.0,
    lanes: 2,
};

/// The same document with no measured pipeline: no capacity.
const UNMEASURED: Pipeline = Pipeline {
    fill_ms: 120.0,
    pitch: 1_000.0,
    lanes: 0,
};

/// One expensive bake at a time.
const STRUGGLING: Pipeline = Pipeline {
    fill_ms: 500.0,
    pitch: 900.0,
    lanes: 1,
};

#[test]
fn an_ordinary_scroll_never_shows_a_placeholder() {
    // 3 000 px/s against 10 000 px/s of fill: content wins the race.
    let ctx = Pipeline {
        fill_ms: 200.0,
        pitch: 1_000.0,
        lanes: 2,
    };
    assert_eq!(ctx.items_per_s(), 10.0);
    assert_eq!(ctx.capacity_px_s(), 10_000.0);

    let mut clock = 0.0;
    let mut motion = Motion::new(MotionConfig::default());
    let band = scroll_at(&mut motion, 3_000.0, 20, &ctx, VIEWPORT, 0.0, &mut clock);
    assert!(
        band.speed_px_s > 2_900.0,
        "the estimate tracks it: {band:?}"
    );
    assert!(!motion.engaged());
    assert!(!band.placeholder, "3 000 px/s cannot outrun 10 000 px/s");
    assert_eq!(band.lead_px, 0.0, "no lead is owed at ordinary speeds");
    // Page 3 is real content the moment its row mounts.
    for index in 0..=20 {
        assert_eq!(
            motion.priority(index, win(1, 2), None),
            FillPriority::Visible,
            "index {index} must not wait behind a placeholder"
        );
    }
}

#[test]
fn the_lead_is_the_distance_covered_during_one_fill_and_saturates() {
    assert!((QUICK.capacity_px_s() - 16_666.667).abs() < 0.01);

    // 20 000 px/s: 2 400 px of lead, one 120 ms fill of ground.
    let mut clock = 0.0;
    let mut motion = Motion::new(MotionConfig::default());
    let band = scroll_at(&mut motion, 20_000.0, 40, &QUICK, VIEWPORT, 0.0, &mut clock);
    assert!(motion.engaged(), "20 000 px/s outruns 16 667 px/s");
    assert!(
        (band.lead_px - band.speed_px_s * 0.12).abs() < 2.0,
        "lead must be speed x fill_ms: {band:?}"
    );
    assert!(band.lead_px < 2.0 * VIEWPORT);

    // Twice as fast asks twice the lead, and the ceiling answers.
    let mut faster = Motion::new(MotionConfig::default());
    let mut clock = 0.0;
    let doubled = scroll_at(&mut faster, 40_000.0, 40, &QUICK, VIEWPORT, 0.0, &mut clock);
    assert!(
        (doubled.lead_px - 2.0 * VIEWPORT).abs() < 1e-9,
        "saturated at max_lead: {doubled:?}"
    );

    // The band is never narrower than the mount padding.
    let mut idle = Motion::new(MotionConfig::default());
    let mut clock = 0.0;
    let padded = scroll_at(
        &mut idle,
        1_500.0,
        60,
        &UNMEASURED,
        VIEWPORT,
        900.0,
        &mut clock,
    );
    assert!(idle.engaged(), "with no capacity the floor decides");
    assert!(
        (padded.lead_px - 900.0).abs() < 1e-9,
        "overscan is the floor: {padded:?}"
    );
}

#[test]
fn the_engagement_latch_opens_wide_and_closes_tight() {
    let config = MotionConfig::default();
    let enter = STRUGGLING.enter_px_s(&config);
    let exit = STRUGGLING.exit_px_s(&config);
    assert_eq!(enter, 1_800.0, "capacity, not the 1 400 px/s floor");
    assert_eq!(exit, 720.0, "and the exit gate is the entry gate, scaled");
    assert!(exit < enter, "that gap is the hysteresis");

    // Arriving from rest at 1 000 px/s: the band never engages.
    let mut m = Motion::new(config);
    let mut clock = 0.0;
    scroll_at(&mut m, 1_000.0, 40, &STRUGGLING, 1_000.0, 0.0, &mut clock);
    assert!(m.speed_px_s() < enter);
    assert!(
        !m.engaged(),
        "1 000 px/s sits between the gates: not a seek"
    );

    // The same speed, already engaged: it holds, out only below the exit.
    let mut m = Motion::new(config);
    let mut clock = 0.0;
    scroll_at(&mut m, 2_500.0, 30, &STRUGGLING, 1_000.0, 0.0, &mut clock);
    assert!(m.engaged(), "2 500 px/s is a seek");
    scroll_at(&mut m, 1_000.0, 30, &STRUGGLING, 1_000.0, 0.0, &mut clock);
    assert!(m.speed_px_s() < enter, "it has slowed past the entry gate");
    assert!(
        m.engaged(),
        "but not below the exit gate, so the band holds"
    );
    assert!(m.band(0.0, 1_000.0, 0.0, &STRUGGLING).placeholder);
    // Stop for real and it lets go without another threshold being consulted.
    scroll_at(&mut m, 0.0, 60, &STRUGGLING, 1_000.0, 0.0, &mut clock);
    assert!(!m.engaged(), "{m:?}");
    assert!(!m.band(0.0, 1_000.0, 0.0, &STRUGGLING).placeholder);
}

#[test]
fn scrollend_ends_the_state_without_waiting_for_the_frames() {
    let mut m = Motion::new(MotionConfig::default());
    let mut clock = 0.0;
    scroll_at(&mut m, 6_000.0, 12, &STRUGGLING, 1_000.0, 0.0, &mut clock);
    assert!(m.engaged());
    m.settle();
    assert_eq!(m.velocity_px_s(), 0.0);
    assert_eq!(m.direction(), virtual_list::Direction::Still);
    assert!(!m.engaged());
    let band = m.band(m.offset(), 1_000.0, 0.0, &STRUGGLING);
    assert!(!band.placeholder, "the reader has stopped: fill everything");
}

#[test]
fn a_recoil_at_the_end_of_a_flick_does_not_move_the_lead() {
    let ctx = STRUGGLING;
    let mut m = Motion::new(MotionConfig::default());
    let mut at = 0.0;
    for _ in 0..12 {
        at += DT;
        let next = m.offset() + 192.0; // 12 000 px/s
        m.update(next, at);
    }
    assert_eq!(m.direction(), virtual_list::Direction::Forward);
    // One frame of recoil against a fast estimate: the direction holds.
    at += DT;
    m.update(m.offset() - 5.0, at);
    let band = m.band(m.offset(), 1_000.0, 0.0, &ctx);
    assert_eq!(
        band.direction,
        virtual_list::Direction::Forward,
        "the lead side must not flip: {band:?}"
    );
}

#[test]
fn the_estimate_does_not_depend_on_the_refresh_rate() {
    // 6 000 px/s reads the same at 30 Hz and 120 Hz.
    let mut at30 = Motion::new(MotionConfig::default());
    let mut at120 = Motion::new(MotionConfig::default());
    for i in 0..40_u32 {
        let t = f64::from(i + 1) * (1_000.0 / 30.0);
        at30.update(f64::from(i + 1) * 200.0, t);
    }
    for i in 0..144_u32 {
        let t = f64::from(i + 1) * (1_000.0 / 120.0);
        at120.update(f64::from(i + 1) * 50.0, t);
    }
    let ratio = at30.speed_px_s() / at120.speed_px_s();
    assert!((0.99..=1.01).contains(&ratio), "{at30:?} vs {at120:?}");
    assert!((at30.speed_px_s() - 6_000.0).abs() < 60.0, "{at30:?}");
    assert_eq!(
        at30.engaged(),
        at120.engaged(),
        "same scroll, same verdict on the same pipeline"
    );
}

#[test]
fn priority_ranks_the_viewport_then_the_approaching_side() {
    let ctx = STRUGGLING;
    let mut m = Motion::new(MotionConfig::default());
    let mut clock = 0.0;
    scroll_at(&mut m, 12_000.0, 12, &ctx, 1_000.0, 0.0, &mut clock);
    // Mounted 8..=18, 10..=12 visible, band 9..=16 carrying content.
    let visible = win(10, 12);
    let band = Some(win(9, 16));
    assert_eq!(m.priority(11, visible, band), FillPriority::Visible);
    assert_eq!(m.priority(15, visible, band), FillPriority::Ahead);
    assert_eq!(m.priority(9, visible, band), FillPriority::Behind);
    assert_eq!(m.priority(17, visible, band), FillPriority::Warm);
    // The ranks sort into exactly that order.
    let mut queue = vec![17, 9, 15, 11];
    queue.sort_by_key(|index| m.priority(*index, visible, band).rank());
    assert_eq!(queue, vec![11, 15, 9, 17]);

    // Reversing turns the queue around.
    let mut back = Motion::new(MotionConfig::default());
    let mut clock = 0.0;
    scroll_at(&mut back, -12_000.0, 12, &ctx, 1_000.0, 0.0, &mut clock);
    assert_eq!(back.direction(), virtual_list::Direction::Backward);
    assert_eq!(back.priority(9, visible, band), FillPriority::Ahead);
    assert_eq!(back.priority(15, visible, band), FillPriority::Behind);
}

#[test]
fn the_landing_index_aims_a_prefetch_a_fill_latency_ahead() {
    let ctx = STRUGGLING;
    let mut m = Motion::new(MotionConfig::default());
    let mut clock = 0.0;
    scroll_at(&mut m, 12_000.0, 12, &ctx, 1_000.0, 0.0, &mut clock);
    let aimed = m.landing_index(10, 900.0, 500.0);
    assert!(aimed > 10, "a forward fling lands later: {aimed}");
    // The partial page rounds toward the reader.
    let ceiling = 10 + (m.speed_px_s() * 0.5 / 900.0).ceil() as usize;
    assert!(aimed <= ceiling, "{aimed} vs {ceiling}");

    let mut back = Motion::new(MotionConfig::default());
    let mut clock = 0.0;
    scroll_at(&mut back, -12_000.0, 12, &ctx, 1_000.0, 0.0, &mut clock);
    assert_eq!(back.landing_index(1, 900.0, 500.0), 0, "never below zero");
    let rest = Motion::new(MotionConfig::default());
    assert_eq!(rest.landing_index(7, 900.0, 500.0), 7, "at rest, here");
    assert_eq!(
        rest.landing_index(0, 0.0, 0.0),
        0,
        "an unknown pitch is safe"
    );
}
