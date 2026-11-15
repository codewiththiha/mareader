use super::*;

#[test]
fn a_table_laying_out_pictures_reads_as_blocks() {
    let mut shelf = Shelf::default();
    for name in ["c1", "c2"] {
        if let Some(p) = png(4, 4) {
            shelf
                .pictures
                .insert(format!("/OEBPS/images/{name}.png"), p);
        }
    }
    let b = read_with(
        &mut shelf,
        "<h2>Main</h2><table class='grid'><tr>\
             <td style='text-align:center'><img src='../images/c1.png' alt='char1'/>\
             <p>Sistine</p><p>The honour student</p></td>\
             <td class='gap'>&#160;</td>\
             <td><div><img src='../images/c2.png' alt='char2'/></div><p>Glenn</p>\
             <table><tr><th>Age</th><td>19</td></tr></table></td>\
             </tr></table><p>after</p>",
    );
    assert_eq!(
        kinds(&b),
        [
            "text", "image", "text", "text", "image", "text", "table", "text"
        ]
    );
    let words = texts(&b);
    assert!(!words.iter().any(|t| t.contains("char")), "{words:?}");
    assert_eq!(b.get(1).map(|b| b.frame.align), Some(Align::Center));
    // Unaligned, a grid's picture is centred too.
    assert_eq!(b.get(4).map(|b| b.frame.align), Some(Align::Center));
}

#[test]
fn presentation_tables_read_as_blocks_and_spacer_columns_go() {
    let b = read(
        "<table role='presentation'><tr><td>one</td><td>two</td></tr></table>\
             <table><tr><th>Word</th><th></th><th>Means</th></tr>\
             <tr><td>tea</td><td> </td><td>a drink</td></tr></table>",
    );
    assert_eq!(kinds(&b), ["text", "text", "table"]);
    let cells: Vec<Vec<&str>> = match b.get(2).map(|b| &b.content) {
        Some(Content::Table(t)) => t
            .rows
            .iter()
            .map(|r| r.cells.iter().map(|c| c.text.as_str()).collect())
            .collect(),
        _ => Vec::new(),
    };
    assert_eq!(cells, [vec!["Word", "Means"], vec!["tea", "a drink"]]);
}

#[test]
fn a_grid_of_text_with_spacers_reads_as_blocks_but_data_stays() {
    let b = read(
        "<table><tr><td><div>Wendy</div><p>A noble</p></td><td> </td>\
             <td><div>Lynn</div><p>Shy</p></td></tr></table>\
             <table><tr><td><p>tea</p></td><td><p>3</p></td></tr>\
             <tr><td><p>rice</p></td><td><p>5</p></td></tr></table>",
    );
    assert_eq!(kinds(&b), ["text", "text", "text", "text", "table"]);
    assert_eq!(texts(&b), ["Wendy", "A noble", "Lynn", "Shy"]);
}

#[test]
fn boxes_keep_their_border_width_and_place() {
    let b = read(
        "<div style=\"border: 3px double; width: 10em; margin: 0 auto\">\
             <p style=\"margin: 0\">one</p><p style=\"margin: 0\">two</p></div>\
             <p>after</p>",
    );
    assert_eq!(texts(&b), ["one", "two", "after"]);
    let boxes = |i: usize| b.get(i).map_or(&[][..], |b| b.frame.boxes.as_slice());
    let (one, two) = (boxes(0).first(), boxes(1).first());
    assert!(one.zip(two).is_some_and(|(a, b)| Arc::ptr_eq(a, b)));
    assert!(boxes(2).is_empty());
    let spec = one.map(|b| (**b).clone()).unwrap_or_default();
    assert_eq!(spec.edges, [Edge::Double; 4]);
    assert_eq!(
        (spec.width, spec.margin),
        (Some(Length::Em(10.0)), [None, None])
    );
    // Its inside top and bottom: the border, the paragraphs flush.
    let border = 3.0 / ROOT_PX;
    assert!((spec.top - border).abs() < 1e-3 && (spec.bottom - border).abs() < 1e-3);
}

#[test]
fn an_empty_bordered_box_is_a_rule_of_its_own() {
    let b = read(
        "<p>a</p><p style=\"border-top: 2px solid; width: 14em; margin: 0 auto\"></p>\
             <p>b</p>",
    );
    assert_eq!(kinds(&b), ["text", "space", "text"]);
    let rule = b.get(1);
    assert!(rule.is_some_and(|r| matches!(r.content, Content::Space(h) if h > 0.1)));
    let edges = rule.and_then(|r| r.frame.boxes.first()).map(|b| b.edges);
    assert_eq!(
        edges,
        Some([Edge::Solid, Edge::None, Edge::None, Edge::None])
    );
}

#[test]
fn a_layout_grids_cells_share_their_row() {
    let b = read(
        "<table role=\"presentation\" style=\"width: 22em\"><tr>\
             <td style=\"width: 10em; vertical-align: top\"><p>a</p></td>\
             <td style=\"width: 2em\"></td><td style=\"width: 10em\"><p>b</p><p>c</p></td>\
             </tr><tr><td><p>d</p></td></tr></table><p>after</p>",
    );
    assert_eq!(texts(&b), ["a", "b", "c", "d", "after"]);
    let cell = |i: usize| b.get(i).and_then(|b| b.frame.cell.clone());
    let columns: Vec<_> = (0..5).map(|i| cell(i).map(|c| c.column)).collect();
    assert_eq!(columns, [Some(0), Some(2), Some(2), Some(0), None]);
    let row = cell(0).map(|c| c.row);
    let same = |i: usize| {
        cell(i)
            .zip(row.as_ref())
            .is_some_and(|(c, r)| Arc::ptr_eq(&c.row, r))
    };
    assert!(same(1) && same(2) && !same(3));
    let row = row.map(|r| (*r).clone()).unwrap_or_default();
    assert_eq!(row.widths.len(), 3);
    assert_eq!(row.widths.first(), Some(&Some(Length::Em(10.0))));
    assert_eq!(row.valign.first(), Some(&VAlign::Top));
    // The table's box holds the grid, outside each cell.
    assert!(cell(0).is_some_and(|c| c.depth == 1));
    let table = b.first().and_then(|b| b.frame.boxes.first());
    assert!(table.is_some_and(|t| t.width == Some(Length::Em(22.0))));
}

#[test]
fn flex_rows_read_their_items_as_cells() {
    let b = read(
        "<div style='display: flex; justify-content: center; gap: 16px; flex-wrap: wrap'>\
             <div style='flex: 1'><p>a</p></div>\
             <div style='flex-basis: 5em; align-self: flex-end'><p>b</p></div></div>\
             <p>after</p>",
    );
    assert_eq!(texts(&b), ["a", "b", "after"]);
    let (Some((row, 0)), Some((same, 1)), None) = (cell_of(&b, 0), cell_of(&b, 1), cell_of(&b, 2))
    else {
        return assert_eq!(b.len(), 0, "cells");
    };
    assert!(Arc::ptr_eq(&row, &same));
    let flex = Flex {
        justify: Justify::Center,
        gap: 1.0,
        wrap: true,
    };
    assert_eq!(row.columns.flex, Some(flex));
    assert_eq!(row.grow, [1.0, 0.0]);
    assert_eq!(row.widths, [None, Some(Length::Em(5.0))]);
    assert_eq!(row.valign, [VAlign::Top, VAlign::Bottom]);
}

#[test]
fn a_flex_row_of_one_lets_its_layout_table_be_the_grid() {
    let b = read(
        "<div style='display: flex; justify-content: center'><div>\
             <table role='presentation'><tr><td><p>a</p></td><td><p>b</p></td></tr>\
             </table></div></div>",
    );
    let (Some((row, 0)), Some((_, 1))) = (cell_of(&b, 0), cell_of(&b, 1)) else {
        return assert_eq!(b.len(), 0, "cells");
    };
    assert_eq!(row.columns.flex, None);
}

#[test]
fn cells_span_columns_and_rows() {
    let b = read(
        "<table role='presentation' style='width: 20em'>\
             <tr><td colspan='2'><p>a</p></td></tr>\
             <tr><td rowspan='2'><p>b</p></td><td><p>c</p></td></tr>\
             <tr><td><p>d</p></td></tr></table>",
    );
    assert_eq!(texts(&b), ["a", "b", "c", "d"]);
    let row = |i: usize| cell_of(&b, i).map(|(r, _)| r).unwrap_or_default();
    let (a, b2, d) = (row(0), row(1), row(3));
    assert_eq!((a.starts.clone(), a.spans.clone()), (vec![0], vec![2]));
    assert_eq!(
        (b2.starts.clone(), b2.spans.clone()),
        (vec![0, 1], vec![1, 1])
    );
    assert_eq!(d.starts, [1]);
    assert!(a.columns.fill && Arc::ptr_eq(&a.columns, &d.columns));
}

#[test]
fn boxes_turn_and_round_and_small_pictures_join_their_line() {
    let mut shelf = Shelf::default();
    if let Some(p) = png(16, 16) {
        shelf.pictures.insert("/OEBPS/images/s.png".into(), p);
    }
    let b = read_with(
        &mut shelf,
        "<div style='transform: rotate(0.25turn); border: 1px solid; \
             border-radius: 8px'><p>t</p></div>\
             <p>x <img src='../images/s.png'/> y</p>",
    );
    assert_eq!(kinds(&b), ["text", "text"]);
    let first = b.first().map(|b| &b.frame);
    assert_eq!(first.map(|f| f.rotate), Some(90.0));
    let radius = first.and_then(|f| f.boxes.first()).and_then(|b| b.radius);
    assert_eq!(radius, Some(Length::Em(0.5)));
    let inlines = b
        .get(1)
        .and_then(text)
        .map(|t| t.inlines.clone())
        .unwrap_or_default();
    assert_eq!(inlines.len(), 1);
    assert!(
        inlines
            .first()
            .is_some_and(|i| i.at <= 2 && (i.width - 1.0).abs() < 0.01)
    );
}

#[test]
fn a_column_flex_lays_each_item_as_its_own_row() {
    let mut shelf = Shelf::default();
    if let Some(p) = png(400, 300) {
        shelf.pictures.insert("/OEBPS/images/a.png".into(), p);
    }
    let b = read_with(
        &mut shelf,
        "<div style='display: flex; flex-direction: column; align-items: center; \
             gap: 2em 1em'><div><p>One</p></div><div style='align-self: stretch'>\
             <p>Two</p></div><div style='width: 5em'><div style='display: flex'>\
             <p>A</p><p>B</p></div></div><img src='../images/a.png'/></div>",
    );
    let texts: Vec<&str> = b.iter().filter_map(text).map(|t| t.text.trim()).collect();
    let row = |i: usize| b.get(i).and_then(|b| b.frame.cell.as_ref()).map(|c| &c.row);
    let justify = |i| row(i).and_then(|r| r.columns.flex).map(|f| f.justify);
    assert_eq!(texts, ["One", "Two", "A", "B"]);
    assert_eq!(justify(0), Some(Justify::Center));
    // Stretched, the second reads as prose, two ems below the first.
    assert!(row(1).is_none(), "{:?}", b.get(1));
    assert!(
        b.get(1).is_some_and(|b| b.frame.before >= 2.0),
        "{:?}",
        b.get(1)
    );
    // The row inside an item lets the item go and lays out itself.
    assert_eq!(row(2).map(|r| r.widths.len()), Some(2));
    assert!(row(2).zip(row(3)).is_some_and(|(a, b)| Arc::ptr_eq(a, b)));
    assert_eq!(b.get(4).map(|b| b.frame.align), Some(Align::Center));
    assert!(b.get(4).is_some_and(|b| b.frame.cell.is_none()));
}

#[test]
fn a_full_height_flex_box_holds_its_row() {
    let b = read(
        "<div style='display: flex; height: 94vh; align-items: center'>\
             <div><p>Key</p></div></div>",
    );
    let frame = b.first().map(|b| &b.frame);
    let centre = frame.and_then(|f| f.boxes.first()).and_then(|b| b.centre);
    assert_eq!(centre, Some(0.94), "{frame:?}");
    let held = frame.and_then(|f| f.boxes.first()).and_then(|b| b.height);
    assert_eq!(held, None, "the page's share, not a height");
    assert_eq!(
        frame.and_then(|f| f.cell.as_ref()).map(|c| c.depth),
        Some(1)
    );
    assert!(
        frame
            .and_then(|f| f.look.as_ref())
            .is_some_and(|l| l.designed)
    );
}
