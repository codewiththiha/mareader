//! The questions a deck asks before a chance goes: reading now with time
//! left, which closes the cards for good, and turning study mode off.

use egui::Context;
use mareader_engine::study_mode::Action;
use mareader_workspace::PaneId;

use super::deck::{clock, send};
use crate::modal::{self, Dialog};
use crate::theme::Tokens;
use crate::ui_command::Commands;

/// The buttons' labels, for screen readers and tests.
pub const READ_NOW: &str = "Yes, read now";
pub const TURN_OFF: &str = "Turn off";
pub const KEEP: &str = "Keep studying";
/// What turning off gives up.
const OFF_COST: &str = "The cards and the lock end here. The page menu turns it on again.";

/// What a pane's deck asks before it acts.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// Read now, with `left` seconds of the deck's time unspent.
    ReadNow {
        range: String,
        left: f64,
    },
    TurnOff,
}

impl Ask {
    /// The question, what it costs, the yes, and what the yes sends.
    fn words(&self, pane: PaneId) -> (String, String, &'static str, Action) {
        match self {
            Self::ReadNow { range, left } => (
                "Read now and close these cards?".to_owned(),
                format!(
                    "{} is left for {range}. Once closed, these cards won't open again.",
                    clock(*left)
                ),
                READ_NOW,
                Action::Start(pane),
            ),
            Self::TurnOff => (
                "Turn off study mode?".to_owned(),
                OFF_COST.to_owned(),
                TURN_OFF,
                Action::Off(pane),
            ),
        }
    }
}

/// Asks `ask` over the window; false once it is answered or dismissed.
pub fn show(ctx: &Context, pane: PaneId, ask: &Ask, out: &mut Commands) -> bool {
    let t = Tokens::get(ctx);
    let (title, cost, yes, action) = ask.words(pane);
    let fill = match ask {
        Ask::ReadNow { .. } => t.accent,
        Ask::TurnOff => t.danger,
    };
    let mut open = true;
    let id = egui::Id::new("study-confirm").with(pane);
    let shown = Dialog::new(id).show(ctx, |ui| {
        ui.set_width(360.0);
        modal::heading(ui, None, &title, Some(cost.as_str()), &t);
        ui.add_space(14.0);
        modal::buttons(ui, |ui| {
            if modal::primary(ui, yes, fill, &t).clicked() {
                send(out, action);
                open = false;
            }
            if modal::quiet(ui, KEEP, &t).clicked() {
                open = false;
            }
        });
    });
    open && !shown.should_close()
}
