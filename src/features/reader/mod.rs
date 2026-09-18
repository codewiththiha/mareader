pub mod page;
pub(crate) mod ghost;
pub(crate) mod rail;
pub(crate) mod render_gate;
pub(crate) mod scroll_kinetics;
pub(crate) mod virtualizers;

pub use page::ReaderPage;
pub(crate) use virtualizers::{use_reader_virtualizers, SmartCfg, SmartVirtualizer};
