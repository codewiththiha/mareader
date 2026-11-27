//! Temporary edge-hover sidebar behavior.

use super::*;

impl Sidebar {
    fn preview(&mut self, ctx: &Context) {
        self.open = true;
        self.recentre = true;
        self.armed = false;
        self.left_at = None;
        self.edge_at = None;
        ctx.request_repaint();
    }

    /// The edge strip opens a closed rail; leaving an open one closes it.
    pub(super) fn track_hover(
        &mut self,
        ctx: &Context,
        cx: &RailContext,
        rail: Rect,
        screen: Rect,
    ) {
        let down = ctx.input(|i| i.pointer.any_down());
        if !self.open {
            let edge = Rect::from_min_size(screen.min, vec2(EDGE, screen.height()));
            if self.hover_blocked {
                if ctx.pointer_hover_pos().is_none_or(|p| !edge.contains(p)) {
                    self.hover_blocked = false;
                }
                return;
            }
            let blocked = |p| {
                ctx.layer_id_at(p)
                    .is_some_and(|l| z_order::is_above_tier(ctx, l, Tier::WindowChrome))
            };
            let hit = ctx
                .pointer_hover_pos()
                .filter(|&p| !down && edge.contains(p) && !blocked(p));
            match hit {
                Some(p) => {
                    let guarded = p.y < cx.bar_bottom
                        || crate::hover_guard::near(ctx, p, crate::hover_guard::Edge::Sidebar);
                    if crate::hover_guard::rested(ctx, &mut self.edge_at, guarded) {
                        self.preview(ctx);
                    }
                }
                None => self.edge_at = None,
            }
            return;
        }
        let pointer = ctx.pointer_hover_pos();
        // Keyboard focus in it holds it, as the pointer does.
        if crate::focus::within(ctx, rail) {
            self.left_at = None;
            return;
        }
        if pointer.is_some_and(|p| rail.contains(p)) {
            self.armed = true;
            self.left_at = None;
            return;
        }
        // Opened from a key or button: wait until the pointer has visited.
        if !self.armed || down || egui::Popup::is_any_open(ctx) {
            return;
        }
        let now = ctx.input(|i| i.time);
        let waited = now - *self.left_at.get_or_insert(now);
        if waited >= HOVER_GRACE {
            self.open = false;
            self.left_at = None;
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(HOVER_GRACE - waited));
        }
    }
}
