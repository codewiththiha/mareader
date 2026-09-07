//! The reader's format-agnostic domain: what it can open, how its own choices
//! persist, and the pure maths every view mode shares — the settings and
//! appearance models and their two pipelines, theme presets, the zoom ladder,
//! the filename policy, the format registry, the view-mode/spread maths, the
//! search result model and the chapter-node type. A plain-text document is
//! tinted, zoomed, spread and searched through exactly the same code as a PDF.
//!
//! Axis 1 (pure computation) at its widest: no wasm, no DOM, no leptos,
//! nothing that knows what a page of PDF is. Everything is unit-testable on
//! the host (`cargo test -p reader-core`).
//!
//! ## What may live here
//!
//! * A type every format needs, or a policy that applies to all of them.
//! * No `Format` branch: a module that matches on the format belongs to that
//!   format's crate (`pdf-core`, `txt-core`, `md-core`).
//! * Only two workspace dependencies, both leaves whose types the persisted
//!   schema names: `pdf-paper` and `virtual-list`.
//!
//! Appearance is one tree (`appearance/`): the kernel (model, base palettes,
//! colour maths, noise/texture helpers, presets) at its root and the two
//! pipelines under it — `appearance::raster` for pages that arrive as pixels,
//! `appearance::reflowable` for pages laid out as DOM type. Neither reads the
//! other; both are pure colour computation over the shared
//! [`appearance::Appearance`], so the host test suite holds every number they
//! produce to account. The engine bridge that APPLIES the raster pipeline
//! stays in the app crate. The floating-box geometry and spring are
//! deliberately NOT here — they live in `ui-geom`, a dependency-free leaf.

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
