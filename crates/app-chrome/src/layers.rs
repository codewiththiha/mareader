//! Z-index layer tokens. The numeric values live in `styles/tokens.css` as
//! `--z-*` custom properties; these class-name constants are what components
//! embed, so layering is one decision instead of ten scattered numbers. The
//! Tailwind compiler scans source text, so every token stays a static literal
//! and `z-[var(--z-popover)]` etc. ship in `styles.css`. Owned here because
//! the chrome surfaces are the layering's anchor; every floating surface,
//! toast and overlay reads the table from `app_chrome::layers`.

pub const CONTENT: &str = "z-0";
pub const CONTROLS: &str = "z-[var(--z-controls)]";
pub const BAR: &str = "z-[var(--z-bar)]";
pub const POPOVER: &str = "z-[var(--z-popover)]";
pub const SELECTION_BAR: &str = "z-[var(--z-selection-bar)]";
pub const CONTEXT_MENU: &str = "z-[var(--z-context-menu)]";
pub const AI_SELECTION: &str = "z-[var(--z-ai-selection)]";
pub const DRAG_OVERLAY: &str = "z-[var(--z-drag-overlay)]";
pub const TOAST: &str = "z-[var(--z-toast)]";
