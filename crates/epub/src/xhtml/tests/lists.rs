use super::*;

#[test]
fn blocks_of_one_list_figure_or_aside_share_it() {
    let b = read(
        "<ul><li>a<ul><li>nested</li></ul></li><li><figure><p>pic</p></figure></li></ul>\
             <p>plain</p><ol><li>b</li></ol><div epub:type='sidebar'><p>side</p></div>",
    );
    let kinds =
        |b: &Block| -> Vec<PassageKind> { b.frame.passages.iter().map(|p| p.kind).collect() };
    let (list, figure) = (PassageKind::List, PassageKind::Figure);
    let all: Vec<Vec<PassageKind>> = b.iter().map(kinds).collect();
    assert_eq!(
        all,
        [
            vec![list],
            vec![list],
            vec![list, figure],
            vec![],
            vec![list],
            vec![PassageKind::Aside],
        ]
    );
    let first = |i: usize| b.get(i).and_then(|b| b.frame.passages.first().cloned());
    let same = |i, j| {
        first(i)
            .zip(first(j))
            .is_some_and(|(x, y)| Arc::ptr_eq(&x, &y))
    };
    assert!(same(0, 1) && same(0, 2), "one list, nested one in it");
    assert!(!same(0, 4), "the next list is its own");
}

#[test]
fn list_style_none_drops_the_marker_but_keeps_the_count() {
    let b = read(
        "<ol><li>one</li><li style='list-style: none'>two</li><li>three</li></ol>\
             <ul style='list-style-type:none'><li>bare</li></ul>",
    );
    let markers: Vec<Option<&str>> = b.iter().map(|b| b.frame.marker.as_deref()).collect();
    assert_eq!(markers, [Some("1."), None, Some("3."), None]);
}

#[test]
fn generated_content_counts_quotes_and_reads_attributes() {
    let b = read(
        "<html><head><style>body { counter-reset: ch } h2 { counter-increment: ch } \
             h2::before { content: 'Chapter ' counter(ch, upper-roman) ': ' } \
             a::after { content: ' (' attr(href) ')' } \
             .n::before { counter-increment: note; content: counter(note) '. ' } \
             hr.o { border: none } hr.o::after { content: '\\2766' } \
.star::before { content: '*' } \
             .rule::before { content: ''; display: block; border-top: 1px solid } \
             .fix::after { content: ''; display: table }</style></head><body>\
             <h2>Start</h2><h2>Next</h2><p><a href='x.html'>link</a></p>\
             <p><q>a <q>b</q> c</q></p><p class='n'>one</p><p class='n'>two</p>\
             <hr class='o'/><div class='star'/><div class='rule fix'><p>after</p></div>\
             </body></html>",
    );
    let texts: Vec<String> = b
        .iter()
        .filter_map(text)
        .map(|t| t.text.trim().to_owned())
        .collect();
    assert_eq!(
        texts,
        [
            "Chapter I: Start",
            "Chapter II: Next",
            "link (x.html)",
            "\u{201c}a \u{2018}b\u{2019} c\u{201d}",
            "1. one",
            "2. two",
            "❦",
            "*",
            "after",
        ]
    );
    assert!(!b.iter().any(|b| matches!(b.content, Content::Rule)));
    // The bordered empty box shows as a rule of the book's.
    let ruled = b.iter().rev().nth(1);
    assert!(
        ruled.is_some_and(|b| matches!(b.content, Content::Space(_))),
        "{ruled:?}"
    );
}

#[test]
fn list_styles_markers_and_marker_boxes_show() {
    let b = read(
        "<html><head><style>ol.r { list-style-type: lower-roman } \
             ol.arrow li::marker { content: '→'; color: #c00 } \
             ol.my { list-style: myanmar }</style></head><body>\
             <ol class='r'><li>a</li><li>b</li></ol><ol type='A'><li>c</li></ol>\
             <ul><li style='list-style: square'>d</li><li value='7'>e</li></ul>\
             <ol class='arrow'><li>f</li></ol><ol class='my' start='4'><li>g</li></ol>\
             <ol><li value='9'>h</li><li>i</li></ol></body></html>",
    );
    let markers: Vec<Option<&str>> = b.iter().map(|b| b.frame.marker.as_deref()).collect();
    assert_eq!(
        markers,
        [
            Some("i."),
            Some("ii."),
            Some("A."),
            Some("▪"),
            Some("•"),
            Some("→"),
            Some("၄."),
            Some("9."),
            Some("10.")
        ]
    );
    let arrow = b.get(5).and_then(|b| b.frame.marker_colour);
    assert!(arrow.is_some_and(|[r, g, _]| r > g), "{arrow:?}");
}

#[test]
fn drop_caps_and_first_lines_mark_their_text() {
    let b = read(
        "<html><head><style>p.first::first-letter { float: left; font-size: 3em } \
             p.raised::first-letter { font-size: 2em } \
             p.lead::first-line { font-variant: small-caps }</style></head><body>\
             <p class='first'>\u{201c}Alice was here</p>\
             <p><span style='float: left; font-size: 4em'>T</span>he end</p>\
             <p class='raised'>Raised</p><p class='lead'>Lead line</p></body></html>",
    );
    let t: Vec<&Text> = b.iter().filter_map(text).collect();
    let initial: Vec<usize> = t.iter().map(|t| t.initial).collect();
    assert_eq!(initial, [4, 1, 0, 0]);
    assert_eq!(
        t.first().map(|t| t.text.as_str()),
        Some("\u{201c}Alice was here")
    );
    let size = |i: usize| {
        t.get(i)
            .and_then(|t| t.spans.first())
            .map(|s| s.emphasis.size)
    };
    assert_eq!((size(0), size(2)), (Some(300), Some(200)));
    let lead = t.get(3).and_then(|t| t.first_line);
    assert_eq!(lead.map(|e| e.case), Some(Case::Small));
}

#[test]
fn quotes_indent_with_a_bar_and_soften() {
    let b = read("<blockquote><p>said</p><blockquote>again</blockquote></blockquote>");
    let frames: Vec<(f32, &[f32], bool)> = b
        .iter()
        .map(|b| (b.frame.inset, b.frame.bars.as_slice(), b.frame.soft))
        .collect();
    assert_eq!(
        frames,
        [(1.0, &[0.0][..], true), (2.0, &[0.0, 1.0][..], true)]
    );
}

#[test]
fn preformatted_text_keeps_its_spaces_and_lines() {
    let b = read("<pre>\nfn main() {\n\tgo(<b>1</b>);\n}\n</pre><p>a  b</p>");
    assert_eq!(texts(&b), ["fn main() {\n    go(1);\n}", "a b"]);
    assert_eq!(role(b.first()), Some(Role::Preformatted));
}
