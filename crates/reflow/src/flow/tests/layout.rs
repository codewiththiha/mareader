use super::*;

#[test]
fn a_region_is_set_like_its_book_and_keeps_spread_sides() {
    let typo = Typography {
        book_layout: true,
        ..Typography::default()
    };
    let base = with(&book(), typo.clone());
    let blocks = Arc::new(vec![para(30), para(500), para(30)]);
    let region = |first, replaced| {
        let fonts = Fonts::bundled();
        base.region(Arc::clone(&blocks), Some("fr"), fonts, (first, replaced))
    };
    let one = region(3, 1);
    let laid = Flow::new(Arc::clone(&blocks), &typo, Fonts::bundled());
    assert_eq!(one.page_size(), base.page_size());
    // An odd change would swap later sides, so a blank page evens it.
    let extra = usize::from(laid.page_count().is_multiple_of(2));
    assert_eq!(one.page_count(), laid.page_count() + extra);
    let last = one.excerpts(one.page_count() - 1);
    assert_eq!(last.is_empty(), extra == 1, "{last:?}");
    // Its first page sits on the side of the page it replaces.
    assert_eq!(one.column(0), base.column(3));
    assert_eq!(region(4, 1).column(0), base.column(4));
    assert_eq!(one.bands().len(), one.page_count());
    let band = one.bands().first().map(|b| (b.x, b.width));
    assert_eq!(band, Some(one.column(0)));
    let plain = with(&book(), Typography::default());
    let free = plain.region(Arc::clone(&blocks), None, Fonts::bundled(), (3, 1));
    assert_eq!(free.page_count(), plain_count(&blocks));
}

#[test]
fn a_resting_book_keeps_only_its_newest_layouts() {
    let mut recent = Recent::default();
    let laid = || Arc::new(Laid::None);
    for block in 0..8 {
        recent.put(block, laid(), REST_LAID / 2);
    }
    // Block 0 was used last, so it outlives the newer puts.
    assert!(recent.get(0).is_some());
    recent.shrink(REST_LAID, None);
    assert!(recent.bytes <= REST_LAID);
    let mut kept: Vec<usize> = recent.laid.keys().copied().collect();
    kept.sort_unstable();
    assert_eq!(kept, [0, 7]);
}

#[test]
fn a_resting_flow_draws_the_same() {
    let f = flow(&book());
    let region = [0, 0, PAGE_WIDTH as u32, PAGE_HEIGHT as u32];
    let first = f.render(1, 1.0, region);
    f.rest();
    assert_eq!(f.render(1, 1.0, region), first);
    assert!(first.is_some());
}

#[test]
fn hebrew_or_arabic_reads_right_to_left_unless_told_otherwise() {
    let hebrew = "שלום עולם";
    let f = flow(&[
        directed(hebrew, Dir::Auto),
        directed("word word", Dir::Auto),
        directed(hebrew, Dir::Ltr),
        directed("word word", Dir::Rtl),
    ]);
    let rtl: Vec<bool> = f.blocks.iter().map(|s| s.rtl).collect();
    assert_eq!(rtl, [true, false, false, true]);
    // A short right-to-left line sits at the right of its column.
    let first = |b| match &*f.laid(b) {
        Laid::Text { para, .. } => para.layout.lines().next().map(|l| {
            let m = l.metrics();
            m.offset + m.inline_min_coord
        }),
        _ => None,
    };
    let (auto, ltr) = (first(0).unwrap_or(0.0), first(2).unwrap_or(f32::NAN));
    assert!(auto > ltr + f.width / 3.0, "{auto} {ltr}");
}

#[test]
fn right_to_left_quote_bars_move_to_the_right() {
    let quote = |dir| {
        let mut b = directed("word word", dir);
        b.frame.bars = crate::thin_vec![0.0];
        b
    };
    let f = flow(&[quote(Dir::Ltr), quote(Dir::Rtl)]);
    let bar = |i: usize| {
        let set = f.blocks.get(i);
        set.and_then(|s| s.bars.first().copied())
            .unwrap_or(f32::NAN)
    };
    assert!(
        bar(0) < 1.0 && bar(1) > f.width / 2.0,
        "{} {}",
        bar(0),
        bar(1)
    );
}

#[test]
fn ruby_lays_its_note_and_makes_room_above_the_line() {
    let mut noted = Block::text(Role::Paragraph, "word word");
    if let Content::Text(t) = &mut noted.content {
        t.ruby.push(crate::block::Annotation {
            range: 0..4,
            text: "note".into(),
        });
    }
    let f = flow(&[noted, Block::text(Role::Paragraph, "word word")]);
    let tall = |i: usize| {
        let set = f.blocks.get(i);
        set.and_then(|s| s.units.first()).map_or(0.0, |u| u.1 - u.0)
    };
    assert!(tall(0) > tall(1) + 1.0, "{} {}", tall(0), tall(1));
    let note = match &*f.laid(0) {
        Laid::Text { para, .. } => para
            .ruby
            .first()
            .map(|n| (n.range.clone(), n.para.is_some())),
        _ => None,
    };
    assert_eq!(note, Some((0..4, true)));
}

#[test]
fn east_asian_letters_stand_and_the_rest_lie() {
    let text = "縦書き ABC、12";
    let runs = upright_runs(text, Orient::Mixed);
    let words: Vec<&str> = runs.iter().filter_map(|r| text.get(r.clone())).collect();
    assert_eq!(words, ["縦書き", "、"]);
    assert!(upright_runs(text, Orient::Sideways).is_empty());
    let all = upright_runs(text, Orient::Upright);
    assert_eq!(all.first(), Some(&(0..text.len())));
}

#[test]
fn vertical_writing_gets_pages_of_its_own() {
    let mut blocks = vec![Block::text(Role::Paragraph, "level words")];
    blocks.extend((0..40).map(|i| turned(&"word ".repeat(30 + i))));
    blocks.push(Block::text(Role::Paragraph, "level again"));
    let f = flow(&blocks);
    let last = f.page_count() - 1;
    assert!(!f.turned(0) && !f.turned(last));
    let turned: Vec<usize> = (0..f.page_count()).filter(|&p| f.turned(p)).collect();
    assert!(turned.len() >= 2, "{turned:?}");
    assert_eq!(turned.first(), Some(&1));
    // Laid across the page's height, a turned piece fits its width.
    let (width, height) = f.page_size();
    for &page in &turned {
        for line in f.text(page) {
            for c in line.chars {
                let [x, y, w, h] = c.rect;
                assert!(x >= 0.0 && x + w <= width + 0.5, "{page} {c:?}");
                assert!(y >= 0.0 && y + h <= height + 0.5, "{page} {c:?}");
            }
        }
    }
}

#[test]
fn a_turned_rect_lands_from_the_right() {
    let f = flow(&[turned("word word")]);
    assert!(f.turned(0));
    let (width, _) = f.page_size();
    // A line along the top of the laid page is the rightmost column.
    let [x, y, w, h] = f.shown_rect(0, [100.0, 0.0, 300.0, 20.0]);
    assert_eq!([x, y, w, h], [width - 20.0, 100.0, 20.0, 300.0]);
}

#[test]
fn a_picture_s_areas_are_link_boxes_over_it() {
    let mut block = image(200, 100, false);
    if let Content::Image { links, .. } = &mut block.content {
        links.push(Area {
            rect: [0.5, 0.0, 0.5, 1.0],
            target: 7,
        });
    }
    let f = flow(&[Block::text(Role::Paragraph, "Above."), block]);
    let (links, drawn) = (f.links(0), f.pictures(0));
    assert_eq!((links.len(), drawn.len()), (1, 1), "{links:?} {drawn:?}");
    let (Some(link), Some(p)) = (links.first(), drawn.first()) else {
        return;
    };
    assert_eq!(link.target, 7);
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    assert!(near(link.rect[0], p[0] + p[2] / 2.0), "{link:?} {p:?}");
    assert!(
        near(link.rect[1], p[1]) && near(link.rect[2], p[2] / 2.0),
        "{link:?}"
    );
    assert!(near(link.rect[3], p[3]), "{link:?} {p:?}");
}
