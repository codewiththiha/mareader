//! The background work the AI settings ask for.
//!
//! Plain data that names only settings types, so it can sit below the crate
//! that runs the work and the one that commands it. A command has to be able
//! to carry a job, and a windowless build has to be able to say what it is.

use crate::{ModelPreset, Provider};

/// Background work the settings ask for.
#[derive(Clone, PartialEq, Eq)]
pub enum Job {
    /// Whether Apple Intelligence is ready.
    Probe,
    Keys(Provider),
    AddKey(Provider, String),
    RemoveKey(Provider, String),
    BanKey(Provider, String),
    RecoverKey(Provider, String),
    Models(Provider),
    /// Explains a sample word with a saved model.
    Try(ModelPreset),
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // A key must not reach a log.
            Self::AddKey(p, _) => write!(f, "AddKey({p:?}, <secret>)"),
            Self::Probe => write!(f, "Probe"),
            Self::Keys(p) => write!(f, "Keys({p:?})"),
            Self::RemoveKey(p, id) => write!(f, "RemoveKey({p:?}, {id})"),
            Self::BanKey(p, id) => write!(f, "BanKey({p:?}, {id})"),
            Self::RecoverKey(p, id) => write!(f, "RecoverKey({p:?}, {id})"),
            Self::Models(p) => write!(f, "Models({p:?})"),
            Self::Try(p) => write!(f, "Try({:?})", p.id),
        }
    }
}
