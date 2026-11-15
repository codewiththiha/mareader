use super::*;

impl State<'_> {
    pub(super) fn link(&mut self, tag: &Tag) {
        let rel = |want: &str| {
            attr(tag, "rel").is_some_and(|r| {
                r.split_ascii_whitespace()
                    .any(|w| w.eq_ignore_ascii_case(want))
            })
        };
        // Alternate sheets wait to be picked; `media` must apply.
        let media = attr(tag, "media").is_none_or(query::media);
        let sheet = rel("stylesheet") && !rel("alternate") && media;
        let path = attr(tag, "href").and_then(|h| href::resolve(self.base, h));
        if let (true, Some(path)) = (sheet, path)
            && let Some(sheet) = self.library.stylesheet(&path)
        {
            self.add_sheet(sheet, 0);
        }
    }

    /// Each image is its own block; one that won't load shows its alt.
    pub(super) fn image(
        &mut self,
        src: Option<&str>,
        alt: Option<&str>,
        sized: (Option<Length>, Option<&Computed>),
    ) {
        let picture = src.and_then(|s| self.load(s));
        self.picture(picture, alt, sized);
    }

    /// The picture at `src`, a data URL or a file of the book.
    pub(super) fn load(&mut self, src: &str) -> Option<Arc<Picture>> {
        let src = src.trim();
        if src.starts_with("data:") {
            let bytes = href::data_url(src);
            return bytes.and_then(|b| Picture::probe(b).ok()).map(Arc::new);
        }
        let path = href::resolve(self.base, src)?;
        self.library.picture(&path)
    }

    /// Starts reading an inline drawing at its `svg` or `math` start tag.
    pub(super) fn begin_drawing(&mut self, tag: &Tag) {
        let name: &str = &tag.name;
        let capture = match () {
            _ if tag.self_closing => return,
            _ if svg::is_root(name) => Caught::Svg(Box::new(Capture::new(tag))),
            _ if name.rsplit(':').next() == Some("math") => {
                Caught::Math(Box::new(math::Capture::new(tag)))
            }
            _ => return,
        };
        let open = self.stack.last().filter(|o| o.name == name);
        self.drawing = Some(Drawing {
            capture,
            name: name.to_owned(),
            computed: open.map_or_else(Computed::default, |o| o.computed.clone()),
            shown: open.is_some_and(|o| !o.style.hidden),
            alt: attr(tag, "aria-label").map(str::to_owned),
        });
    }

    /// Feeds a drawing; at its end it is shown as one picture.
    pub(super) fn draw(&mut self, token: &Token) {
        let done = self.drawing.as_mut().is_some_and(|d| match &mut d.capture {
            Caught::Svg(c) => c.token(token),
            Caught::Math(m) => m.token(token),
        });
        let Some(d) = self.drawing.take_if(|_| done) else {
            return;
        };
        // Its place closes with it, but the picture still goes there.
        let places = self.places.clone();
        self.end(&d.name);
        let after = std::mem::replace(&mut self.places, places);
        let svg = match d.capture {
            Caught::Svg(c) => Some(c),
            Caught::Math(m) if d.shown => {
                self.formula(*m, &d.computed);
                None
            }
            Caught::Math(_) => None,
        };
        if let Some(c) = svg.filter(|c| d.shown && !c.broken) {
            let (width, alt) = (picture_width(&d.computed), d.alt.as_deref());
            let c = *c;
            match c.images.as_slice() {
                // A frame round one image, as covers are: that image shows.
                [only] if !c.drawn => self.image(Some(only), alt, (None, None)),
                _ => {
                    let links = svg_links(&mut *self.library, self.base, c.images);
                    let picture = Picture::svg(c.markup.into_bytes(), links).ok();
                    self.picture(picture.map(Arc::new), alt, (width, Some(&d.computed)));
                }
            }
        }
        self.places = after;
        if matches!(token, Token::EOFToken) {
            self.eof();
        }
    }

    /// Sets a `math` element: in its line, or centred as its own block.
    pub(super) fn formula(&mut self, capture: math::Capture, computed: &Computed) {
        if self.top().hidden {
            return;
        }
        let broken = capture.broken;
        let Some(root) = capture.root() else {
            return;
        };
        let block = math::is_block(&root) || computed.display == Display::Block;
        let drawn = (!broken).then(|| math::render(&root, block)).flatten();
        let picture = drawn.as_ref().and_then(|f| {
            let svg = f.svg.as_bytes().to_vec();
            Picture::svg(svg, Links::new()).ok().map(Arc::new)
        });
        let (Some(f), Some(picture)) = (drawn, picture) else {
            return self.formula_alt(&root);
        };
        // A table's cells hold lines only, so display math sits in one.
        if block && self.table.is_none() {
            self.finish();
            let mut b = Block::new(Content::Image {
                picture,
                cover: false,
                width: Some(Length::Em(f.width)),
                links: Vec::new(),
            });
            b.frame = self.frame();
            b.frame.align = Align::Center;
            b.frame.hidden = computed.invisible;
            self.emit(b);
            return;
        }
        if self.target().is_none_or(|t| t.inlines.len() >= MAX_INLINES) {
            return self.formula_alt(&root);
        }
        let hidden = computed.invisible;
        let link = self.top().link.filter(|_| !hidden);
        let space = std::mem::take(&mut self.space);
        let Some(t) = self.target() else {
            return;
        };
        if space && !t.text.is_empty() && !t.text.ends_with('\n') {
            t.push(" ", Emphasis::default());
        }
        t.inlines.push(Inline {
            at: t.text.len(),
            picture,
            width: f.width,
            height: f.height,
            rise: Rise::By(-f.depth),
            link,
            hidden,
        });
    }

    /// A formula that can't be drawn: its own picture, else its text.
    pub(super) fn formula_alt(&mut self, root: &math::Node) {
        let alt = math::alt(root);
        if let Some(src) = root.attr("altimg") {
            let src = src.to_owned();
            return self.image(Some(&src), alt.as_deref(), (None, None));
        }
        let space = std::mem::take(&mut self.space);
        if let (Some(text), Some(t)) = (alt, self.target()) {
            if space && !t.text.is_empty() && !t.text.ends_with('\n') {
                t.push(" ", Emphasis::default());
            }
            t.push(&text, Emphasis::default());
        }
    }
}
