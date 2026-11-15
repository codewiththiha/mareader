use super::*;

#[test]
fn line_pictures_sit_where_their_vertical_align_puts_them() {
    let mut shelf = Shelf::default();
    if let Some(p) = png(16, 16) {
        shelf.pictures.insert("/OEBPS/images/s.png".into(), p);
    }
    let img = |align: &str| format!("<img style='vertical-align: {align}' src='../images/s.png'/>");
    let html = ["middle", "text-top", "bottom", "0.25em", "-8px", "auto"].map(img);
    let b = read_with(&mut shelf, &format!("<p>x {}</p>", html.concat()));
    let rises: Vec<Rise> = (b.first().and_then(text).into_iter())
        .flat_map(|t| t.inlines.iter().map(|i| i.rise))
        .collect();
    let expected = [
        Rise::Middle,
        Rise::TextTop,
        Rise::Bottom,
        Rise::By(0.25),
        Rise::By(-0.5),
        Rise::Baseline,
    ];
    assert_eq!(rises, expected);
}

#[test]
fn a_one_cell_table_of_paragraphs_is_a_framed_box() {
    let b = read(
        "<table style=\"border: 3px double; width: 17em; margin: 0 auto\">\
             <tr><td><p>KEY WORD</p><p>A sword.</p></td></tr></table>",
    );
    assert_eq!(kinds(&b), ["text", "text"]);
    let edges = b
        .get(1)
        .and_then(|b| b.frame.boxes.first())
        .map(|b| b.edges);
    assert_eq!(edges, Some([Edge::Double; 4]));
}

#[test]
fn images_take_the_width_the_book_sets() {
    let mut shelf = Shelf::default();
    if let Some(p) = png(4, 4) {
        shelf.pictures.insert("/OEBPS/images/a.png".into(), p);
    }
    let sheet = Stylesheet::parse("img.small { width: 2em }", "/OEBPS/s.css");
    shelf.sheets.insert("/OEBPS/s.css".into(), Arc::new(sheet));
    let b = read_with(
        &mut shelf,
        "<link rel='stylesheet' href='../s.css'/>\
             <img src='../images/a.png' style='width: 6.5em'/>\
             <img class='small' src='../images/a.png'/>\
             <img src='../images/a.png' width='32'/>\
             <img src='../images/a.png' style='width: 50%'/>\
             <img src='../images/a.png'/>",
    );
    let widths: Vec<Option<Length>> = b
        .iter()
        .filter_map(|b| match &b.content {
            Content::Image { width, .. } => Some(*width),
            _ => None,
        })
        .collect();
    assert_eq!(
        widths,
        [
            Some(Length::Em(6.5)),
            Some(Length::Em(2.0)),
            Some(Length::Em(2.0)),
            Some(Length::Percent(50.0)),
            None,
        ]
    );
}

#[test]
fn a_chapter_applies_a_few_sheets_each_once() {
    let mut shelf = Shelf::default();
    let linked = Stylesheet::parse(".it { font-style: italic }", "/OEBPS/css/book.css");
    shelf
        .sheets
        .insert("/OEBPS/css/book.css".into(), Arc::new(linked));
    let links = "<link rel='stylesheet' href='../css/book.css'/>".repeat(100);
    let styles: String = (0..MAX_SHEETS)
        .map(|i| format!("<style>.c{i} {{ text-align: center }}</style>"))
        .collect();
    let last = MAX_SHEETS - 2;
    let body = format!(
        "<p class='c0'>a</p><p class='c{last}'>b</p><p class='c{}'>c</p>",
        last + 1
    );
    let b = read_with(&mut shelf, &format!("<head>{links}{styles}</head>{body}"));
    let aligns: Vec<Align> = b.iter().map(|b| b.frame.align).collect();
    // The linked sheet counts once, so the last style is one too many.
    assert_eq!(aligns, [Align::Center, Align::Center, Align::Start]);
}

#[test]
fn sheets_load_by_media_and_import_and_fill_in_custom_values() {
    let mut shelf = Shelf::default();
    for (path, css) in [
        (
            "/OEBPS/css/book.css",
            "@import url(base.css); @import 'print.css' print; .it { font-style: italic }",
        ),
        (
            "/OEBPS/css/base.css",
            "p { --mid: center; text-align: var(--mid) }",
        ),
        ("/OEBPS/css/print.css", "p { display: none }"),
        ("/OEBPS/css/night.css", "p { display: none }"),
        (
            "/OEBPS/css/loop.css",
            "@import url(loop.css); .lp { font-weight: bold }",
        ),
    ] {
        let sheet = Arc::new(Stylesheet::parse(css, path));
        shelf.sheets.insert(path.into(), sheet);
    }
    let b = read_with(
        &mut shelf,
        "<head><link rel='stylesheet' href='../css/book.css'/>\
             <link rel='alternate stylesheet' href='../css/night.css'/>\
             <link rel='stylesheet' media='print' href='../css/print.css'/>\
             <link rel='stylesheet' href='../css/loop.css'/>\
             <style media='amzn-kf8'>p { display: none }</style>\
             <style media='screen'>.wide { width: calc(100% - 4em) }</style></head>\
             <p>a <span class='it lp'>b</span></p>\
             <div class='wide' style='border: 1px solid'><p>c</p></div>",
    );
    assert_eq!(texts(&b), ["a b", "c"]);
    assert_eq!(b.first().map(|b| b.frame.align), Some(Align::Center));
    let e = Emphasis {
        italic: true,
        bold: true,
        ..Emphasis::default()
    };
    assert_eq!(spans(b.first()), [(" b", e)]);
    let wide = b
        .get(1)
        .and_then(|b| b.frame.boxes.first())
        .map(|x| x.width);
    let mix = Length::Mix {
        em: -4.0,
        percent: 100.0,
    };
    assert_eq!(wide, Some(Some(mix)));
}

#[test]
fn root_custom_values_reach_the_body_once_sheets_load() {
    let b = read(
        "<html class='x'><head><style>:root { --mid: center } \
             html.x { --pad: 2em } p { text-align: var(--mid); text-indent: var(--pad) }\
             </style></head><body><p>a</p></body></html>",
    );
    assert_eq!(b.first().map(|b| b.frame.align), Some(Align::Center));
    let indent = b
        .first()
        .and_then(|b| b.frame.look.as_ref())
        .map(|l| l.indent);
    assert_eq!(indent, Some(2.0));
}
