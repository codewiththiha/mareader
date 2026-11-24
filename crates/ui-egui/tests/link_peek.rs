//! Peeking at where a page link goes, on the sample PDF.

use egui_kittest::kittest::Queryable as _;
use mareader_pdf::LinkTarget;

use crate::selection::{H, Out, page_one, reader};

#[test]
fn resting_on_a_page_link_peeks_at_its_page_before_any_jump() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    let chapter = layer
        .links
        .iter()
        .find(|l| matches!(l.target, LinkTarget::Page(5)))
        .ok_or("the sample page links to page 6")?;
    let pane = h.state().active_pane();
    let peeks = |h: &H| h.query_all_by_label("Page 6").count();
    let before = peeks(&h);
    let on = sheet.to_screen(chapter.bounds).center();
    h.hover_at(on);
    h.run_steps(4);
    assert_eq!(peeks(&h), before, "a passing pointer opens no peek");
    // Past the rest the peek shows; the page stays where it was.
    h.run_steps(40);
    assert_eq!(peeks(&h), before + 1);
    assert_eq!(h.state().documents().current(pane), Some(0));
    h.hover_at(on + egui::vec2(0.0, 200.0));
    h.run_steps(10);
    assert_eq!(peeks(&h), before, "leaving the link closes it");
    Ok(())
}
