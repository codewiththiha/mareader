use super::*;

const LETTER: PageSize = PageSize {
    width: 612.0,
    height: 792.0,
};

fn column(zoom: f32) -> Layout {
    let inputs = Inputs {
        mode: ViewMode::Vertical,
        frame: vec2(644.0, 800.0),
        inset: 0.0,
        zoom,
        focus: 0,
        gap: GAP,
        margin: MARGIN,
        align: egui::Align::Center,
        rtl: false,
        lead: 0,
    };
    Layout::new(&[LETTER; 20], inputs)
}

#[test]
fn zooming_holds_the_spot_under_the_pivot() {
    let (old, new) = (column(1.0), column(2.0));
    let (offset, viewport) = (vec2(0.0, 5_000.0), vec2(644.0, 800.0));
    let centre = viewport / 2.0;
    let before = old.locate(Pos2::ZERO + offset + centre).unwrap_or_default();
    let to = hold(&old, &new, offset, (centre, centre)).unwrap_or_default();
    let after = new.locate(Pos2::ZERO + to + centre).unwrap_or_default();
    assert_eq!(after.0, before.0);
    assert!(
        (after.1 - before.1).length() < 1e-3,
        "{after:?} vs {before:?}"
    );
}

#[test]
fn a_resized_pane_resets_its_book_once_it_holds_still() -> Result<(), &'static str> {
    let mut view = PdfView::new();
    // The first page size sets at once.
    assert_eq!(view.settle_sheet(Some((400.0, 600.0)), 0.0), None);
    assert!(std::mem::take(&mut view.sheet_moved));
    // A resize waits for the pane to hold still.
    let wider = Some((500.0, 600.0));
    assert!(view.settle_sheet(wider, 1.0).is_some());
    assert!(view.settle_sheet(wider, 1.2).is_some());
    assert_eq!(view.sheet(), Some((400.0, 600.0)));
    assert_eq!(view.settle_sheet(wider, 1.31), None);
    assert_eq!(view.sheet(), wider);
    assert!(std::mem::take(&mut view.sheet_moved));
    // A strip goes back to the card at once.
    assert_eq!(view.settle_sheet(None, 1.4), None);
    assert_eq!((view.sheet(), view.sheet_moved), (None, true));
    Ok(())
}

#[test]
fn a_mode_switch_keeps_the_page_and_a_turn_moves_it() -> Result<(), &'static str> {
    let doc = crate::testing::doc()?;
    let mut view = PdfView::new();
    let mut at = Place::new(doc);
    at.current = 7;
    view.set_mode(&mut at, ViewMode::Spread, 0.0);
    assert_eq!((view.focus, at.current, view.reveal), (6, 7, Some(6)));
    assert_eq!(view.zoom.fit, Some(Fit::Width));
    view.arrow(&mut at, Arrow::Right, 20, 0.0);
    assert_eq!((view.focus, at.current), (8, 8));
    view.set_mode(&mut at, ViewMode::Horizontal, 0.0);
    assert_eq!((view.zoom.fit, view.reveal), (None, Some(8)));
    // Along the strip, the arrow is the scroll's, not a turn.
    view.arrow(&mut at, Arrow::Right, 20, 0.0);
    assert_eq!(at.current, 8);
    view.arrow(&mut at, Arrow::Up, 20, 0.0);
    assert_eq!(at.current, 8);
    // Across the column, left and right still turn.
    view.set_mode(&mut at, ViewMode::Vertical, 0.0);
    view.arrow(&mut at, Arrow::Left, 20, 0.0);
    assert_eq!((at.current, view.reveal), (7, Some(7)));
    Ok(())
}
