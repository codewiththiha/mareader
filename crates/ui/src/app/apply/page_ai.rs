use super::*;

pub(super) fn handles(command: &Command) -> bool {
    match command {
        #[cfg(not(target_arch = "wasm32"))]
        Command::Ai(..) => true,
        Command::PageAi(..) => true,
        _ => false,
    }
}

pub(super) fn apply(app: &mut MareaderApp, ctx: &Context, command: Command) {
    super::one_stage(app, ctx, command, handles_1, apply_1);
}

fn handles_1(command: &Command) -> bool {
    match command {
        #[cfg(not(target_arch = "wasm32"))]
        Command::Ai(..) => true,
        Command::PageAi(..) => true,
        _ => false,
    }
}

fn apply_1(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        Command::PageAi(action) => app.page_ai_act(ctx, action),
        #[cfg(not(target_arch = "wasm32"))]
        Command::Ai(job) => app.ai.send(job),
        _ => {}
    }
}
