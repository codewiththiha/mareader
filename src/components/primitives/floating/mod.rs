//! The floating system: placement math + dismissal + the anchored surfaces
//! that compose them (popover, context menu, floating card).
//!
//! Position math is pure and lives in `pdf_core::floating` (host-testable);
//! `position.rs` adapts it to the DOM. `dismiss.rs` owns the shared
//! Escape/outside-press contract. `popover.rs` is the classic anchored menu,
//! `context_menu.rs` the cursor-point menu, `floating_card.rs` the advanced
//! phase-driven surface AI-style cards compose.

pub mod context_menu;
pub mod dismiss;
pub mod floating_card;
pub mod popover;
pub mod position;
pub mod types;
