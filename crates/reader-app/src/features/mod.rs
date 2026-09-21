//! The reader crate's page assembly: the surface ([`root::ReaderRoot`]),
//! the rail composition ([`rail`]), and the virtualizer handles both the
//! route's effects and the surface share ([`virtualizers`]).

pub mod rail;
pub mod root;
pub mod virtualizers;

pub use root::ReaderRoot;
pub use virtualizers::{ReaderVirtualizers, use_reader_virtualizers};
