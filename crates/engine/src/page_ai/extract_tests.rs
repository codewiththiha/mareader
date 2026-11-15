use super::*;

fn para(text: &str) -> Block {
    Block::text(BlockRole::Paragraph, text)
}

#[test]
fn code_is_kept_but_verse_is_sent() {
    assert!(looks_like_code("fn main() {\n    println!(\"hi\");\n}"));
    assert!(looks_like_code("x = [a * b for a in xs]\nprint(x)"));
    let verse = "Once upon a midnight dreary,\nwhile I pondered, weak and weary";
    assert!(!looks_like_code(verse));
}

#[test]
fn a_book_page_comes_back_with_its_kept_parts_in_place() {
    let mut code = Text::new(BlockRole::Preformatted);
    code.push("let x = foo(1);\nbar(x);", Emphasis::default());
    let blocks = vec![
        para("First paragraph that the page starts in the middle of."),
        Block::new(Content::Text(code)),
        Block::new(Content::Rule),
        para("Last paragraph, cut short by the page."),
    ];
    let span = span(&[(0, 12..usize::MAX), (3, 0..14)], &blocks).unwrap();
    assert_eq!(span, [[0, 12], [3, 14]]);
    let source = book(&blocks, span);
    assert_eq!(source.pieces.len(), 4);
    assert!(matches!(
        source.pieces[1],
        Piece::Kept(Kept::Code { lines: 2 })
    ));
    let Piece::Text { text, .. } = &source.pieces[3] else {
        panic!("the last piece is text");
    };
    assert_eq!(text, "Last paragraph");
    let result = Rewritten {
        outs: vec![
            Out::Text {
                role: Role::Para,
                text: "Premier paragraphe `x`.".into(),
                code: std::iter::once(19..22).collect(),
                from: Some(0),
            },
            Out::Kept(1),
            Out::Kept(2),
            Out::Text {
                role: Role::Para,
                text: "Dernier".into(),
                code: Vec::new(),
                from: Some(3),
            },
        ],
        missing: 0,
    };
    let set = super::blocks(&result, &source, &blocks);
    assert_eq!(set.len(), 4);
    assert_eq!(set[1].content, blocks[1].content);
    let Content::Text(first) = &set[0].content else {
        panic!("the first block is text");
    };
    assert_eq!(first.spans.len(), 1);
    assert!(first.spans[0].emphasis.code);
    assert!(set.iter().all(|b| !b.break_before));
}

#[test]
fn a_summary_point_hangs_a_bullet() {
    let result = Rewritten {
        outs: vec![Out::Text {
            role: Role::Item,
            text: "One point.".into(),
            code: Vec::new(),
            from: None,
        }],
        missing: 0,
    };
    let set = blocks(&result, &Source::default(), &[]);
    assert_eq!(set[0].frame.marker.as_deref(), Some("•"));
    assert_eq!(plain(&result), "• One point.");
}

#[test]
fn a_pdf_page_breaks_at_gaps_and_indents() {
    let b = |y: f32, x0: f32, x1: f32| Bounds {
        x0,
        y0: y,
        x1,
        y1: y + 10.0,
    };
    let rows = vec![
        ("The first line runs".to_owned(), b(0.0, 50.0, 500.0), false),
        ("on and ends here.".to_owned(), b(12.0, 50.0, 200.0), false),
        ("A new one starts".to_owned(), b(24.0, 50.0, 500.0), false),
        ("after a full stop.".to_owned(), b(36.0, 50.0, 500.0), false),
        (
            "Past a gap, another.".to_owned(),
            b(60.0, 50.0, 500.0),
            false,
        ),
        ("    Indented, too.".to_owned(), b(72.0, 70.0, 500.0), false),
    ];
    let starts: Vec<bool> = pdf_lines(&rows).iter().map(|l| l.starts).collect();
    assert_eq!(starts, [true, false, true, false, true, true]);
}

#[test]
fn pages_join_and_a_paragraph_cut_by_the_break_is_whole() {
    let page = |texts: &[&str]| {
        let mut s = Source::default();
        for t in texts {
            s.push(Piece::text(Role::Para, t), Origin::Loose);
        }
        s
    };
    let joined = join(vec![
        (page(&["One.", "It goes on ac-"]), false),
        (page(&["ross the break.", "Two."]), true),
        (page(&["Three starts anew."]), false),
    ]);
    let texts: Vec<&str> = (joined.pieces.iter())
        .filter_map(|p| match p {
            Piece::Text { text, .. } => Some(text.as_str()),
            Piece::Kept(_) => None,
        })
        .collect();
    assert_eq!(
        texts,
        [
            "One.",
            "It goes on across the break.",
            "Two.",
            "Three starts anew."
        ]
    );
    assert_eq!(joined.origins.len(), joined.pieces.len());
}
