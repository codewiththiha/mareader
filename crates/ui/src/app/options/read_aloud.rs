use super::*;

pub(super) fn set(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "english_voice" => Some(app.option_english_voice(key, value)),
        #[cfg(not(target_arch = "wasm32"))]
        "voice_models" => Some(app.option_voice_models(key, value)),
        "read_aloud" => Some(app.option_read_aloud(key, value)),
        "transcript" => Some(app.option_transcript(key, value)),
        "trigger" => Some(app.option_trigger(key, value)),
        "script" => Some(app.option_script(key, value)),
        "settings" => Some(app.option_settings(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    // The English voice, as if picked: `english_voice=Karen:en-AU`.
    fn option_english_voice(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let (name, tag) = value.split_once(':').ok_or("english_voice=name:tag")?;
        let (name, tag) = (name.replace('_', " "), tag.to_owned());
        self.english = Some(crate::sounds::EnglishVoice { name, tag });

        Ok(())
    }

    // The models on offer, kept in this folder.
    #[cfg(not(target_arch = "wasm32"))]
    fn option_voice_models(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let (dir, models) = (PathBuf::from(value), mareader_speech::MODELS);
        self.attach_voices(dir, models, Arc::new(|| {}))?;

        Ok(())
    }

    fn option_read_aloud(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.read_aloud_shot = value == "true";
        Ok(())
    }

    fn option_transcript(&mut self, _key: &str, value: &str) -> Result<(), String> {
        // `card` or `card:track`, comma separated.
        for one in value.split(',') {
            let (card, track) = one.split_once(':').unwrap_or((one, ""));
            let card = card.parse().map_err(|_| "transcript: card[:track],…")?;
            self.transcript_shot.push((card, track.parse().ok()));
        }

        Ok(())
    }

    fn option_trigger(&mut self, _key: &str, value: &str) -> Result<(), String> {
        // `target` shows it, `target:hide` hides it; comma separated.
        for one in value.split(',') {
            let (target, how) = one.split_once(':').unwrap_or((one, "show"));
            let target = target.parse().map_err(|_| "trigger: target[:hide],…")?;
            self.trigger_shot.push((target, how != "hide"));
        }

        Ok(())
    }

    // Clicks the first element each selector matches; comma separated.
    fn option_script(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.script_shot.extend(value.split(',').map(str::to_owned));
        Ok(())
    }

    fn option_settings(&mut self, _key: &str, value: &str) -> Result<(), String> {
        // A downloaded model's tab goes by the model's id.
        let model = self
            .read_aloud
            .shelf
            .ready()
            .into_iter()
            .find(|m| m.id == value);
        let tab = Tab::parse(value).or(model.map(Tab::Model));
        let tab = tab.ok_or("settings: a tab name, e.g. theme")?;
        self.open_settings(Some(tab));

        Ok(())
    }
}
