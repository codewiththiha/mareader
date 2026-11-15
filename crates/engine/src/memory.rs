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
#[path = "memory_tests.rs"]
mod tests;
