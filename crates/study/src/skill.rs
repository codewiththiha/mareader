//! A skill run on some text, as the engine sees it, and who answers.
//!
//! The engine names a skill by its key and hands over the text; the AI
//! service looks up the prompt and the models. The answer is plain text.

use crate::explain::{Failure, Why};
use crate::pending::{News, Pending};

/// A skill asked about some text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillAsk {
    /// The skill's key, as `translate` or `custom-3`.
    pub skill: String,
    pub text: String,
}

/// A skill's answer on its way: the text so far, then all of it.
pub type SkillJob = dyn Pending<String>;

/// Who answers skills.
pub trait Skiller: Send + Sync {
    /// Who answers `skill` now, as the card's small print names them.
    fn by(&self, skill: &str) -> String;

    /// Starts on `ask`; dropping the job cancels it.
    fn run(&self, ask: SkillAsk) -> Box<SkillJob>;
}

/// Answers from a script, for tests and shots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scripted {
    /// Hands the text back, as if rewritten, at once.
    #[default]
    Echo,
    /// A made-up answer as long as a real one, for shots.
    Sample,
    /// Never answers, so the card keeps waiting.
    Silent,
    /// Fails as an unreachable server does.
    Failing,
    /// No AI at all, as a build without one.
    Nobody,
}

impl Scripted {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim() {
            "echo" => Some(Self::Echo),
            "sample" => Some(Self::Sample),
            "silent" => Some(Self::Silent),
            "failing" => Some(Self::Failing),
            "none" => Some(Self::Nobody),
            _ => None,
        }
    }
}

impl Skiller for Scripted {
    fn by(&self, _: &str) -> String {
        match self {
            Self::Nobody => String::new(),
            _ => "Sample answers".to_owned(),
        }
    }

    fn run(&self, ask: SkillAsk) -> Box<SkillJob> {
        let answer = match self {
            Self::Echo => Some(Ok(ask.text.trim().to_owned())),
            Self::Sample => Some(Ok(sample(&ask.skill).to_owned())),
            Self::Silent => None,
            Self::Failing => Some(Err(Failure::detailed(Why::Offline, "scripted failure"))),
            Self::Nobody => Some(Err(Why::NotHere.into())),
        };
        Box::new(Ready(answer))
    }
}

/// What [`Scripted::Sample`] says for `skill`.
fn sample(skill: &str) -> &'static str {
    match skill {
        "translate" => {
            "Le renard brun et rapide saute par-dessus le chien paresseux, puis \
             disparaît dans la forêt avant que quiconque ne remarque son passage."
        }
        "summarize" => {
            "A quick fox outruns a sleepy dog and slips into the woods unseen. \
             The scene sets up the story's theme: speed and attention win over comfort."
        }
        "simplify" => "A fast fox jumps over a lazy dog. Then it runs into the woods.",
        _ => {
            "The passage contrasts two animals. The fox is quick and alert; the dog \
             is slow and does not notice it.\n\nThe word \"lazy\" is the key: it tells \
             the reader the dog had every chance to act but chose rest. Writers often \
             pair a lively character with a still one so the reader feels the movement \
             more strongly.\n\nTry it: describe your morning with one quick and one \
             slow detail."
        }
    }
}

/// An answer known from the start, or none ever.
struct Ready(Option<Result<String, Failure>>);

impl Pending<String> for Ready {
    fn news(&mut self) -> Vec<News<String>> {
        self.0.take().map(News::Done).into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(text: &str) -> SkillAsk {
        SkillAsk {
            skill: "translate".to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn scripts_echo_wait_or_fail_once() {
        let mut echo = Scripted::Echo.run(ask(" Bonjour "));
        assert_eq!(echo.take(), Some(Ok("Bonjour".to_owned())));
        assert!(echo.take().is_none(), "an answer comes once");
        assert!(Scripted::Silent.run(ask("x")).take().is_none());
        let failed = Scripted::Failing.run(ask("x")).take();
        assert!(matches!(failed, Some(Err(f)) if f.why == Why::Offline));
        let none = Scripted::Nobody.run(ask("x")).take();
        assert!(matches!(none, Some(Err(f)) if f.why == Why::NotHere));
        assert_eq!(Scripted::parse("silent"), Some(Scripted::Silent));
        assert_eq!(Scripted::parse("slow"), None);
        let sample = Scripted::Sample.run(ask("x")).take();
        assert!(matches!(sample, Some(Ok(t)) if t.starts_with("Le renard")));
        assert!(Scripted::Nobody.by("translate").is_empty());
    }
}
