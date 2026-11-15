use super::*;

#[test]
fn designed_chapters_keep_the_books_type_and_space() {
    let b = read(
        "<style>@font-face { font-family: Title; src: url(../Fonts/t.ttf) }\
             .t { font-family: Title, serif; font-size: 2em; letter-spacing: 0.3em }\
             .gap { margin-top: 1em } p { margin: 0.5em 0 } .lh { line-height: 1em }\
             </style><p class='t'>TITLE <span style='font-size: 50%'>small</span></p>\
             <div class='gap'><p class='lh'>cast</p></div><p>after</p>",
    );
    assert_eq!(texts(&b), ["TITLE small", "cast", "after"]);
    let title = look(b.first());
    assert!(title.is_some_and(|l| l.designed && l.size == 2.0));
    assert_eq!(
        title.map(|l| l.faces.clone()),
        Some(vec![format!("{BOOK_FACE}title")])
    );
    assert!(title.is_some_and(|l| (l.letter - 0.6).abs() < 0.01));
    let small = spans(b.first())
        .into_iter()
        .find(|(t, _)| t.trim() == "small");
    assert_eq!(small.map(|(_, e)| e.size), Some(50));
    // Margins collapse to the largest; a block's own are in its em.
    let before: Vec<f32> = b.iter().map(|b| b.frame.before).collect();
    assert_eq!(before, [1.0, 1.0, 0.5]);
    // Without paragraphs' own margins, only the div's is left.
    let outer: Vec<f32> = b.iter().map(|b| b.frame.outer).collect();
    assert_eq!(outer, [0.0, 1.0, 0.0]);
    assert_eq!(look(b.get(1)).and_then(|l| l.line_height), Some(1.0));
}

#[test]
fn a_framed_figure_leaves_prose_prose_but_a_framed_page_is_designed() {
    let prose = "<p>Once upon a midnight dreary, while I pondered.</p>".repeat(40);
    let designed = |b: &[Block]| b.iter().any(|b| look(Some(b)).is_some_and(|l| l.designed));
    let figure = "<div style='width: 556px'><p>A figure's caption.</p></div>\
             <p style='border-top: 1px dotted'>Transcriber's note.</p>";
    assert!(!designed(&read(&format!("{prose}{figure}{prose}"))));
    // Short, or mostly inside its frames, a chapter is laid out.
    assert!(designed(&read(figure)));
    let framed = format!("<div style='border: 1px solid'>{prose}</div>");
    assert!(designed(&read(&framed)));
}

#[test]
fn unstyled_chapters_leave_the_type_to_the_reader() {
    let b = read("<h1>One</h1><p>a</p>");
    assert!(b.iter().all(|b| b.frame.look.is_none()), "{b:?}");
    let b = read("<h1>One</h1><p style='text-indent: 1em'>a</p>");
    assert!(b.iter().all(|b| b.frame.look.is_some()), "{b:?}");
    // Sans and mono stay marked; the machine's own names stay too.
    let b = read("<style>p { font-family: Georgia, monospace }</style><p>a</p>");
    let faces = look(b.first()).map(|l| l.faces.clone());
    assert_eq!(
        faces,
        Some(vec!["Georgia".to_owned(), "monospace".to_owned()])
    );
}

#[test]
fn a_headings_plain_words_stay_light_at_their_size() {
    let b = read(
        "<style>.t { font-weight: normal; font-size: 26px }\
             .n { font-weight: normal; font-size: 94px }</style>\
             <h2><span class='t'>Chapter</span> <span class='n'>2</span> The Pool</h2>",
    );
    let runs: Vec<(String, bool, u16)> = (spans(b.first()).into_iter())
        .map(|(t, e)| (t.trim().to_owned(), e.light, e.size))
        .collect();
    // Against the heading's 24 px: 26 and 94 px.
    let want = [("Chapter", true, 108), ("2", true, 392)];
    assert_eq!(runs, want.map(|(t, l, s)| (t.to_owned(), l, s)), "{b:?}");
}

#[test]
fn prose_chapters_carry_their_type_unlaid() {
    let b = read(
        "<style>p { margin: 1em 0; font-size: 1.1em } h1 { font-size: 2em }\
             h1 + p { text-align: center }</style>\
             <h1>One</h1><p>a <b>b</b></p><p>c</p>",
    );
    assert!(b.iter().all(|b| look(Some(b)).is_some_and(|l| !l.designed)));
    // A heading's own weight is no emphasis; bold in a paragraph is.
    assert!(spans(b.first()).iter().all(|(_, e)| !e.bold));
    assert!(
        spans(b.get(1))
            .iter()
            .any(|(t, e)| t.trim() == "b" && e.bold)
    );
    let aligns: Vec<Align> = b.iter().map(|b| b.frame.align).collect();
    assert_eq!(aligns, [Align::Start, Align::Center, Align::Start]);
}

#[test]
fn direction_follows_the_attribute_and_the_sheet() {
    let blocks = read(
        "<html><head><style>.r { direction: rtl } .l { direction: ltr }</style></head>\
             <body><div class=\"r\"><p>a</p><p dir=\"ltr\">b</p></div>\
             <p dir=\"rtl\" class=\"l\">c</p><p dir=\"auto\">d</p><p>e</p></body></html>",
    );
    let dirs: Vec<Dir> = blocks.iter().map(|b| b.frame.dir).collect();
    // The sheet beats the attribute, which is only a hint.
    assert_eq!(dirs, [Dir::Rtl, Dir::Ltr, Dir::Ltr, Dir::Auto, Dir::Auto]);
}

#[test]
fn ruby_without_rt_or_too_long_a_note_adds_nothing() {
    let long = "x".repeat(MAX_NOTE + 1);
    let blocks = read(&format!(
        "<body><p><ruby>base</ruby> <ruby>more<rt>{long}</rt></ruby>\
             <ruby>end<rt> </rt></ruby></p></body>"
    ));
    let t = blocks.first().and_then(text);
    assert_eq!(
        t.map(|t| (t.text.as_str(), t.ruby.len())),
        Some(("base moreend", 0))
    );
}
