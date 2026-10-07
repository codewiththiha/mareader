//! The zoom subsystem: one controller, one transition pipeline.

pub mod actuator;
pub mod animation;
pub mod command;
pub mod config;
pub mod coordinator;
pub mod target;

pub use coordinator::ZoomController;
