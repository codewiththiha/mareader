use super::*;

impl State<'_> {
    /// Opens `name`'s space above, as a box if it draws or sits apart.
    pub(super) fn open_block(&mut self, tag: &Tag, (c, d): (&Computed, &Decl), part: Part) -> bool {
        let cell = part == Part::Cell;
        if !cell {
            self.gap.margin(c.margin_px(0), &*tag.name == "p");
        }
        let name: &str = &tag.name;
        // A table of data draws its own grid, not a box.
        let layout = name != "table" || self.lays_out(tag);
        let rule = std::mem::take(&mut self.rule_next);
        // A fixed page's root ground is drawn under the whole page.
        let ground = self.grounded && name == "body";
        let (contrast, backdrop, finish) = match layout && !ground {
            true => self.paint(c),
            false => (None, None, None),
        };
        let spec = layout.then(|| boxed(name, c, d, cell)).flatten();
        let shows = finish.as_ref().is_some_and(Finish::shows);
        let painted = contrast.is_some() || backdrop.is_some() || shows;
        // Kept whole, a plain wrapper's blocks need a box to share.
        let wraps = c.whole && !TEXT_BLOCKS.contains(&name);
        let spec = spec.or_else(|| (painted || wraps).then(|| bare(c)));
        let colours = c
            .border_colour
            .map(|ink| ink.and_then(|ink| self.drawn(ink, MAX_BORDER_FADE)));
        let finish = finish.map(Arc::new);
        let spec = spec.map(|spec| Boxed {
            fill: contrast.map(|c| c.min(MAX_TEXT_FILL)),
            colours,
            backdrop,
            finish: finish.as_deref().map(|f| Arc::new(capped(f))),
            rule,
            whole: c.whole,
            ..spec
        });
        let snapshot = self.gap;
        let inside = c.border[0] + c.padding[0];
        if inside > 0.0 {
            self.gap.fixed(inside);
        }
        let Some(spec) = spec else {
            return false;
        };
        self.boxes.push(BoxOpen {
            spec,
            arc: None,
            gap: snapshot,
            first: self.blocks.len(),
            contrast,
            finish,
        });
        true
    }

    /// A background's ink strength, its contrast with the page; its picture.
    pub(super) fn paint(
        &mut self,
        c: &Computed,
    ) -> (Option<f32>, Option<Arc<Backdrop>>, Option<Finish>) {
        let contrast = c
            .fill
            .map(|f| (css::luminance(f) - css::luminance(self.page)).abs());
        let backdrop = c.picture.as_deref().and_then(|p| self.backdrop(p, c));
        (
            contrast.filter(|c| *c >= MIN_FILL),
            backdrop,
            self.box_finish(c),
        )
    }

    /// A background picture at `url`, sized and placed as `c` says.
    pub(super) fn backdrop(&mut self, url: &str, c: &Computed) -> Option<Arc<Backdrop>> {
        let picture = self.css_picture(url)?;
        let length = |l: Option<Len>| match l? {
            Len::Px(px) => Some(Length::Em(px / ROOT_PX)),
            Len::Em(e) | Len::Rem(e) => Some(Length::Em(e)),
            Len::Percent(p) => Some(Length::Percent(p)),
            Len::Mix { px, em, pct } => Some(Length::Mix {
                em: px / ROOT_PX + em,
                percent: pct,
            }),
            _ => None,
        };
        let size = match c.picture_size {
            PictureSize::Auto => BackdropSize::Auto,
            PictureSize::Cover => BackdropSize::Cover,
            PictureSize::Contain => BackdropSize::Contain,
            PictureSize::Set([w, h]) => BackdropSize::Set([length(w), length(h)]),
        };
        Some(Arc::new(Backdrop {
            picture,
            size,
            at: c.picture_at,
            repeat: c.picture_repeat,
        }))
    }

    /// How strongly `tint` shows on the page, as a share of the ink.
    pub(super) fn strength(&self, (rgb, alpha): Tint, c: &Computed) -> f32 {
        let rgb = rgb.or(c.ink).unwrap_or(self.book_ink);
        let contrast = (css::luminance(rgb) - css::luminance(self.page)).abs();
        (contrast * alpha).clamp(0.0, 1.0)
    }

    /// Shadows, gradients, a border picture, an outline and opacity.
    pub(super) fn box_finish(&mut self, c: &Computed) -> Option<Finish> {
        let em = |l: Len| l.px(c.size, css::BASE_WIDTH).unwrap_or(0.0) / ROOT_PX;
        let shadows = c.shadows.iter().map(|s| mareader_reflow::Shadow {
            at: s.at.map(em),
            blur: em(s.blur).max(0.0),
            spread: em(s.spread),
            strength: self.strength(s.tint, c),
            inset: s.inset,
        });
        let shadows = shadows.filter(|s| s.strength >= 0.01).collect();
        let mut layers = Vec::new();
        for layer in c.layers.iter() {
            let made = match layer {
                decor::Layer::Url(url) => {
                    self.backdrop(url, c).map(mareader_reflow::Layer::Picture)
                }
                decor::Layer::Gradient(g) => {
                    Some(mareader_reflow::Layer::Gradient(self.gradient(g, c)))
                }
            };
            layers.extend(made);
        }
        let frame = c.frame.as_deref().and_then(|url| self.frame9(url, c));
        let outline = (c.outline > 0.0 && c.outline_style != BorderStyle::None).then(|| Outline {
            width: c.outline / ROOT_PX,
            offset: c.outline_offset / ROOT_PX,
            edge: edge(c.outline_style),
            colour: c
                .outline_colour
                .and_then(|ink| self.drawn(ink, MAX_BORDER_FADE)),
        });
        let finish = Finish {
            shadows,
            layers,
            frame,
            outline,
            opacity: if c.invisible {
                0.0
            } else {
                c.opacity.clamp(0.0, 1.0)
            },
        };
        (finish.shows() || finish.opacity < 0.99).then_some(finish)
    }

    /// A gradient's stops as strengths, its places as lengths.
    pub(super) fn gradient(&self, g: &decor::Gradient, c: &Computed) -> mareader_reflow::Gradient {
        let place = |l: Len| match l {
            Len::Percent(p) => Length::Percent(p),
            Len::Em(e) => Length::Em(e * c.size / ROOT_PX),
            Len::Mix { px, em, pct } => Length::Mix {
                em: (px + em * c.size) / ROOT_PX,
                percent: pct,
            },
            other => Length::Em(other.px(c.size, css::BASE_WIDTH).unwrap_or(0.0) / ROOT_PX),
        };
        let shape = match g.radial {
            true => GradientShape::Radial {
                at: g.at,
                circle: g.circle,
                near: g.near,
            },
            false => GradientShape::Linear(g.angle),
        };
        mareader_reflow::Gradient {
            shape,
            stops: g
                .stops
                .iter()
                .map(|(tint, at)| (at.map(place), self.strength(*tint, c)))
                .collect(),
            repeating: g.repeating,
        }
    }

    /// A border picture at `url`, cut and sized as `c` says.
    pub(super) fn frame9(&mut self, url: &str, c: &Computed) -> Option<Frame9> {
        let picture = self.css_picture(url)?;
        let (pw, ph) = (picture.width as f32, picture.height as f32);
        if pw < 1.0 || ph < 1.0 {
            return None;
        }
        let s = c.frame_slice;
        let dims = [ph, pw, ph, pw];
        let at = |a: &[f32; 4], i: usize| a.get(i).copied().unwrap_or_default();
        let slice: [f32; 4] = std::array::from_fn(|i| {
            let cut = at(&s.cut, i);
            let share = match s.percent.get(i).copied().unwrap_or(true) {
                true => cut / 100.0,
                false => cut / at(&dims, i),
            };
            share.clamp(0.0, 1.0)
        });
        let span = |spans: &[Span; 4], i: usize| {
            let px = match spans.get(i).copied().unwrap_or_default() {
                Span::Auto => at(&slice, i) * at(&dims, i),
                Span::Times(n) => n * at(&c.border, i),
                Span::Len(l) => l.px(c.size, css::BASE_WIDTH).unwrap_or(0.0),
            };
            px.max(0.0) / ROOT_PX
        };
        let width = std::array::from_fn(|i| span(&c.frame_width, i));
        let outset = std::array::from_fn(|i| span(&c.frame_outset, i));
        Some(Frame9 {
            picture,
            slice,
            fill: s.fill,
            width,
            outset,
            tile: c.frame_tile,
        })
    }

    /// Closes the innermost box, giving its blocks its bottom inside.
    pub(super) fn close_box(&mut self, c: &Computed, own: bool) {
        let Some(mut open) = self.boxes.pop() else {
            return self.space_below(c, own);
        };
        let inside = c.border[2] + c.padding[2];
        let shows = open.spec.finish.as_ref().is_some_and(|f| f.shows());
        let painted = open.spec.fill.is_some() || open.spec.backdrop.is_some() || shows;
        let edged = painted || open.spec.border.iter().any(|w| *w > 0.0);
        let held = self.blocks.get(open.first..).unwrap_or_default();
        if edged && open.arc.is_some() && held.iter().all(Block::is_blank) {
            // Only blank lines inside, which aren't laid: an empty box.
            self.blocks.truncate(open.first);
            open.arc = None;
        }
        match &open.arc {
            Some(arc) if inside > 0.0 => {
                self.gap.fixed(inside);
                let mut spec = (**arc).clone();
                spec.bottom = self.gap.total() / ROOT_PX;
                let spec = Arc::new(spec);
                for block in self.blocks.iter_mut().skip(open.first) {
                    for b in &mut block.frame.boxes {
                        if Arc::ptr_eq(b, arc) {
                            *b = Arc::clone(&spec);
                        }
                    }
                }
            }
            Some(_) => {}
            // An empty bordered box, a book's rule: a blank block of it.
            None if edged => {
                let tall = open.spec.height.unwrap_or(0.0) * ROOT_PX;
                let gaps = (self.gap.total() - open.gap.total() + inside).max(0.0);
                let height = (gaps + tall) / ROOT_PX;
                self.gap = open.gap;
                // No text to hide: a line or block shows at full strength.
                open.spec.fill = open.contrast;
                open.spec.finish = open.finish.clone();
                self.boxes.push(open);
                let mut block = Block::new(Content::Space(height.max(0.0)));
                block.frame = self.frame();
                self.boxes.pop();
                self.emit(block);
            }
            None if inside > 0.0 => self.gap.fixed(inside),
            None => {}
        }
        self.gap.margin(c.margin_px(2), own);
    }
}
