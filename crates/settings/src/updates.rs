//! Software update preferences: startup checks and background installs.

use serde::{Deserialize, Serialize};

/// How the desktop app checks for and applies GitHub Releases.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Check GitHub Releases automatically at startup and notify when newer.
    pub check_on_startup: bool,
    /// Download and install updates silently on Windows and Linux.
    pub auto_update: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            auto_update: true,
        }
    }
}
