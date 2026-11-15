//! What sits behind a bar: blurred pages, a tint, or a solid fill.

use egui::{CornerRadius, Rect, Ui};

use crate::gpu::{Glass, Look};
use crate::theme::Tokens;

/// One frame's backdrop, shared by the title bar and the page bars.
#[derive(Clone, Debug)]
pub enum Backdrop {
    /// The pages behind, blurred, under a frosted wash.
    Glass(Glass),
    /// No GPU to blur with: a nearly opaque surface.
    Tint,
    /// Glass is turned off in settings: the plain surface.
    Solid,
}

impl Backdrop {
    /// Fills `rect` behind a bar; `look` places and fades its glass.
    pub fn paint(&self, ui: &Ui, rect: Rect, look: Look) {
        let t = Tokens::get(ui.ctx());
        let fill = match self {
            Self::Glass(glass) => {
                let screen = ui.ctx().content_rect();
                ui.painter().add(glass.shape(rect, screen, look));
                t.frost
            }
            Self::Tint => t.glass,
            Self::Solid => t.surface,
        };
        ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
    }
}
