use super::*;

#[test]
fn prose_takes_the_books_type_moved_by_the_readers() {
    let typed = |role, size, weight| {
        let mut b = Block::text(role, "Short words");
        b.frame.look = Some(Arc::new(Look {
            size,
            weight,
            ..Look::default()
        }));
        b
    };
    let blocks = Arc::new(vec![
        typed(Role::Heading(2), 1.5, 700.0),
        typed(Role::Paragraph, 1.0, 400.0),
        typed(Role::Paragraph, 0.8, 400.0),
        designed(Block::text(Role::Paragraph, "Short words"), &[], None),
    ]);
    let laid = |typo: Typography| Flow::new(Arc::clone(&blocks), &typo, Fonts::bundled());
    let tall = |f: &Flow, i: usize| {
        let s = f.blocks.get(i);
        s.map_or(0.0, |s| span_height(s, 0..s.units.len()))
    };
    let base = laid(Typography::default());
    let big = laid(Typography {
        font_size: 34.0,
        ..Typography::default()
    });
    let off = laid(Typography {
        design: Design::Off,
        ..Typography::default()
    });
    // The book's steps hold: the heading is 1.5 times, the note 0.8.
    let near = |a: f32, b: f32| (a - b).abs() <= b * 0.06;
    let (heading, body, note) = (tall(&base, 0), tall(&base, 1), tall(&base, 2));
    assert!(
        near(heading / body, 1.5 * HEADING_LINE_HEIGHT / READER.1),
        "{heading} {body}"
    );
    assert!(near(note / body, 0.8), "{note} {body}");
    // Twice the reader's size doubles every size, designed type too.
    for i in 0..4 {
        assert!(near(tall(&big, i), 2.0 * tall(&base, i)), "{i}");
    }
    // The book's margins of none hold; Off keeps the reader's gap.
    let gap = |f: &Flow| span(f, 0, 2).0 - span(f, 0, 1).1;
    assert!(gap(&base).abs() < 0.5, "{}", gap(&base));
    assert!(near(gap(&off), READER.0), "{}", gap(&off));
    // Off sets every paragraph in the reader's one size.
    assert!(near(tall(&off, 2), tall(&off, 1)));
}

#[test]
fn designed_type_is_set_as_on_a_phone_page() {
    let f = flow(&[designed(para(1), &[], None), para(1)]);
    let em = Typography::default().font_size;
    let design = design_em(&f);
    assert!(design > em && design <= em * MAX_DESIGN_SCALE, "{design}");
    let tall = |i| {
        f.blocks
            .get(i)
            .map_or(0.0, |s| span_height(s, 0..s.units.len()))
    };
    assert!(tall(0) > tall(1) * 1.2, "{} {}", tall(0), tall(1));
}

#[test]
fn cases_show_over_the_text_keeping_its_offsets() {
    let mut t = Text::new(Role::Paragraph);
    let small = Emphasis {
        case: Case::Small,
        ..Emphasis::default()
    };
    t.push("Top ", Emphasis::default());
    t.push("Straße ok", small);
    let (shown, runs) = cased(&t);
    assert_eq!(shown, "Top STRAßE OK");
    assert_eq!(shown.len(), t.text.len());
    let runs: Vec<Range<usize>> = runs.into_iter().map(|(r, _)| r).collect();
    assert_eq!(runs, [5..8, 10..11, 12..14]);
}

#[test]
fn all_small_caps_shrink_capitals_too() {
    let mut t = Text::new(Role::Paragraph);
    let all = Emphasis {
        case: Case::AllSmall,
        ..Emphasis::default()
    };
    t.push("NASA flew", all);
    let (shown, runs) = cased(&t);
    assert_eq!(shown, "NASA FLEW");
    let runs: Vec<Range<usize>> = runs.into_iter().map(|(r, _)| r).collect();
    assert_eq!(runs, [0..4, 5..9]);
}

#[test]
fn a_paragraphs_block_and_set_stay_small() {
    // A book keeps one of each per paragraph; see docs/memory.md.
    assert!(std::mem::size_of::<Block>() <= 264);
    assert!(std::mem::size_of::<Set>() <= 200);
}

#[test]
fn splits_keep_the_books_widows_and_orphans() {
    let usual = (2, 2);
    // Ten lines, room for nine: two go over, as one would be alone.
    assert_eq!(balance(9, (0, 10), false, usual), 8);
    assert_eq!(balance(9, (0, 10), false, (2, 3)), 7);
    // One line fits at the foot: the paragraph moves on.
    assert_eq!(balance(1, (0, 10), false, usual), 0);
    assert_eq!(balance(2, (0, 10), false, (3, 2)), 0);
    assert_eq!(balance(3, (0, 10), false, (3, 2)), 3);
    // Atop a page there is no better place: it splits anyway.
    assert_eq!(balance(1, (0, 10), true, (3, 3)), 1);
    assert_eq!(lines_kept(0, true), 2);
    assert_eq!(lines_kept(4, false), 2);
    assert_eq!(lines_kept(200, true), MAX_KEPT_LINES);
}

#[test]
fn features_later_win_and_read_as_css() {
    use crate::block::Features;
    let on = |t| Features::of(t, true).unwrap_or_default();
    let off = |t| Features::of(t, false).unwrap_or_default();
    let early = on("liga").then(on("onum"));
    let both = early.then(off("liga")).then(on("zero"));
    assert_eq!(both.source(), "\"liga\" 0, \"onum\" 1, \"zero\" 1");
    assert_eq!(Features::of("wxyz", true), None);
    assert!(Features::default().is_empty());
}

#[test]
fn raised_text_sets_smaller_on_its_line() {
    let mut t = Text::new(Role::Paragraph);
    t.push("x", Emphasis::default());
    let sup = Emphasis {
        script: Script::Super,
        underline: true,
        strike: true,
        ..Emphasis::default()
    };
    t.push("2", sup);
    let f = flow(&[Block::new(Content::Text(t))]);
    let laid = f.laid(0);
    let lines = match &*laid {
        Laid::Text { para, .. } => para.layout.lines().collect(),
        _ => Vec::new(),
    };
    let runs: Vec<(f32, bool, bool)> = lines
        .iter()
        .flat_map(|l| l.items().collect::<Vec<_>>())
        .filter_map(|i| match i {
            parley::PositionedLayoutItem::GlyphRun(r) => Some((
                r.run().font_size(),
                r.style().underline.is_some(),
                r.style().strikethrough.is_some(),
            )),
            parley::PositionedLayoutItem::InlineBox(_) => None,
        })
        .collect();
    assert_eq!(runs.len(), 2, "{runs:?}");
    let (Some(x), Some(two)) = (runs.first(), runs.get(1)) else {
        return;
    };
    assert!(two.0 < x.0 && two.1 && two.2 && !x.1, "{runs:?}");
}

#[test]
fn hyphenated_links_keep_their_words() {
    let typo = Typography {
        hyphenate: true,
        ..Typography::default()
    };
    let words = "extraordinarily incomprehensible ".repeat(30);
    let blocks = [linked("Some text ", &words, 3)];
    let plain = with(&blocks, Typography::default()).links(0);
    let split = with(&blocks, typo).links(0);
    // The link starts after the same unlinked words either way.
    let first = |l: &[PageLink]| l.first().map(|l| l.rect[0]);
    assert!(first(&split).is_some() && first(&split) == first(&plain));
    assert!(split.len() + 1 >= plain.len(), "{split:?}");
    let right = PAGE_WIDTH - PAD + 1.0;
    let inside = |l: &PageLink| l.rect[0] >= PAD - 1.0 && l.rect[0] + l.rect[2] <= right;
    assert!(split.iter().all(inside), "{split:?}");
}
