//! What the caches freed at rest goes back to the system, off the frame.

/// Walks the heap on a helper thread, so a rest never stalls a frame.
pub fn give_back() {
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::thread::Builder::new()
        .name("give-back".into())
        .spawn(|| {
            // Once the GPU's deferred frees have landed.
            std::thread::sleep(std::time::Duration::from_secs(1));
            mareader_memory::give_back();
        });
}

#[cfg(test)]
mod tests {
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
}
