//! Self-contained commands that mutate [`Session`] settings, panes, and route.

use crate::command::Command;

use super::{Done, Session};

impl Session {
    /// Carries out a command that needs no controller: panes, route, and dials.
    ///
    /// `execute` routes those here; the rest answer quiet, untouched.
    pub(crate) fn apply(&mut self, intent: Command) -> Done {
        match intent {
            Command::FocusPane(id) => {
                self.focus(id);
                Done::changed()
            }
            Command::ClosePane(id) => match self.close(id) {
                true => Done::changed(),
                false => Done::quiet(),
            },
            Command::SetRatio(path, ratio) => {
                self.tree.set_ratio(&path, ratio);
                Done::changed()
            }
            Command::SwapPanes { pane, other } => self.swap(pane, other),
            Command::MovePane {
                pane,
                other,
                edge,
                area,
            } => self.move_to(pane, other, edge, area),
            Command::ShowRoute(route) => {
                self.show(route);
                Done::changed()
            }
            Command::SetAppearance(look) => {
                self.set_appearance(look);
                Done::changed()
            }
            Command::Vocab(action) => {
                self.vocab_act(action);
                Done::changed()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Command::AttachVocab(root) => {
                self.attach_vocab(root);
                Done::changed()
            }
            Command::Pronunciation(action) => {
                self.pronunciation_act(action);
                Done::changed()
            }
            Command::SetSettings(settings) => {
                let mut settings = *settings;
                settings.sanitize();
                if self.settings != settings {
                    self.settings = settings;
                    self.save.touch();
                }
                Done::changed()
            }
            Command::SetField { path, value } => self.put(&path, Ok(value)),
            other => self.dial(other),
        }
    }

    /// The dials a command names outright, each written as its path is.
    fn dial(&mut self, intent: Command) -> Done {
        match intent {
            Command::SetSidebarOverlay(on) => {
                if self.settings.chrome.sidebar_overlay != on {
                    self.settings.chrome.sidebar_overlay = on;
                    self.save.touch();
                }
                Done::changed()
            }
            Command::SetTitlebarPinned(on) => {
                let bar = self.route.bar();
                if self.settings.chrome.pinned(bar) != on {
                    self.settings.chrome.pin(bar, on);
                    self.save.touch();
                }
                Done::changed()
            }
            Command::SetInkContrast(ink) => {
                self.put("text.ink_contrast", serde_json::to_string(&ink))
            }
            Command::SetQuickText(quick) => {
                let mut text = self.settings.text.clone();
                quick.apply(&mut text);
                self.put("text", serde_json::to_string(&text))
            }
            Command::SetIndependence(own) => {
                let themes = serde_json::to_string(&own.themes);
                self.put("workspace.independent_themes", themes);
                let textures = serde_json::to_string(&own.textures);
                self.put("workspace.independent_textures", textures)
            }
            Command::SavePreset { name, group, look } => {
                self.settings.save_preset(&name, &group, look);
                self.settings.sanitize();
                self.save.touch();
                Done::changed()
            }
            Command::DeletePreset(id) => {
                self.settings.delete_preset(&id);
                self.settings.sanitize();
                self.save.touch();
                Done::changed()
            }
            Command::SetDictionary(dictionary) => {
                self.put("dictionary", serde_json::to_string(&*dictionary))
            }
            Command::SetLibraryView(view) => self.put("library", serde_json::to_string(&view)),
            _ => Done::quiet(),
        }
    }

    /// One setting by its dotted path; a bad path or value is refused.
    fn put(&mut self, path: &str, json: serde_json::Result<String>) -> Done {
        let mut next = self.settings.clone();
        let set = json
            .map_err(|e| e.to_string())
            .and_then(|value| next.set_path(path, &value));
        match set {
            Ok(()) => {
                if next != self.settings {
                    self.settings = next;
                    self.save.touch();
                }
                Done::changed()
            }
            Err(why) => Done {
                dirty: false,
                refused: Some(why),
            },
        }
    }
}
