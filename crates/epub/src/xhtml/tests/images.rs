use super::*;

#[test]
fn inline_svg_is_drawn_as_one_picture() {
    let mut shelf = Shelf::default();
    let dot = Arc::new(b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec());
    shelf.files.insert("/OEBPS/images/dot.svg".into(), dot);
    let b = read_with(
        &mut shelf,
        "<p>Before</p>\
             <svg viewBox='0 0 400 100' epub:type='ornament' aria-label='Leaf'>\
             <style><![CDATA[ .a { fill: black } ]]></style>\
             <lineargradient id='g'/><path class='a' d='M0 0L400 100'/>\
             <text>No words</text><image xlink:href='../images/dot.svg'/></svg>\
             <p>After</p>",
    );
    assert_eq!(kinds(&b), ["text", "image", "text"]);
    assert_eq!(texts(&b), ["Before", "After"]);
    let p = picture_of(&b, 1);
    assert!(p.as_ref().is_some_and(|p| p.vector()));
    assert_eq!(p.map(|p| (p.width, p.height)), Some((400, 100)));
}

#[test]
fn a_frame_round_one_image_shows_that_image() {
    let mut shelf = Shelf::default();
    if let Some(p) = png(4, 2) {
        shelf.pictures.insert("/OEBPS/images/c.png".into(), p);
    }
    let b = read_with(
        &mut shelf,
        "<div><svg:svg xmlns:svg='http://www.w3.org/2000/svg' viewBox='0 0 4 2'>\
             <svg:image xlink:href='../images/c.png'/></svg:svg></div>",
    );
    let p = picture_of(&b, 0);
    assert!(p.as_ref().is_some_and(|p| !p.vector() && p.width == 4));
    // Hidden drawings and their text show nothing.
    let none = read("<svg style='display:none'><path d='M0 0'/><text>x</text></svg>");
    assert!(none.is_empty(), "{none:?}");
}

#[test]
fn transparent_borders_keep_their_room_but_draw_nothing() {
    let b =
        read("<div style='border: 4px solid transparent; border-top-color: red'><p>a</p></div>");
    let spec = b.first().and_then(|b| b.frame.boxes.first());
    let edges = spec.map(|b| b.edges);
    assert_eq!(
        edges,
        Some([Edge::Solid, Edge::Clear, Edge::Clear, Edge::Clear])
    );
    let room = 4.0 / ROOT_PX;
    assert!(spec.is_some_and(|b| b.border.iter().all(|w| (w - room).abs() < 1e-3)));
}

#[test]
fn a_book_drawn_hr_is_a_box_and_a_plain_one_a_rule() {
    let b = read(
        "<html><head><style>hr.s { border: 0; border-top: 3px double; width: 30%; \
             margin: 1em auto } hr.g { border: none; height: 2px; \
             background: linear-gradient(to right, transparent, #000, transparent) }\
             </style></head><body><p>a</p><hr/><hr class='s'/><hr class='g'/><p>b</p>\
             </body></html>",
    );
    assert_eq!(kinds(&b), ["text", "rule", "space", "space", "text"]);
    let boxed = |i: usize| b.get(i).and_then(|b| b.frame.boxes.first());
    assert!(boxed(2).is_some_and(|x| x.rule
        && x.edges[0] == Edge::Double
        && x.width == Some(Length::Percent(30.0))));
    // Empty, its gradient shows at full strength.
    let ramp = boxed(3).and_then(|x| x.finish.clone());
    let stops = ramp.and_then(|f| match f.layers.first() {
        Some(mareader_reflow::Layer::Gradient(g)) => Some(g.stops.clone()),
        _ => None,
    });
    assert!(stops.is_some_and(|s| s.len() == 3 && s.get(1).is_some_and(|s| s.1 > 0.9)));
}

#[test]
fn boxes_carry_shadows_gradients_outlines_and_border_pictures() {
    let svg = "data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' \
                   width='90' height='90'%3E%3C/svg%3E";
    let css = format!(
        ".f {{ box-shadow: 2px 2px 4px rgba(0,0,0,.5); background: \
             linear-gradient(to right, #000, transparent), url(\"{svg}\"); \
             outline: 1px dashed; outline-offset: 2px; border: 30px solid transparent; \
             border-image: url(\"{svg}\") 30 round; opacity: .5 }}"
    );
    let b = read(&format!(
        "<html><head><style>{css}</style></head><body>\
             <div class='f'><p>a</p></div></body></html>"
    ));
    let spec = b.first().and_then(|b| b.frame.boxes.first()).cloned();
    assert_eq!(spec.as_ref().map(|s| s.edges), Some([Edge::Clear; 4]));
    let f = spec.and_then(|s| s.finish.clone());
    let Some(f) = f else {
        return assert!(b.is_empty(), "no finish");
    };
    let shadow = f.shadows.first().copied().unwrap_or_default();
    assert!((shadow.strength - 0.5).abs() < 0.02 && (shadow.at[0] - 0.125).abs() < 1e-3);
    assert!(matches!(f.layers.as_slice(), [
            mareader_reflow::Layer::Gradient(g),
            mareader_reflow::Layer::Picture(_),
        ] if g.stops.iter().all(|s| s.1 <= MAX_TEXT_FILL)));
    let outline = f.outline.map(|o| (o.edge, o.width, o.offset));
    assert_eq!(outline, Some((Edge::Dashed, 1.0 / ROOT_PX, 2.0 / ROOT_PX)));
    let frame = f.frame.as_ref().map(|r| (r.slice, r.width, r.tile, r.fill));
    let third = 30.0 / 90.0;
    assert_eq!(
        frame,
        Some((
            [third; 4],
            [30.0 / ROOT_PX; 4],
            [mareader_reflow::Tile::Round; 2],
            false
        ))
    );
    assert!((f.opacity - 0.5).abs() < 1e-3);
}

#[test]
fn carved_borders_keep_their_style() {
    let b = read("<div style='border: 3px groove; border-left-style: outset'><p>a</p></div>");
    let edges = b
        .first()
        .and_then(|b| b.frame.boxes.first())
        .map(|b| b.edges);
    assert_eq!(
        edges,
        Some([Edge::Groove, Edge::Groove, Edge::Groove, Edge::Outset])
    );
}

#[test]
fn text_shadow_and_opacity_reach_the_text() {
    let b = read(
        "<p>a <span style='text-shadow: 1px 1px #000'>b</span> \
             <span style='opacity: 0.5'>c</span> \
             <span style='text-shadow: 0 0 3px #fff'>d</span></p>",
    );
    let spans = b.first().and_then(text).map_or(Vec::new(), |t| {
        t.spans.iter().map(|s| s.emphasis).collect::<Vec<_>>()
    });
    // A white shadow on white paper is nothing to draw.
    let shadows: Vec<_> = spans.iter().filter_map(|e| e.shadow).collect();
    let one = TextShadow {
        at: [6, 6],
        blur: 0,
        strength: 255,
    };
    assert_eq!(shadows, [one], "{spans:?}");
    let grey = |e: &Emphasis| e.colour.is_some_and(|[r, ..]| r > 60 && r < 200);
    assert!(spans.iter().any(grey), "{spans:?}");
}

#[test]
fn hidden_text_keeps_its_room_and_filters_fade_boxes() {
    let b = read(
        "<div style='visibility: hidden; border: 1px solid'><p>a \
             <span style='visibility: visible'>b</span></p></div>\
             <div style='filter: opacity(50%) drop-shadow(1px 1px 2px #000); \
             border: 1px solid'><p>c</p></div>",
    );
    let hidden = b.first().and_then(text).map_or(Vec::new(), |t| {
        let spans = t.spans.iter().filter(|s| s.emphasis.hidden);
        let words = spans.filter_map(|s| t.text.get(s.range.clone()));
        words.map(|w| w.trim().to_owned()).collect::<Vec<_>>()
    });
    assert_eq!(hidden, ["a"]);
    let finish = |i: usize| {
        let spec = b.get(i).and_then(|b| b.frame.boxes.first());
        spec.and_then(|s| s.finish.clone())
    };
    assert!(finish(0).is_some_and(|f| f.opacity == 0.0));
    let faded = finish(1);
    assert!(faded.is_some_and(|f| (f.opacity - 0.5).abs() < 1e-3 && f.shadows.len() == 1));
}

#[test]
fn background_pictures_may_be_data_urls() {
    let svg = "data:image/svg+xml;utf8,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 \
                   width=%2220%22 height=%2220%22%3E%3C/svg%3E";
    let b = read(&format!(
        "<div style=\"background: url('{svg}') no-repeat; padding: 1em\"><p>a</p></div>"
    ));
    let spec = b.first().and_then(|b| b.frame.boxes.first());
    let size = spec
        .and_then(|s| s.backdrop.as_ref())
        .map(|d| d.picture.width);
    assert_eq!(size, Some(20));
}

#[test]
fn data_urls_are_pictures() {
    let svg = "data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' \
                   width='30' height='10'%3E%3C/svg%3E";
    let b = read(&format!("<p>a</p><img src=\"{svg}\"/>"));
    assert_eq!(picture_of(&b, 1).map(|p| p.width), Some(30));
    // A 1 × 1 PNG, base64.
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAf\
                   FcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    let b = read(&format!("<p>a</p><img src='{png}'/>"));
    assert_eq!(picture_of(&b, 1).map(|p| p.width), Some(1));
}

#[test]
fn an_svg_page_fills_its_page() {
    let mut shelf = Shelf::default();
    let page = b"<svg xmlns='http://www.w3.org/2000/svg' width='600' height='800'/>";
    let picture = Picture::probe(page.to_vec()).ok().map(Arc::new);
    if let Some(p) = picture {
        shelf.pictures.insert("/OPS/p1.svg".into(), p);
    }
    let (mut out, mut refs) = (Vec::new(), Refs::default());
    read_svg_page("/OPS/p1.svg", &mut shelf, (&mut out, &mut refs), false);
    let cover = matches!(
        out.first().map(|b| &b.content),
        Some(Content::Image { cover: true, .. })
    );
    assert!(cover && out.first().is_some_and(|b| b.break_before));
    assert_eq!(refs.place("/OPS/p1.svg").map(|a| a.block), Some(0));
}
