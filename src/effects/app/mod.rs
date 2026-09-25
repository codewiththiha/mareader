//! The shell's own effects: the durable theme/typography/motion paints on
//! `<html>` (they must survive every runtime transition) and the drag-drop
//! overlay arms the ACTIVE runtime owns — the shell installs none.

pub mod motion;
pub mod theme;
pub mod typography;
