use super::*;

#[test]
fn headings_and_paragraphs_become_blocks() {
    let b = read(
        "<?xml version='1.0'?><html><head><title>T</title>\
             <style>p { color: red }</style></head><body>\
             <h2>Chapter  One</h2><p>First\n   para.</p><p>Second.</p></body></html>",
    );
    assert_eq!(texts(&b), ["Chapter One", "First para.", "Second."]);
    assert_eq!(role(b.first()), Some(Role::Heading(2)));
    assert_eq!(role(b.get(1)), Some(Role::Paragraph));
}

#[test]
fn book_colours_ride_the_themes_greys_keeping_their_hue() {
    let b = read(
        "<html><head><style>body { color: #333 } .r { color: #c00 } \
             .g { color: #999 } .k { color: #000 } .w { color: #fff } \
             .box { background: #000 }</style></head><body>\
             <p>plain <span class='r'>red</span> <span class='g'>grey</span> \
             <span class='k'>black</span> <font color='#c00'>font</font></p>\
             <div class='box'><p class='w'>white on black</p></div>\
             <p style='color: #fafafa'>faint</p></body></html>",
    );
    let marks: Vec<(String, Option<[u8; 3]>)> = b
        .iter()
        .flat_map(|b| spans(Some(b)))
        .map(|(t, e)| (t.trim().to_owned(), e.colour))
        .collect();
    let of = |word: &str| marks.iter().find(|(t, _)| t == word).and_then(|m| m.1);
    let near = |c: Option<[u8; 3]>, v: u8| c.is_some_and(|c| c.iter().all(|x| x.abs_diff(v) <= 1));
    // The book's own ink, and anything darker, is the theme's ink.
    assert!(marks.iter().all(|(t, _)| t != "plain"), "{marks:?}");
    assert_eq!(of("black"), None, "{marks:?}");
    assert_eq!(of("white on black"), None, "{marks:?}");
    // A hue keeps the grey it needs; greys fade by their contrast.
    assert_eq!(of("red"), Some([204, 0, 0]), "{marks:?}");
    assert_eq!(of("font"), of("red"));
    assert!(near(of("grey"), 128), "{marks:?}");
    // A faint colour fades only so far.
    assert!(near(of("faint"), 140), "{marks:?}");
}

#[test]
fn borders_markers_and_grids_take_the_books_colours() {
    let b = read(
        "<html><head><style>.box { border: 2px solid #c00; border-left-color: #000 } \
             .tint { color: #1c71d8; border-bottom: 1px dotted } .faint { border: 1px solid #ddd } \
             li { color: #c00 } td { border: 1px solid #c00 }</style></head><body>\
             <div class='box'><p>boxed</p></div><p class='tint'>tinted</p>\
             <div class='faint'><p>faint</p></div><ul><li>item</li></ul>\
             <table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table></body></html>",
    );
    let block = |word: &str| {
        b.iter()
            .find(|b| text(b).is_some_and(|t| t.text.trim() == word))
    };
    let sides = |word: &str| {
        let frame = block(word).map(|b| &b.frame);
        frame.and_then(|f| f.boxes.last()).map(|b| b.colours)
    };
    let red = Some([204, 0, 0]);
    // The book's own ink stays the theme's; other sides keep theirs.
    assert_eq!(sides("boxed"), Some([red, red, red, None]));
    // Unsaid, a border takes its text's colour.
    let blue = sides("tinted").and_then(|s| s[2]);
    assert!(blue.is_some_and(|[r, _, b]| b > r + 100), "{blue:?}");
    // A light rule may stay lighter than text would.
    let faint = sides("faint").and_then(|s| s[0]);
    assert!(
        faint.is_some_and(|c| c.iter().all(|v| *v > 150)),
        "{faint:?}"
    );
    assert_eq!(block("item").and_then(|b| b.frame.marker_colour), red);
    let grid = b.iter().find_map(|b| match &b.content {
        Content::Table(t) => Some(t.rule),
        _ => None,
    });
    assert_eq!(grid, Some(red));
}

#[test]
fn inline_marks_raise_case_and_line_their_text() {
    let b = read(
        "<p>mc<sup>2</sup> H<sub>2</sub>O <u>u</u> <del>d</del> \
             <span style='font-variant: small-caps'>sc</span> \
             <span style='text-transform: uppercase'>up</span></p>",
    );
    let marks: Vec<(String, Emphasis)> = spans(b.first())
        .into_iter()
        .map(|(t, e)| (t.trim().to_owned(), e))
        .collect();
    let of = |word: &str| marks.iter().find(|(t, _)| t == word).map(|(_, e)| *e);
    assert_eq!(of("2").map(|e| e.script), Some(Script::Super), "{marks:?}");
    assert!(
        marks
            .iter()
            .any(|(t, e)| t == "2" && e.script == Script::Sub)
    );
    assert!(of("u").is_some_and(|e| e.underline && !e.strike));
    assert!(of("d").is_some_and(|e| e.strike && !e.underline));
    assert_eq!(of("sc").map(|e| e.case), Some(Case::Small));
    assert_eq!(of("up").map(|e| e.case), Some(Case::Upper));
}

#[test]
fn page_breaks_before_and_after_start_pages() {
    let b = read(
        "<p>a</p><p style='page-break-after: always'>b</p><p>c</p>\
             <h2 style='break-before: page'>d</h2><p>e</p>",
    );
    let breaks: Vec<bool> = b.iter().map(|b| b.break_before).collect();
    assert_eq!(breaks, [true, false, true, true, false]);
}

#[test]
fn backgrounds_show_by_their_contrast_with_the_page() {
    let b = read(
        "<body style='background: #000'>\
             <div style='background-color: #fff; width: 1px; height: 6em'><p><br/></p></div>\
             <div style='background: white; padding: 1em'><p>words</p></div>\
             <div style='background: #111'><p>faint</p></div></body>",
    );
    assert_eq!(kinds(&b), ["space", "text", "text"]);
    let fill = |i: usize| {
        let block = b.get(i);
        block
            .and_then(|b| b.frame.boxes.first())
            .and_then(|x| x.fill)
    };
    let line = b.first();
    assert!(line.is_some_and(|l| matches!(l.content, Content::Space(h) if h >= 6.0)));
    assert!(fill(0).is_some_and(|f| f > 0.9), "{:?}", fill(0));
    assert_eq!(fill(1), Some(MAX_TEXT_FILL));
    assert_eq!(fill(2), None);
}

#[test]
fn a_background_picture_resolves_against_its_sheet() {
    let mut shelf = Shelf::default();
    if let Some(p) = png(4, 4) {
        shelf.pictures.insert("/OEBPS/images/c1.png".to_owned(), p);
    }
    let b = read_with(
        &mut shelf,
        "<html><head><style>.b { background: url('../images/c1.png') no-repeat \
             center / cover; padding: 1em }</style></head>\
             <body><div class='b'><p>over</p></div></body></html>",
    );
    let backdrop = b
        .first()
        .and_then(|b| b.frame.boxes.first())
        .and_then(|x| x.backdrop.clone());
    let Some(backdrop) = backdrop else {
        return assert_eq!(kinds(&b), ["backdrop"]);
    };
    assert_eq!(backdrop.size, BackdropSize::Cover);
    assert!(!backdrop.repeat);
    assert_eq!(backdrop.at, [0.5, 0.5]);
}

#[test]
fn a_floats_blocks_share_it_and_what_follows_does_not() {
    let b = read(
        "<div style='float: right'><p>Key</p><p>word</p></div><p>text</p>\
             <span style='float: left'>inline</span>",
    );
    let float = |i: usize| b.get(i).and_then(|b| b.frame.float.clone());
    let (Some(a), Some(c)) = (float(0), float(1)) else {
        return assert_eq!(texts(&b), ["float"]);
    };
    assert!(Arc::ptr_eq(&a, &c) && a.right);
    assert!(float(2).is_none() && float(3).is_none());
}

#[test]
fn only_the_first_block_starts_a_page() {
    let b = read("<p>a</p><p>b</p>");
    let breaks: Vec<bool> = b.iter().map(|b| b.break_before).collect();
    assert_eq!(breaks, [true, false]);
}
