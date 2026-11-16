use super::*;

const ITALIC: Emphasis = Emphasis {
    italic: true,
    bold: false,
    light: false,
    code: false,
    size: 0,
    script: Script::Normal,
    case: Case::Normal,
    underline: false,
    strike: false,
    colour: None,
    shadow: None,
    hidden: false,
    features: Features { on: 0, off: 0 },
    face: 0,
};

#[test]
fn neighbouring_runs_of_one_emphasis_share_a_span() {
    let mut b = Text::new(Role::Paragraph);
    b.push("a ", Emphasis::default());
    b.push("bc", ITALIC);
    b.push("d", ITALIC);
    b.push(" e", Emphasis::default());
    assert_eq!(b.text, "a bcd e");
    assert_eq!(
        b.spans,
        [Span {
            range: 2..5,
            emphasis: ITALIC
        }]
    );
}

#[test]
fn trimming_clips_and_drops_spans() {
    let mut b = Text::new(Role::Paragraph);
    b.push("a", Emphasis::default());
    b.push(" b ", ITALIC);
    b.push("  ", ITALIC);
    b.trim_end();
    assert_eq!(b.text, "a b");
    assert_eq!(b.spans.len(), 1);
    assert_eq!(b.spans.first().map(|s| s.range.clone()), Some(1..3));
}

#[test]
fn tables_of_blank_cells_are_blank() {
    let row = |s: &str| Row {
        cells: vec![{
            let mut t = Text::new(Role::Paragraph);
            t.push(s, Emphasis::default());
            t
        }],
        head: false,
    };
    let table = |rows| Block::new(Content::Table(Table { rows, rule: None }));
    assert!(table(vec![row(" ")]).is_blank());
    assert!(table(vec![]).is_blank());
    assert!(!table(vec![row(" "), row("x")]).is_blank());
    assert!(!Block::new(Content::Rule).is_blank());
}

#[test]
fn each_paragraph_and_cell_costs_a_layout_plus_its_text() {
    let para = Block::text(Role::Paragraph, "four");
    assert_eq!(para.layout_cost(), LAYOUT_BYTES + 4 * CHAR_BYTES);
    let cell = |s: &str| {
        let mut t = Text::new(Role::Paragraph);
        t.push(s, Emphasis::default());
        t
    };
    let row = Row {
        cells: vec![cell("ab"), cell("")],
        head: false,
    };
    let table = Block::new(Content::Table(Table {
        rows: vec![row],
        rule: None,
    }));
    assert_eq!(table.layout_cost(), 2 * LAYOUT_BYTES + 2 * CHAR_BYTES);
    assert_eq!(Block::new(Content::Rule).layout_cost(), 0);
}

#[test]
fn cubic_bezier_matches_endpoints_and_monotonic_midpoint() {
    use crate::anim::Easing;
    assert_eq!(Easing::EASE_IN_OUT.eval(0.0), 0.0);
    assert_eq!(Easing::EASE_IN_OUT.eval(1.0), 1.0);
    let mid = Easing::EASE_IN_OUT.eval(0.5);
    assert!((mid - 0.5).abs() < 0.02, "mid={mid}");
    assert!(Easing::EASE_IN.eval(0.5) < 0.4);
    assert!(Easing::EASE_OUT.eval(0.5) > 0.6);
}

#[test]
fn keyframe_animation_interpolates_and_alternates() {
    use crate::anim::{AnimationSpec, Easing, KeyframeStop, Transform2d};
    let anim = AnimationSpec {
        duration_ms: 1000,
        delay_ms: 0,
        easing: Easing::Linear,
        iterations: 2.0,
        alternate: true,
        reverse: false,
        forwards: true,
        stops: std::sync::Arc::from([
            KeyframeStop {
                at: 0.0,
                transform: Some(Transform2d::IDENTITY),
                opacity: Some(0.0),
                fill: None,
            },
            KeyframeStop {
                at: 1.0,
                transform: Some(Transform2d {
                    scale: [1.5, 1.5],
                    ..Transform2d::IDENTITY
                }),
                opacity: Some(1.0),
                fill: None,
            },
        ]),
    };
    let half = anim.sample(0.5);
    assert!(half.active);
    assert!((half.opacity.unwrap_or_default() - 0.5).abs() < 1e-3);
    let rev = anim.sample(1.25);
    assert!(rev.active);
    assert!((rev.opacity.unwrap_or_default() - 0.75).abs() < 1e-3);
    let end = anim.sample(2.5);
    assert!(!end.active);
    assert!((end.opacity.unwrap_or_default() - 0.0).abs() < 1e-3);
}
