//! Shelf instance. Compiled only into the library artifact. The reader host
//! does not start while this module is the page, so a closed book is not in
//! this heap.

#[cfg(target_arch = "wasm32")]
pub async fn mount() {
    crate::app::mount_shelf();
}

#[cfg(target_arch = "wasm32")]
pub fn detach() {
    crate::app::dispose_shelf();
}

#[cfg(target_arch = "wasm32")]
pub async fn dispose() {
    detach();
}
