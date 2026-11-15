use super::*;

impl State<'_> {
    /// Opens a fixed page at `<body>`: its viewport and root ground.
    pub(super) fn open_sheet(&mut self, body: &Computed) {
        let sized = |l: Len| l.px(ROOT_PX, 0.0).filter(|v| *v >= 1.0);
        let own = sized(body.width).zip(sized(body.height));
        let (width, height) = self.viewport.or(own).unwrap_or(DEFAULT_SHEET);
        let root = self.stack.first().map(|o| o.computed.clone());
        let paints = |p: &(Option<f32>, Option<Arc<Backdrop>>, Option<Finish>)| {
            p.0.is_some() || p.1.is_some() || p.2.as_ref().is_some_and(Finish::shows)
        };
        let from_root = root.map(|c| self.paint(&c)).filter(paints);
        self.grounded = from_root.is_none();
        let (fill, backdrop, finish) = match from_root {
            Some(p) => p,
            None => self.paint(body),
        };
        let ground = Boxed {
            fill: fill.map(|c| c.min(MAX_TEXT_FILL)),
            backdrop,
            finish: finish.map(|f| Arc::new(capped(&f))),
            ..Boxed::default()
        };
        let shows = ground.fill.is_some() || ground.backdrop.is_some() || ground.finish.is_some();
        self.sheet = Some(Arc::new(Sheet {
            width,
            height,
            ground: shows.then(|| Arc::new(ground)),
        }));
        self.places.push(Place {
            depth: 0,
            rect: [0.0, 0.0, width, height],
            spot: None,
        });
    }

    /// Opens the containing block `position` makes; true when out of flow.
    pub(super) fn place_start(&mut self, c: &Computed) -> bool {
        let Some(sheet) = self.sheet.clone().filter(|_| self.fixed) else {
            return false;
        };
        let Some(outer) = self.places.last().cloned() else {
            return false;
        };
        if c.placing == Placing::Static || self.table.is_some() {
            return false;
        }
        let view = (sheet.width, sheet.height);
        let [cx, cy, cw, ch] = match c.placing {
            Placing::Fixed => [0.0, 0.0, view.0, view.1],
            _ => outer.rect,
        };
        let at = |l: Len, base: f32| placed_px(l, base, view);
        let side = |i: usize, base: f32| c.offsets.get(i).and_then(|l| at(*l, base));
        let (top, right, bottom, left) = (side(0, ch), side(1, cw), side(2, ch), side(3, cw));
        let depth = self.stack.len();
        if c.placing == Placing::Relative {
            // In the flow still: only what it holds is placed from it.
            let x = cx + left.or(right.map(|r| -r)).unwrap_or(0.0);
            let y = cy + top.or(bottom.map(|b| -b)).unwrap_or(0.0);
            let (w, h) = (
                at(c.width, cw).unwrap_or(cw),
                at(c.height, ch).unwrap_or(ch),
            );
            let spot = outer.spot;
            self.places.push(Place {
                depth,
                rect: [x, y, w, h],
                spot,
            });
            return false;
        }
        let margin =
            |i: usize, base: f32| c.margin.get(i).and_then(|m| at(*m, base)).unwrap_or(0.0);
        let (m_top, m_right, m_bottom, m_left) =
            (margin(0, cw), margin(1, cw), margin(2, cw), margin(3, cw));
        let across = c.padding[1] + c.padding[3] + c.border[1] + c.border[3];
        let down = c.padding[0] + c.padding[2] + c.border[0] + c.border[2];
        let room = |l: Option<f32>, r: Option<f32>| {
            cw - l.unwrap_or(0.0) - r.unwrap_or(0.0) - m_left - m_right
        };
        let width = at(c.width, cw)
            .map_or_else(|| room(left, right), |w| w + across)
            .max(1.0);
        let x = match (left, right) {
            (Some(l), _) => cx + l + m_left,
            (None, Some(r)) => cx + cw - r - m_right - width,
            (None, None) => cx + m_left,
        };
        let height = at(c.height, ch).map(|h| h + down);
        let height = height.or_else(|| top.zip(bottom).map(|(t, b)| ch - t - b - m_top - m_bottom));
        let height = height.map(|h| h.max(1.0));
        let (y, hung) = match (top, bottom, height) {
            (Some(t), _, _) => (cy + t + m_top, None),
            (None, Some(b), Some(h)) => (cy + ch - b - m_bottom - h, None),
            (None, Some(b), None) => (cy, Some(cy + ch - b - m_bottom)),
            (None, None, _) => (cy + m_top, None),
        };
        let spot = Spot {
            x,
            y,
            width,
            height,
            bottom: hung,
            z: c.z_index,
            depth: u8::try_from(self.boxes.len()).unwrap_or(u8::MAX),
        };
        // What came before stays in the flow; this starts its own blocks.
        self.finish();
        self.gap = Gap::default();
        self.places.push(Place {
            depth,
            rect: [x, y, width, height.unwrap_or(ch)],
            spot: Some(Arc::new(spot)),
        });
        true
    }

    /// Closes the containing blocks of elements at depth `at` and in.
    pub(super) fn place_end(&mut self, at: usize) {
        let keep = self
            .places
            .iter()
            .take_while(|p| p.depth < at || p.depth == 0);
        let keep = keep.count();
        self.places.truncate(keep);
    }

    /// Shows `picture` in a line or as its own block, or else its alt.
    pub(super) fn picture(
        &mut self,
        picture: Option<Arc<Picture>>,
        alt: Option<&str>,
        sized: (Option<Length>, Option<&Computed>),
    ) {
        let (width, computed) = sized;
        let inline_link = self.inline_link.take();
        if self.top().hidden {
            return;
        }
        let parent = self.stack.last().is_some_and(|o| o.computed.invisible);
        let invisible = computed.map_or(parent, |c| c.invisible);
        let alt = alt.map(str::trim).filter(|a| !a.is_empty());
        let unseen = Emphasis {
            hidden: invisible,
            ..Emphasis::default()
        };
        if self.table.is_some() {
            if let (Some(alt), Some(cell)) = (alt, self.target()) {
                cell.push(alt, unseen);
            }
            return;
        }
        let small = picture
            .as_ref()
            .zip(computed)
            .and_then(|(p, c)| inline_size(p, c));
        let after_text = self.current.as_ref().is_some_and(|b| match &b.content {
            Content::Text(t) => !t.text.trim().is_empty(),
            _ => false,
        });
        let parent = self.stack.last().map(|o| o.name.as_str());
        let small = small.filter(|_| after_text || parent.is_some_and(in_line));
        let depth = self.stack.len();
        let own = |p: &Place| p.depth + 1 >= depth && p.spot.is_some();
        let small = small.filter(|_| !self.places.last().is_some_and(own));
        if let (Some(picture), Some((width, height))) = (&picture, small)
            && self.inline_picture(picture, width, height, computed, invisible, inline_link)
        {
            return;
        }
        self.finish();
        let mut links = std::mem::take(&mut self.areas);
        links.extend(self.top().link.map(Area::whole));
        if invisible {
            links.clear();
        }
        if let Some(picture) = picture {
            let mut block = Block::new(Content::Image {
                picture,
                cover: false,
                width,
                links,
            });
            block.frame = self.frame();
            block.frame.hidden = invisible;
            // A grid's portrait sits over its centred name, read in a row.
            if self.laying > 0 && self.top().align == Align::Start {
                block.frame.align = Align::Center;
            }
            // A column stack's picture takes the place it sets for items.
            let stack = self.stack.last().filter(|o| o.part == Part::Stack);
            match stack.and_then(|o| o.computed.align_items) {
                Some(VAlign::Middle) => block.frame.align = Align::Center,
                Some(VAlign::Bottom) => block.frame.align = Align::End,
                _ => {}
            }
            self.emit(block);
        } else if let Some(alt) = alt {
            let italic = Emphasis {
                italic: true,
                ..unseen
            };
            if let Some(t) = self.target() {
                t.push(alt, italic);
            }
            self.finish();
        }
    }

    fn inline_picture(
        &mut self,
        picture: &Arc<Picture>,
        width: f32,
        height: f32,
        computed: Option<&Computed>,
        invisible: bool,
        inline_link: Option<u32>,
    ) -> bool {
        // A small picture is a glyph of its line.
        let link = inline_link.or(self.top().link).filter(|_| !invisible);
        let space = std::mem::take(&mut self.space);
        if let Some(t) = self.target() {
            let spaced = !t.text.is_empty() && !t.text.ends_with('\n');
            if space && spaced {
                t.push(" ", Emphasis::default());
            }
            let at = t.text.len();
            if t.inlines.len() < MAX_INLINES {
                let picture = Arc::clone(picture);
                let rise = computed.map_or_else(Default::default, |c| c.rise);
                t.inlines.push(Inline {
                    at,
                    picture,
                    width,
                    height,
                    rise,
                    link,
                    hidden: invisible,
                });
            }
            return true;
        }
        false
    }
}
