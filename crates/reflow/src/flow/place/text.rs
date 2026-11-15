use super::*;

pub(super) fn spans(b: &mut RangedBuilder<'_, Ink>, text: &Text, style: Style<'_>, ink: Ink) {
    for span in &text.spans {
        let own = span_style(b, span, style);
        span_brush(b, span, style, own, ink);
    }
}

fn span_style<'a>(
    b: &mut RangedBuilder<'_, Ink>,
    span: &crate::block::Span,
    style: Style<'a>,
) -> Option<&'a [FontFamilyName<'static>]> {
    let range = span.range.clone();
    let Emphasis {
        italic,
        bold,
        light,
        code: _,
        size,
        script,
        case: _,
        underline,
        strike,
        colour: _,
        shadow: _,
        hidden: _,
        features,
        face,
    } = span.emphasis;
    if !features.is_empty() && !style.turned {
        let source = FontFeatures::Source(Cow::Owned(features.source()));
        b.push(StyleProperty::FontFeatures(source), range.clone());
    }
    if script != Script::Normal {
        let shift = match script {
            Script::Sub => VerticalAlign::SUB,
            _ => VerticalAlign::SUPER,
        };
        b.push(StyleProperty::VerticalAlign(shift), range.clone());
        if !style.sized {
            let size = StyleProperty::FontSize(style.size * SCRIPT_SCALE);
            b.push(size, range.clone());
        }
    }
    if underline {
        b.push(StyleProperty::Underline(true), range.clone());
    }
    if strike {
        b.push(StyleProperty::Strikethrough(true), range.clone());
    }
    if size != 0 && style.sized {
        let size = (style.size * f32::from(size) / 100.0).max(style.floor);
        let size = StyleProperty::FontSize(size.clamp(MIN_SIZE, MAX_SIZE));
        b.push(size, range.clone());
    }
    if italic {
        b.push(StyleProperty::FontStyle(FontStyle::Italic), range.clone());
    }
    if bold {
        let weight = FontWeight::new(bolder(style.weight));
        b.push(StyleProperty::FontWeight(weight), range.clone());
    } else if light {
        let weight = FontWeight::new((style.weight - LIGHTER).max(100.0));
        b.push(StyleProperty::FontWeight(weight), range.clone());
    }
    let own = usize::from(face).checked_sub(1);
    let own = own.and_then(|i| style.runs.get(i)).map(Vec::as_slice);
    if let Some(stack) = own {
        let family = FontFamily::List(Cow::Borrowed(stack));
        b.push(StyleProperty::FontFamily(family), range.clone());
    }
    own
}

fn span_brush(
    b: &mut RangedBuilder<'_, Ink>,
    span: &crate::block::Span,
    style: Style<'_>,
    own: Option<&[FontFamilyName<'static>]>,
    ink: Ink,
) {
    let range = span.range.clone();
    let Emphasis {
        code,
        colour,
        shadow,
        hidden,
        ..
    } = span.emphasis;
    if code && !style.pre {
        let mono = FontFamily::List(Cow::Borrowed(own.unwrap_or(style.mono)));
        b.push(StyleProperty::FontFamily(mono), range.clone());
        b.push(
            StyleProperty::FontSize(style.size * CODE_SCALE),
            range.clone(),
        );
        b.push(
            StyleProperty::Brush(Ink {
                code: true,
                colour,
                shadow,
                hidden,
                ..ink
            }),
            range,
        );
    } else if colour.is_some() || shadow.is_some() || hidden {
        let brush = Ink {
            colour,
            shadow,
            hidden,
            ..ink
        };
        b.push(StyleProperty::Brush(brush), range);
    }
}

pub(super) fn small_caps(
    b: &mut RangedBuilder<'_, Ink>,
    small: Vec<(Range<usize>, f32)>,
    style: Style<'_>,
    text: &Text,
) {
    for (range, scale) in small {
        let size = style.size * scale * SMALL_CAPS;
        b.push(
            StyleProperty::FontSize(size.clamp(MIN_SIZE, MAX_SIZE)),
            range,
        );
    }
    for link in &text.links {
        b.push(StyleProperty::Underline(true), link.range.clone());
    }
}

pub(super) fn inlines(b: &mut RangedBuilder<'_, Ink>, text: &Text, style: Style<'_>, shown: &str) {
    // A drop cap's letter keeps its place in the text but shows apart.
    let initial = text.initial.min(shown.len());
    if style.sized && initial > 0 && shown.is_char_boundary(initial) {
        b.push(StyleProperty::FontSize(HIDDEN_SIZE), 0..initial);
    }
    for (n, inline) in text.inlines.iter().enumerate().take(MAX_INLINES) {
        let size = |ems: f32| (ems * style.size).clamp(1.0, MAX_SIZE * 8.0);
        // Vertical: a picture lies on its side, to be drawn standing.
        let (width, height) = match style.turned {
            true => (inline.height, inline.width),
            false => (inline.width, inline.height),
        };
        b.push_inline_box(InlineBox {
            id: n as u64,
            kind: InlineBoxKind::InFlow,
            index: inline.at.min(shown.len()),
            width: size(width),
            height: size(height),
            baseline: None,
            vertical_align: rise(inline.rise, style.size),
        });
    }
}

pub(super) fn finish(
    b: RangedBuilder<'_, Ink>,
    shown: &str,
    text: &Text,
    style: Style<'_>,
    hyphenated: Option<&hyphen::Hyphenated>,
    (width, beside): (Option<f32>, Option<Beside>),
    (upright, combine): (Vec<Range<usize>>, Vec<Range<usize>>),
) -> Para {
    let mut layout = b.build(shown);
    if style.indent > 0.0 {
        layout.set_text_indent(style.indent, IndentOptions::default());
    }
    match (width, beside) {
        (Some(width), Some(beside)) => break_beside(&mut layout, width, beside),
        _ => layout.break_all_lines(width),
    }
    let last = style.last.map(|last| match last {
        LastLine::Start => Alignment::Start,
        LastLine::Center => Alignment::Center,
        LastLine::End => Alignment::End,
        LastLine::Justify => Alignment::Justify,
    });
    let options = AlignmentOptions {
        last_line_alignment: last,
        ..AlignmentOptions::default()
    };
    layout.align(alignment(&style), options);
    let ends_soft = |end: usize| text.text.get(..end).is_some_and(|s| s.ends_with(SHY));
    let hyphens = layout
        .lines()
        .enumerate()
        .filter(|(_, l)| hyphenated.is_some() && ends_soft(l.text_range().end))
        .map(|(i, _)| i)
        .collect();
    let starts = layout
        .lines()
        .map(|l| {
            let start = l.text_range().start;
            hyphenated.map_or(start, |h| h.original(start))
        })
        .collect();
    Para {
        layout,
        text: text.text.clone(),
        hyphens,
        marks: hyphenated
            .as_ref()
            .map(|h| h.marks.clone())
            .unwrap_or_default(),
        starts,
        links: text.links.to_vec(),
        turned: style.turned,
        upright,
        combine,
        ruby: (text.ruby.iter())
            .map(|a| Note {
                range: a.range.clone(),
                text: a.text.clone(),
                para: None,
            })
            .collect(),
        pictures: text
            .inlines
            .iter()
            .map(|i| Arc::clone(&i.picture))
            .collect(),
        picture_links: text.inlines.iter().map(|i| i.link).collect(),
        veiled: text.inlines.iter().map(|i| i.hidden).collect(),
    }
}
