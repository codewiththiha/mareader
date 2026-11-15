use super::*;

#[test]
fn emphasis_marks_spans() {
    let b = read("<p>a <em>b <strong>c</strong></em> <code>d</code></p>");
    assert_eq!(texts(&b), ["a b c d"]);
    let e = |italic, bold, code| Emphasis {
        italic,
        bold,
        code,
        ..Emphasis::default()
    };
    // A collapsed space joins the run after it.
    assert_eq!(
        spans(b.first()),
        [
            (" b", e(true, false, false)),
            (" c", e(true, true, false)),
            (" d", e(false, false, true))
        ]
    );
}

#[test]
fn self_closing_tags_open_nothing() {
    let b = read(
        "<head><script src='x.js'/><link rel='stylesheet'/></head>\
             <p><a id='n1'/>Kept <em/>plain.</p>",
    );
    assert_eq!(texts(&b), ["Kept plain."]);
    assert!(spans(b.first()).is_empty());
}

#[test]
fn unclosed_and_stray_tags_are_tolerated() {
    let b = read("<p>one<p>two</em></b><div>three</span>");
    assert_eq!(texts(&b), ["one", "two", "three"]);
}

#[test]
fn no_break_spaces_are_kept() {
    let b = read("<p>a&nbsp;&nbsp;b   c</p>");
    assert_eq!(texts(&b), ["a\u{a0}\u{a0}b c"]);
}

#[test]
fn line_breaks_survive_collapsing() {
    let b = read("<p>line one<br/>\n  line two</p>");
    assert_eq!(texts(&b), ["line one\nline two"]);
}

#[test]
fn deep_nesting_is_read_flat() {
    let deep = "<em>".repeat(10_000) + "x" + &"</em>".repeat(10_000);
    let b = read(&format!("<p>{deep}</p>"));
    assert_eq!(texts(&b), ["x"]);
}

#[test]
fn hidden_text_stays_hidden() {
    // A small drawing joins its line; its words aren't the text's.
    let b = read(
        "<p>a<svg width='16' height='16'><text>no</text></svg>b\
             <script>if (a<b) {}</script></p>",
    );
    assert_eq!(texts(&b), ["ab"]);
    assert_eq!(b.first().and_then(text).map(|t| t.inlines.len()), Some(1));
}
