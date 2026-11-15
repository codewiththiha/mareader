use super::*;

#[test]
fn text_wraps_past_a_float_then_takes_the_column() {
    let f = flow(&[floated(para(1), false), designed(para(300), &[], None)]);
    let (float, text) = (f.blocks.first(), f.blocks.get(1));
    let fw = float.map_or(f32::NAN, |s| s.width);
    assert!(fw < f.width / 4.0, "{fw}");
    assert!(float.is_some_and(|s| s.float.is_some()));
    assert!(text.is_some_and(|s| s.beside.is_some()));
    let ((a, a_end), (b, _)) = (span(&f, 0, 0), span(&f, 0, 1));
    assert!((a - b).abs() < 0.5, "{a} {b}");
    let lefts = line_lefts(&f, 1);
    assert!(lefts.first().is_some_and(|x| *x >= fw), "{lefts:?}");
    assert!(lefts.last().is_some_and(|x| *x == 0.0), "{lefts:?}");
    // Lines beside the float are the float's height, no more.
    let beside = lefts.iter().take_while(|x| **x > 0.0).count();
    let line = text.map_or(0.0, |s| span_height(s, 0..1));
    assert!(beside as f32 * line <= a_end - a + line + 0.5, "{beside}");
}

#[test]
fn a_right_float_keeps_text_to_its_left() {
    let f = flow(&[floated(para(1), true), designed(para(80), &[], None)]);
    let float = f.blocks.first().map_or((0.0, 0.0), |s| (s.x, s.width));
    assert!((float.0 + float.1 - f.width).abs() < 0.5, "{float:?}");
    assert!(line_lefts(&f, 1).iter().all(|x| *x == 0.0));
    let widths: Vec<f32> = match &*f.laid(1) {
        Laid::Text { para, .. } => para.layout.lines().map(|l| l.metrics().advance).collect(),
        _ => Vec::new(),
    };
    assert!(
        widths.first().is_some_and(|w| *w <= float.0 + 0.5),
        "{widths:?}"
    );
}

#[test]
fn a_picture_after_a_float_starts_below_it() {
    let mut picture = image(400, 200, false);
    picture.frame.look = Some(Arc::new(Look {
        designed: true,
        ..Look::default()
    }));
    let f = flow(&[floated(para(1), false), picture]);
    assert!(f.blocks.get(1).is_some_and(|s| s.clear));
    let ((_, float_end), (top, _)) = (span(&f, 0, 0), span(&f, 0, 1));
    assert!(top >= float_end - 0.5, "{float_end} {top}");
}

#[test]
fn a_set_height_holds_its_box_open() {
    let b = Arc::new(Boxed {
        height: Some(10.0),
        fill: Some(0.1),
        ..Boxed::default()
    });
    let f = flow(&[designed(para(1), &[b], None)]);
    let em = design_em(&f);
    let deco = f.decos.first().and_then(|d| d.first());
    assert!(deco.is_some_and(|d| d.rect[3] >= 10.0 * em - 0.5 && d.fill == Some(0.1)));
}

#[test]
fn a_drop_cap_spans_lines_with_the_text_beside_it() {
    let mut t = Text::new(Role::Paragraph);
    let big = Emphasis {
        size: 300,
        ..Emphasis::default()
    };
    t.push("A", big);
    let rest = "lice was beginning to get very tired of sitting by her sister ";
    t.push(&rest.repeat(6), Emphasis::default());
    t.initial = 1;
    let mut b = Block::new(Content::Text(t));
    b.frame.look = Some(Arc::new(Look::default()));
    let f = flow(&[b.clone()]);
    let line = match &*f.laid(0) {
        Laid::Text {
            initial: Some(i), ..
        } => i.line,
        _ => 0,
    };
    assert_eq!(line, 2);
    let lefts = line_lefts(&f, 0);
    assert!(lefts.iter().take(2).all(|x| *x > 17.0), "{lefts:?}");
    assert!(lefts.get(2).is_some_and(|x| *x == 0.0), "{lefts:?}");
    // Read as the reader's own, the letter stays in its line.
    b.frame.look = None;
    let plain = flow(&[b]);
    assert!(matches!(&*plain.laid(0), Laid::Text { initial: None, .. }));
}

#[test]
fn marker_pictures_and_first_lines_lay_out() {
    let mut item = para(3);
    item.frame.marker = Some("".into());
    item.frame.marker_picture = Picture::probe(png(32, 32)).ok().map(Arc::new);
    let f = flow(&[item]);
    let pictures = match &*f.laid(0) {
        Laid::Text {
            marker: Some(m), ..
        } => m.pictures.len(),
        _ => 0,
    };
    assert_eq!(pictures, 1);
    let mut t = Text::new(Role::Paragraph);
    t.push("one ", Emphasis::default());
    let bold = Emphasis {
        bold: true,
        ..Emphasis::default()
    };
    t.push("two three", bold);
    let upper = Emphasis {
        case: Case::Upper,
        ..Emphasis::default()
    };
    let styled = over_first_line(&t, 7, upper);
    let spans: Vec<(Range<usize>, bool, Case)> = (styled.spans.iter())
        .map(|s| (s.range.clone(), s.emphasis.bold, s.emphasis.case))
        .collect();
    let want = [
        (0..4, false, Case::Upper),
        (4..7, true, Case::Upper),
        (7..13, true, Case::Normal),
    ];
    assert_eq!(spans, want);
    assert_eq!(styled.text, t.text);
}
