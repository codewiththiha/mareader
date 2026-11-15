# Mareader compatibility patch

Upstream: `block` 0.1.6, by Steven Sheldon, from crates.io and
https://github.com/SSheldon/rust-block (MIT).

The published crate declares `_NSConcreteStackBlock` with an empty-enum type.
Rust's `uninhabited_static` future-incompatibility lint rejects that foreign
static. `Class` is private and only its address and pointer layout cross the
FFI boundary, so the patch uses the standard zero-sized `#[repr(C)]` opaque
marker instead. It also spells out the existing `extern "C"` ABIs required by
current Rust. No block layout, callback, ownership, copy or dispose behavior
changes.

The crate is outside the workspace. Its upstream test-only modules and
unavailable `objc_test_utils` path dependency are omitted; production sources
otherwise remain upstream's. Edition 2024 also requires an unsafe foreign
block and explicit unsafe operations inside unsafe functions. Remove this
patch when every dependency path has moved from `block` to maintained
`objc2`/`block2` bindings.
