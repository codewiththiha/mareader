use super::*;

#[test]
fn an_empty_book_has_one_blank_page() {
    let f = flow(&[]);
    assert_eq!(f.page_count(), 1);
}

#[test]
fn pages_never_overflow_the_column() {
    let mut blocks: Vec<Block> = (0..60).map(|i| para(20 + i * 7)).collect();
    blocks.insert(9, image(1200, 3000, false));
    let f = flow(&blocks);
    for page in &f.pages {
        for piece in page {
            let set = f.blocks.get(piece.block);
            let bottom = set
                .and_then(|s| s.units.get(piece.units.end - 1))
                .map_or(0.0, |u| u.1);
            assert!(bottom + piece.dy <= room() + 0.1, "{}", bottom + piece.dy);
        }
    }
}

#[test]
fn a_narrow_room_is_drawn_as_narrow_as_it_was_measured() {
    let blocks = Arc::new(vec![para(120)]);
    let typo = Typography::default();
    let f = Flow::sized(blocks, &typo, Fonts::bundled(), (300.0, 2000.0), false);
    let width = match &*f.laid(0) {
        Laid::Text { para, .. } => para.layout.width(),
        _ => f32::INFINITY,
    };
    assert!(width <= 300.5, "{width}");
}

#[test]
fn no_page_holds_a_lone_line_of_a_split_paragraph() {
    let blocks: Vec<Block> = (0..50).map(|i| para(30 + i * 11)).collect();
    let f = flow(&blocks);
    for (n, page) in f.pages.iter().enumerate() {
        for piece in page {
            let total = f.blocks.get(piece.block).map_or(0, |s| s.units.len());
            let split = piece.units.len() < total;
            assert!(
                !(split && piece.units.len() == 1),
                "page {n}: {:?} of {total}",
                piece.units
            );
        }
    }
}

#[test]
fn a_chapter_starts_a_new_page() {
    let mut second = para(5);
    second.break_before = true;
    let f = flow(&[para(5), second, para(5)]);
    let blocks: Vec<Vec<usize>> = f
        .pages
        .iter()
        .map(|p| p.iter().map(|piece| piece.block).collect())
        .collect();
    assert_eq!(blocks, [vec![0], vec![1, 2]]);
}

#[test]
fn lines_of_a_piece_sit_where_the_layout_put_them() {
    let f = flow(&[para(300)]);
    let first = f.pages.first().and_then(|p| p.first());
    assert_eq!(first.map(|p| p.dy), Some(0.0));
}

#[test]
fn list_items_sit_closer_than_paragraphs() {
    let item = |tight| {
        let mut b = para(3);
        b.frame = Frame {
            inset: 1.6,
            marker: Some("•".into()),
            tight,
            ..Frame::default()
        };
        b
    };
    let loose = tops(&flow(&[para(3), para(3)]));
    let tight = tops(&flow(&[item(false), item(true)]));
    let step = |t: &[f32]| t.get(1).zip(t.first()).map(|(b, a)| b - a);
    assert!(step(&tight) < step(&loose), "{tight:?} {loose:?}");
    let f = flow(&[item(false)]);
    let set = f.blocks.first();
    assert_eq!(set.map(|s| s.x), Some(1.6 * 17.0));
    let marked = matches!(
        &*f.laid(0),
        Laid::Text {
            marker: Some(_),
            ..
        }
    );
    assert!(marked);
}

#[test]
fn a_cover_fills_its_own_page() {
    let mut next = para(10);
    next.break_before = true;
    let f = flow(&[image(400, 900, true), next]);
    assert_eq!(f.page_count(), 2);
    let rect = match f.blocks.first().map(|s| &s.draw) {
        Some(Draw::Image { rect, .. }) => *rect,
        _ => [0.0; 4],
    };
    // Scaled up to the page box's height, centred across.
    assert!((rect[3] - room()).abs() < 0.01, "{rect:?}");
    let column = PAGE_WIDTH - 2.0 * PAD;
    assert!((rect[0] * 2.0 + rect[2] - column).abs() < 0.01, "{rect:?}");
}

#[test]
fn a_table_head_never_ends_a_page_alone() {
    let cell = |s: &str| {
        let mut t = Text::new(Role::Paragraph);
        t.push(s, Emphasis::default());
        t
    };
    let row = |head| Row {
        cells: vec![cell("cell"), cell("cell")],
        head,
    };
    // Some lead-in length leaves room for only the head row.
    for words in (200..900).step_by(9) {
        let rows = vec![row(true), row(false), row(false)];
        let table = Block::new(Content::Table(Table { rows, rule: None }));
        let f = flow(&[para(words), table]);
        for page in &f.pages {
            let last = page.last();
            let alone = last.is_some_and(|p| p.block == 1 && p.units == (0..1));
            assert!(!alone, "{words} words: {:?}", f.pages);
        }
    }
}

#[test]
fn preformatted_text_keeps_its_lines_and_padding() {
    let mut t = Text::new(Role::Preformatted);
    t.push("fn main() {\n    run();\n}", Emphasis::default());
    let f = flow(&[Block::new(Content::Text(t))]);
    let set = f.blocks.first();
    assert_eq!(set.map(|s| s.units.len()), Some(3));
    // The first unit starts above the first line by the padding.
    let first = set.and_then(|s| s.units.first()).map_or(0.0, |u| u.0);
    assert!((first + PRE_PAD.1 * 17.0).abs() < 0.01, "{first}");
}
