//! The reader's format-agnostic domain: what it can open, how it persists
//! the reader's own choices, and the pure maths every view mode shares.
//!
//! This is the crate that used to be the miscellaneous half of `pdf-core`.
//! The settings model, the appearance model and its two pipelines, the theme
//! presets, the zoom ladder, the filename policy,
//! the format registry, the view-mode/spread maths, the search result model
//! and the chapter-node type are all the reader's, not PDF's — a plain-text
//! document is tinted, zoomed, spread and searched through exactly the same
//! code, and naming that shared half after one format is what made every
//! new format start life as a special case.
//!
//! The four axes of the codebase, this crate is axis 1 (pure computation) at
//! its widest: no wasm, no DOM, no leptos, nothing that knows what a page of
//! PDF is. Everything here is unit-testable on the host via
//! `cargo test -p reader-core`.
//!
//! ## What may live here
//!
//! * A type every format needs, or a policy that applies to all of them.
//! * No `Format` branch: the moment a module starts matching on the format it
//!   belongs to that format's crate (`pdf-core`, `txt-core`, `md-core`).
//! * Only two workspace dependencies, both leaves whose types the persisted
//!   schema names: `pdf-paper` (which pixels of a page carry the paper colour)
//!   and `virtual-list` (how far ahead a strip mounts).
//!
//! Appearance is one tree, `appearance/`, and it is the one deliberate
//! narrowing in this crate: the kernel (model, base palettes, colour maths,
//! noise and texture helpers, presets) at its root, and the two pipelines
//! under it — `appearance::raster` for pages that arrive as pixels from a
//! renderer, `appearance::reflowable` for pages laid out as DOM type. They
//! are named for that split rather than for a file format because the split
//! is what they key on, and neither reads the other. They live here rather
//! than in the format crates because they are pure colour computation over
//! the shared [`appearance::Appearance`] type — no DOM, no engine — so the
//! host test suite can hold every number they produce to account. The engine
//! bridge that APPLIES the raster pipeline stays in the app crate.
//!
//! The floating-box geometry and the spring those boxes ride are deliberately
//! NOT here: they live in `ui-geom`, a dependency-free leaf. The chrome crate
//! places popovers with that math and the AI feature steps its gloss card on
//! that spring, and neither should have to reach into the reader's domain — or
//! into a feature — to do it.

pub mod appearance;
pub mod filename;
pub mod format;
pub mod outline;
pub mod search;
pub mod settings;
pub mod view;
pub mod zoom_math;

pub use format::{DocumentKind, Format, extensions, first_supported, format_of, is_supported_mime, is_supported_path, kind_list, kind_names, SUPPORTED};
pub use outline::OutlineNode;
