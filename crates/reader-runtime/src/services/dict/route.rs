//! The panel's pack rows over the pure rules in `dict-core`.

use dict_core::PackLike;

pub use dict_core::panel::{
    Asked, Pair, Route, answer_pack, built_packs, languages, named_packs, resolve,
};

use super::wire::PackMirror;

impl PackLike for PackMirror {
    fn pack_id(&self) -> &str {
        &self.id
    }
    fn source(&self) -> &str {
        &self.source
    }
    fn target(&self) -> &str {
        &self.target
    }
    fn built(&self) -> bool {
        self.built
    }
}
