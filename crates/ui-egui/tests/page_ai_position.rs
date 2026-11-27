//! Page-action submenus keep outside their parent when either side fits.

use super::*;

#[test]
fn page_submenus_use_an_available_side_even_near_the_bottom() -> Out {
    let mut saw_left = false;
    let mut saw_right = false;
    let mut saw_overlap = false;
    for width in [1000.0, 600.0, 320.0] {
        for (row, switch) in [
            ("Translate page…", "Translate continuously"),
            ("Simplify page", "Simplify continuously"),
        ] {
            let positions: &[(f32, f32)] = if width == 320.0 {
                &[(0.03, 0.05)]
            } else {
                &[(0.03, 0.05), (0.97, 0.95), (0.97, 0.50)]
            };
            for &(x, y) in positions {
                let mut h = reader(&[("pdf", "sample:5"), ("provider", "ollama")]);
                h.set_size(egui::vec2(width, 500.0));
                h.run_steps(10);
                let (_, sheet) = page(&h, 0)?;
                let shown = sheet.shown.intersect(h.ctx.content_rect());
                let at = egui::pos2(
                    egui::lerp(shown.x_range(), x),
                    egui::lerp(shown.y_range(), y),
                );
                press(&mut h, at, PointerButton::Secondary);
                assert!(h.query_by_label(row).is_some(), "page menu for {at:?}");
                h.get_by_label(row).hover();
                h.run_steps(12);
                let parent = h.state().page_ai().menu_rect().ok_or("missing parent")?;
                let point = h.get_by_label(switch).rect().center();
                let layer = h.ctx.layer_id_at(point).ok_or("missing popup layer")?;
                let child = h
                    .ctx
                    .memory(|m| m.area_rect(layer.id))
                    .ok_or("missing popup")?;
                let room = h.ctx.content_rect();
                let left = parent.left() - room.left();
                let right = room.right() - parent.right();
                let needed = child.width() + 4.0;
                if right >= needed - 0.5 {
                    saw_right = true;
                    assert!(
                        child.left() >= parent.right() - 0.5,
                        "right fits: {parent:?} {child:?}"
                    );
                } else if left >= needed - 0.5 {
                    saw_left = true;
                    assert!(
                        child.right() <= parent.left() + 0.5,
                        "left fits: {parent:?} {child:?}"
                    );
                } else {
                    saw_overlap = true;
                }
                h.hover_at(point);
                h.run_steps(25);
                assert!(
                    h.query_by_label(switch).is_some(),
                    "submenu stays open on entry"
                );
                h.get_by_label(switch).click();
                h.run();
                let tasks = &h.state().settings().ai.tasks;
                let actual = if row.starts_with("Translate") {
                    tasks.translate.start
                } else {
                    tasks.simplify.start
                };
                assert_eq!(actual, PageStart::Continuous, "switch remains clickable");
                assert!(
                    h.query_by_label(row).is_some(),
                    "switch does not close menu"
                );
            }
        }
    }
    assert!(
        saw_left && saw_right && saw_overlap,
        "left={saw_left}, right={saw_right}, overlap={saw_overlap}"
    );
    Ok(())
}
