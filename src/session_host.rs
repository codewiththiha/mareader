//! Reader-host instance. Compiled only into the host artifact. The shelf
//! module is not in this heap, and this heap does not open the book.

#[cfg(target_arch = "wasm32")]
pub async fn mount() {
    crate::app::mount_reader();
}

#[cfg(target_arch = "wasm32")]
pub fn detach() {
    crate::app::dispose_reader();
}

#[cfg(target_arch = "wasm32")]
pub async fn dispose() {
    detach();
}
