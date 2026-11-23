//! The backup models every AI task tries after its own.

use serde::{Deserialize, Serialize};

use crate::{AiSettings, ModelPreset, PresetId};

/// Backup models, top first. Automatic until the reader arranges them: then
/// the first [`Backups::AUTO`] saved models, oldest first, kept up to date.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Backups(Option<Vec<PresetId>>);

impl Backups {
    /// How many saved models the automatic list holds.
    pub const AUTO: usize = 3;

    /// Whether the reader has arranged the list.
    pub fn arranged(&self) -> bool {
        self.0.is_some()
    }

    /// The list, top first, of models still saved.
    pub fn ids(&self, presets: &[ModelPreset]) -> Vec<PresetId> {
        let saved = |id: &PresetId| presets.iter().any(|p| p.id == *id);
        match &self.0 {
            Some(ids) => ids.iter().copied().filter(saved).collect(),
            // Ids only grow, so the lowest are the oldest.
            None => {
                let mut ids: Vec<PresetId> = presets.iter().map(|p| p.id).collect();
                ids.sort_unstable();
                ids.truncate(Self::AUTO);
                ids
            }
        }
    }

    /// Keeps `ids` as the reader's list, each once.
    pub fn arrange(&mut self, ids: Vec<PresetId>) {
        let mut seen = Vec::new();
        let ids = ids.into_iter().filter(|id| {
            let first = !seen.contains(id);
            seen.push(*id);
            first
        });
        self.0 = Some(ids.collect());
    }

    /// Back to the automatic list.
    pub fn reset(&mut self) {
        self.0 = None;
    }

    /// Keeps only saved models, each once.
    pub(crate) fn prune(&mut self, presets: &[ModelPreset]) {
        if let Some(ids) = self.0.take() {
            let saved = |id: &PresetId| presets.iter().any(|p| p.id == *id);
            self.arrange(ids.into_iter().filter(saved).collect());
        }
    }
}

impl AiSettings {
    /// The backup models, top first.
    pub fn backups(&self) -> Vec<&ModelPreset> {
        let ids = self.tasks.backups.ids(&self.presets);
        ids.into_iter().filter_map(|id| self.preset(id)).collect()
    }
}

#[cfg(test)]
#[path = "ai_backups_tests.rs"]
mod tests;
