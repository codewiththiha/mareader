use super::*;

mod appearance;
mod library;
mod page_ai;
mod panes;
mod read_aloud;
mod study;

type Handles = fn(&Command) -> bool;
type Apply = fn(&mut MareaderApp, &Context, Command);

fn one_stage(
    app: &mut MareaderApp,
    ctx: &Context,
    command: Command,
    handles: Handles,
    apply: Apply,
) {
    if handles(&command) {
        apply(app, ctx, command);
    }
}

fn two_stage(
    app: &mut MareaderApp,
    ctx: &Context,
    command: Command,
    handles_1: Handles,
    apply_1: Apply,
    handles_2: Handles,
    apply_2: Apply,
) {
    if handles_1(&command) {
        apply_1(app, ctx, command);
        return;
    }
    if handles_2(&command) {
        apply_2(app, ctx, command);
    }
}

impl MareaderApp {
    pub(super) fn apply(&mut self, ctx: &Context, command: Command) {
        if !self.route().allows(&command) {
            return;
        }
        // Text edits must reach the pane focused when they were made.
        self.reconcile_texts();
        if panes::handles(&command) {
            panes::apply(self, ctx, command);
            return;
        }
        if appearance::handles(&command) {
            appearance::apply(self, ctx, command);
            return;
        }
        if study::handles(&command) {
            study::apply(self, ctx, command);
            return;
        }
        if page_ai::handles(&command) {
            page_ai::apply(self, ctx, command);
            return;
        }
        if read_aloud::handles(&command) {
            read_aloud::apply(self, ctx, command);
            return;
        }
        if library::handles(&command) {
            library::apply(self, ctx, command);
        }
    }
}
