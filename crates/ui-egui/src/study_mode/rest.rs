//! The line under an open deck's header while its rest is written.
//!
//! An AI deck opens half written. While the rest comes, a muted line counts
//! it; if it fails, the line says why and offers another try, the
//! dictionary for the rest, or going on with the cards there are.

use egui::{Label, RichText, Ui};
use mareader_engine::study_mode::{Action, Open, Rest};
use mareader_workspace::PaneId;

use super::controls::{self, Chip, Look};
use super::deck::send;
use super::waiting::RETRY;
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::Commands;

/// The buttons' labels, for screen readers and tests.
pub const FINISH_WITH_DICTIONARY: &str = "Finish with the dictionary";
pub const SKIP_REST: &str = "Skip the rest";

/// What the rest is doing, in words; none once it is in.
pub fn said(open: &Open) -> Option<String> {
    match &open.rest {
        Rest::Done => None,
        Rest::Writing(job) => Some(match job.progress() {
            Some((done, of)) => format!("Writing the rest: {done} of {of}…"),
            None => "Writing the rest…".to_owned(),
        }),
        Rest::Failed { why, .. } => Some(format!("The rest couldn’t be written. {why}")),
    }
}

/// The rest's line, and its buttons once it failed.
pub fn show(ui: &mut Ui, pane: PaneId, open: &Open, out: &mut Commands) {
    let Some(said) = said(open) else {
        return;
    };
    let t = Tokens::get(ui.ctx());
    let failed = matches!(open.rest, Rest::Failed { .. });
    let ink = if failed { t.danger } else { t.muted };
    let line = RichText::new(said).size(t.text_small).color(ink);
    ui.add(Label::new(line).wrap());
    if !failed {
        ui.add_space(4.0);
        return;
    }
    let chip = |label, look, icon| Chip::new(label, look).icon(icon).tall(30.0);
    let chips = vec![
        chip(RETRY, Look::Outline, Icon::Rescan),
        chip(FINISH_WITH_DICTIONARY, Look::Outline, Icon::Book),
        Chip::new(SKIP_REST, Look::Quiet).tall(30.0),
    ];
    let actions = [Action::Retry, Action::UseDictionary, Action::SkipRest];
    let pressed = controls::row(ui, chips, 8.0);
    for (button, action) in pressed.iter().zip(actions) {
        if button.clicked() {
            send(out, action(pane));
        }
    }
    ui.add_space(6.0);
}
