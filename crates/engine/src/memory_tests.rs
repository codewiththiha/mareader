//! The rest that hands the caches back, asked for between two frames.

use std::time::{Duration, Instant};

use super::give_back;

/// A frame at 60 Hz lasts 16 ms; the walk this starts waits a whole second.
const FRAME: Duration = Duration::from_millis(500);

#[test]
fn a_rest_leaves_the_frame_that_asked_for_it_to_draw() {
    let started = Instant::now();
    give_back();
    give_back();
    let taken = started.elapsed();
    assert!(
        taken < FRAME,
        "two rests took {taken:?}, so a frame waited on the walk"
    );
}
