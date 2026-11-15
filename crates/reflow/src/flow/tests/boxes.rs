use super::*;

#[test]
fn a_book_box_centres_its_width_and_draws_its_border() {
    let b = Arc::new(Boxed {
        width: Some(Length::Em(10.0)),
        border: [0.2; 4],
        edges: [Edge::Double; 4],
        ..Boxed::default()
    });
    let boxed = || designed(para(3), &[Arc::clone(&b)], None);
    let f = flow(&[boxed(), boxed(), designed(para(3), &[], None)]);
    let em = design_em(&f);
    let set = f.blocks.first();
    let left = (f.width - 10.4 * em) / 2.0 + 0.2 * em;
    assert!(set.is_some_and(|s| (s.x - left).abs() < 0.5 && (s.width - 10.0 * em).abs() < 0.5));
    let decos = f.decos.first().map_or(&[][..], Vec::as_slice);
    assert_eq!(decos.len(), 1);
    let (top, _) = span(&f, 0, 0);
    let (_, bottom) = span(&f, 0, 1);
    let (after, _) = span(&f, 0, 2);
    let deco = decos.first().map_or([0.0; 4], |d| d.rect);
    assert!(deco[1] <= top && deco[1] + deco[3] >= bottom, "{deco:?}");
    assert!(deco[1] + deco[3] <= after, "{deco:?} {after}");
}

#[test]
fn a_full_height_box_centres_its_content_down_the_page() {
    // Held a page tall as well, it still centres.
    let tall = Arc::new(Boxed {
        centre: Some(0.94),
        height: Some(80.0),
        ..Boxed::default()
    });
    let flex = Arc::new(Columns {
        flex: Some(Flex::default()),
        ..Columns::default()
    });
    let row = row_of(&flex, vec![None], vec![0]);
    let cell = |row: &Arc<GridRow>| {
        let mut c = at(row, 0);
        if let Some(c) = c.as_mut() {
            c.depth = 1;
        }
        c
    };
    for (name, cell) in [("plain", None), ("cell", cell(&row))] {
        let blocks = [
            designed(para(3), &[Arc::clone(&tall)], cell.clone()),
            designed(para(3), &[Arc::clone(&tall)], cell),
        ];
        let f = flow(&blocks);
        let (top, _) = span(&f, 0, 0);
        assert!(top > 300.0, "{name}: {top}");
    }
}

#[test]
fn a_grid_rows_cells_sit_side_by_side() {
    let widths = [10.0, 2.0, 10.0].map(|w| Some(Length::Em(w))).to_vec();
    let row = Arc::new(GridRow {
        widths,
        valign: vec![VAlign::Top; 3],
        ..GridRow::default()
    });
    let cell = |column| {
        let row = Arc::clone(&row);
        Some(Cell {
            row,
            column,
            depth: 0,
        })
    };
    let f = flow(&[
        designed(para(5), &[], cell(0)),
        designed(para(5), &[], cell(2)),
        designed(para(5), &[], cell(2)),
        designed(para(3), &[], None),
    ]);
    let x = |i: usize| f.blocks.get(i).map_or((0.0, 0.0), |s| (s.x, s.width));
    assert!(
        x(1).0 >= x(0).0 + x(0).1 && x(1) == x(2),
        "{:?} {:?}",
        x(0),
        x(1)
    );
    let (a, b, c, d) = (
        span(&f, 0, 0),
        span(&f, 0, 1),
        span(&f, 0, 2),
        span(&f, 0, 3),
    );
    assert!((a.0 - b.0).abs() < 0.01, "{a:?} {b:?}");
    assert!(c.0 >= b.1 && d.0 >= c.1.max(a.1), "{c:?} {d:?}");
}

#[test]
fn table_columns_take_their_contents_width() {
    let columns = Arc::new(Columns::default());
    let row = row_of(&columns, vec![None, None], vec![0, 1]);
    let f = flow(&[
        words("ab", at(&row, 0)),
        designed(para(60), &[], at(&row, 1)),
    ]);
    let ((x0, w0), (x1, w1)) = (across(&f, 0), across(&f, 1));
    assert!(w0 < w1 && w0 < f.width / 4.0, "{w0} {w1}");
    assert!(x1 >= x0 + w0 - 0.01, "{x0} {w0} {x1}");
}

#[test]
fn a_cells_boxes_stay_inside_it() {
    let columns = Arc::new(Columns::default());
    let fixed = Some(Length::Em(5.0));
    let row = row_of(&columns, vec![fixed, fixed], vec![0, 1]);
    let wide = Arc::new(Boxed {
        margin: [Some(Length::Em(-2.0)); 2],
        ..Boxed::default()
    });
    let f = flow(&[
        designed(para(9), &[wide], at(&row, 0)),
        words("next", at(&row, 1)),
    ]);
    let ((x0, w0), (x1, _)) = (across(&f, 0), across(&f, 1));
    assert!(x0 + w0 <= x1 + 0.01, "{x0} {w0} {x1}");
}

#[test]
fn a_cell_spanning_rows_down_sits_across_them() {
    let columns = Arc::new(Columns::default());
    let fixed = Some(Length::Em(8.0));
    let row = |down: Vec<u16>, starts: Vec<u16>| {
        Arc::new(GridRow {
            widths: vec![fixed; starts.len()],
            valign: vec![VAlign::Middle; starts.len()],
            spans: vec![1; starts.len()],
            starts,
            down,
            columns: Arc::clone(&columns),
            ..GridRow::default()
        })
    };
    let (first, second, third) = (
        row(vec![3, 1], vec![0, 1]),
        row(vec![], vec![1]),
        row(vec![], vec![1]),
    );
    let f = flow(&[
        words("tall", at(&first, 0)),
        words("one", at(&first, 1)),
        words("two", at(&second, 0)),
        words("three", at(&third, 0)),
        words("after", None),
    ]);
    let [tall, one, two, three, after] = [0, 1, 2, 3, 4].map(|i| span(&f, 0, i));
    let line = one.1 - one.0;
    // Its own row is no taller; it sits mid-way down all three.
    assert!((two.0 - one.1).abs() < line / 2.0, "{one:?} {two:?}");
    let (middle, rows) = ((tall.0 + tall.1) / 2.0, (one.0 + three.1) / 2.0);
    assert!((middle - rows).abs() < 1.0, "{tall:?} {one:?} {three:?}");
    assert!(after.0 >= three.1 - 0.01, "{three:?} {after:?}");
    // Taller than its rows, the last of them grows to hold it.
    let first = row(vec![2, 1], vec![0, 1]);
    let second = row(vec![], vec![1]);
    let f = flow(&[
        designed(para(60), &[], at(&first, 0)),
        words("one", at(&first, 1)),
        words("two", at(&second, 0)),
        words("after", None),
    ]);
    let [tall, one, two, after] = [0, 1, 2, 3].map(|i| span(&f, 0, i));
    assert!(
        two.0 < tall.1 && (two.0 - one.1).abs() < line / 2.0,
        "{tall:?} {two:?}"
    );
    assert!(after.0 >= tall.1 - 0.01, "{tall:?} {after:?}");
}

#[test]
fn a_cell_spans_the_columns_under_it() {
    let columns = Arc::new(Columns::default());
    let wide = Arc::new(GridRow {
        widths: vec![None],
        valign: vec![VAlign::Top],
        starts: vec![0],
        spans: vec![2],
        columns: Arc::clone(&columns),
        ..GridRow::default()
    });
    let row = row_of(&columns, vec![None, None], vec![0, 1]);
    let f = flow(&[
        designed(para(30), &[], at(&wide, 0)),
        words("one two three", at(&row, 0)),
        words("four five six", at(&row, 1)),
    ]);
    let ((a, aw), (b, _), (c, cw)) = (across(&f, 0), across(&f, 1), across(&f, 2));
    assert!((a - b).abs() < 1.0, "{a} {b}");
    assert!((a + aw - c - cw).abs() < 1.0, "{a} {aw} {c} {cw}");
}

#[test]
fn flex_rows_justify_grow_and_wrap() {
    let flex = |justify, wrap| {
        let flex = Some(Flex {
            justify,
            gap: 0.0,
            wrap,
        });
        Arc::new(Columns {
            flex,
            ..Columns::default()
        })
    };
    let plain = || designed(para(3), &[], None);
    let centred = flex(Justify::Center, false);
    let row = row_of(&centred, vec![None, None], vec![0, 1]);
    let f = flow(&[words("ab", at(&row, 0)), words("cd", at(&row, 1)), plain()]);
    let (left, room) = across(&f, 2);
    assert!(across(&f, 0).0 > left + room / 4.0, "{:?}", across(&f, 0));

    let start = flex(Justify::Start, false);
    let grown = Arc::new(GridRow {
        grow: vec![0.0, 1.0],
        ..(*row_of(&start, vec![None, None], vec![0, 1])).clone()
    });
    let f = flow(&[
        words("ab", at(&grown, 0)),
        words("cd", at(&grown, 1)),
        plain(),
    ]);
    let ((x0, _), (x1, w1)) = (across(&f, 0), across(&f, 1));
    assert!((x0 - left).abs() < 1.0 && (x1 + w1 - left - room).abs() < 1.0);

    let wrapping = flex(Justify::Start, true);
    let row = row_of(&wrapping, vec![Some(Length::Em(12.0)); 4], vec![0, 1, 2, 3]);
    let items: Vec<Block> = (0..4).map(|i| words("w", at(&row, i))).collect();
    let f = flow(&items);
    let (top0, top2) = (span(&f, 0, 0).0, span(&f, 0, 2).0);
    assert!(top2 > top0, "{top0} {top2}");
    assert!((across(&f, 2).0 - across(&f, 0).0).abs() < 1.0);
}

#[test]
fn small_pictures_sit_in_their_line() {
    let mut block = Block::text(Role::Paragraph, "see the sign here");
    if let Content::Text(t) = &mut block.content {
        let picture = picture(8, 8);
        t.inlines.push(Inline {
            at: 8,
            picture,
            width: 1.0,
            height: 1.0,
            rise: Rise::Baseline,
            link: Some(4),
            hidden: false,
        });
    }
    let f = flow(&[block]);
    let Laid::Text { para, .. } = &*f.laid(0) else {
        return assert!(f.blocks.len() == 1, "text");
    };
    assert_eq!(para.pictures.len(), 1);
    let boxes = para
        .layout
        .lines()
        .flat_map(|l| l.items().collect::<Vec<_>>());
    let boxes = boxes.filter(|i| matches!(i, PositionedLayoutItem::InlineBox(_)));
    assert_eq!(boxes.count(), 1);
    let links = f.links(0);
    assert!(links.iter().any(|l| l.target == 4), "{links:?}");
}

#[test]
fn round_corners_and_turns_reach_the_page() {
    let b = Arc::new(Boxed {
        border: [0.1; 4],
        edges: [Edge::Solid; 4],
        radius: Some(Length::Em(1.0)),
        ..Boxed::default()
    });
    let mut block = designed(para(5), &[b], None);
    block.frame.rotate = 90.0;
    let f = flow(&[block]);
    assert_eq!(f.blocks.first().map(|s| s.rotate), Some(90.0));
    let radius = f
        .decos
        .first()
        .and_then(|d| d.first())
        .map_or(0.0, |d| d.radius);
    assert!(radius > 1.0, "{radius}");
}

#[test]
fn a_box_cut_by_a_page_stays_open_at_the_cut() {
    let b = Arc::new(Boxed {
        border: [0.1; 4],
        edges: [Edge::Solid; 4],
        ..Boxed::default()
    });
    let blocks: Vec<Block> = (0..30)
        .map(|_| designed(para(60), &[Arc::clone(&b)], None))
        .collect();
    let f = flow(&blocks);
    assert!(f.page_count() > 1);
    let edges = |page: usize| f.decos.get(page).and_then(|d| d.first()).map(|d| d.edges);
    let last = f.page_count() - 1;
    assert!(edges(0).is_some_and(|e| e[0] == Edge::Solid && e[2] == Edge::None));
    assert!(edges(last).is_some_and(|e| e[0] == Edge::None && e[2] == Edge::Solid));
}
