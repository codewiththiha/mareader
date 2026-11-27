//! A rest at an edge near another control prevents accidental reveals.

use egui::{Context, Id, Pos2, Rect};

const DELAY: f64 = 0.5;
const CLEARANCE: f32 = 12.0;

/// The edge a floating control needs protected.
#[derive(Clone, Copy)]
pub enum Edge {
    Sidebar,
    PageBar,
}

fn key() -> Id {
    Id::new("edge-hover-guards")
}

/// Keep a control's bounds for this and the next pass, before edge tracking.
pub fn protect(ctx: &Context, rect: Rect, sidebar: bool, page_bar: bool) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let all = d.get_temp_mut_or_default::<Vec<(u64, Rect, bool, bool)>>(key());
        all.retain(|(at, _, _, _)| at + 1 >= pass);
        all.push((pass, rect, sidebar, page_bar));
    });
}

/// Whether a point on an edge lies in a protected control's column or row.
pub fn near(ctx: &Context, p: Pos2, edge: Edge) -> bool {
    let pass = ctx.cumulative_pass_nr();
    let all: Vec<(u64, Rect, bool, bool)> = ctx.data(|d| d.get_temp(key())).unwrap_or_default();
    all.into_iter().any(|(at, r, sidebar, page_bar)| {
        at + 1 >= pass
            && match edge {
                Edge::Sidebar => {
                    sidebar
                        && r.y_range().expand(CLEARANCE).contains(p.y)
                        && p.x <= r.right()
                        && p.x >= r.left() - CLEARANCE * 2.0
                }
                Edge::PageBar => {
                    page_bar
                        && r.x_range().expand(CLEARANCE).contains(p.x)
                        && p.y >= r.top()
                        && p.y - r.bottom() < 36.0
                }
            }
    })
}

/// Wait only in a protected area; moving away resets the clock.
pub fn rested(ctx: &Context, since: &mut Option<f64>, guarded: bool) -> bool {
    if !guarded {
        *since = None;
        return true;
    }
    let now = ctx.input(|i| i.time);
    let waited = now - *since.get_or_insert(now);
    if waited < DELAY {
        ctx.request_repaint_after_secs((DELAY - waited) as f32);
    }
    waited >= DELAY
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    #[test]
    fn controls_can_protect_one_or_both_edges() {
        let ctx = Context::default();
        let rect = Rect::from_min_size(pos2(12.0, 740.0), vec2(36.0, 36.0));
        protect(&ctx, rect, true, true);
        assert!(near(&ctx, pos2(2.0, 755.0), Edge::Sidebar));
        assert!(near(&ctx, pos2(28.0, 799.0), Edge::PageBar));
        assert!(!near(&ctx, pos2(100.0, 799.0), Edge::PageBar));
        let other = Context::default();
        protect(&other, rect, true, false);
        assert!(near(&other, pos2(2.0, 755.0), Edge::Sidebar));
        assert!(!near(&other, pos2(28.0, 799.0), Edge::PageBar));
    }
}
