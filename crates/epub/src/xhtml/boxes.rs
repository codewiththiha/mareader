use super::*;

/// Maximum vertical body margin on reflowable chapters.
const MAX_BODY_V_MARGIN: f32 = 2.0 * ROOT_PX;

struct BoxPaint {
    contrast: Option<f32>,
    fill_rgba: Option<[u8; 4]>,
    backdrop: Option<Arc<Backdrop>>,
    finish: Option<Arc<Finish>>,
}

impl State<'_> {
    /// Opens `name`'s space above, as a box if it draws or sits apart.
    pub(super) fn open_block(
        &mut self,
        tag: &Tag,
        el: &El,
        (c, d): (&Computed, &Decl),
        part: Part,
    ) -> bool {
        let cell = part == Part::Cell;
        let name: &str = &tag.name;
        let reflow_body = name == "body" && !self.fixed;
        if !cell {
            let top = if reflow_body {
                c.margin_px(0).min(MAX_BODY_V_MARGIN)
            } else {
                c.margin_px(0)
            };
            self.gap.margin(top, name == "p");
        }
        // A table of data draws its own grid, not a box.
        let layout = name != "table" || self.lays_out(tag);
        let rule = std::mem::take(&mut self.rule_next);
        // A fixed page's root ground is drawn under the whole page.
        let ground = self.grounded && name == "body";
        let (contrast, backdrop, finish) = match layout && !ground {
            true => self.paint(c),
            false => (None, None, None),
        };
        let fill_rgba = (layout && !ground)
            .then(|| fixed_fill_rgba(c))
            .flatten()
            .filter(|&[r, g, b, a]| {
                self.fixed || (a <= 96 && r.max(g).max(b) - r.min(g).min(b) >= 24)
            });
        let interaction = layout.then(|| self.interaction(el, c, d)).flatten();
        let sheet_h = self.sheet.as_deref().map(|s| s.height);
        let reflow_vw = self
            .viewport
            .map(|(w, _)| w)
            .filter(|&w| !self.fixed && w > css::BASE_WIDTH);
        let spec = layout
            .then(|| boxed_spec(name, c, d, (cell, reflow_body, sheet_h, reflow_vw)))
            .flatten();
        let finish = finish.map(Arc::new);
        let paint = BoxPaint {
            contrast,
            fill_rgba,
            backdrop,
            finish: finish.clone(),
        };
        let spec = self.finish_box_spec(name, c, spec, paint, (rule, interaction, reflow_vw));
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

    fn finish_box_spec(
        &self,
        name: &str,
        c: &Computed,
        spec: Option<Boxed>,
        paint: BoxPaint,
        (rule, interaction, reflow_vw): (bool, Option<Arc<Interaction>>, Option<f32>),
    ) -> Option<Boxed> {
        let BoxPaint {
            contrast,
            fill_rgba,
            backdrop,
            finish,
        } = paint;
        let extra = c.extra.as_deref();
        let shows = finish.as_deref().is_some_and(Finish::shows);
        let painted = contrast.is_some()
            || fill_rgba.is_some()
            || backdrop.is_some()
            || shows
            || interaction.is_some();
        // Kept whole, a plain wrapper's blocks need a box to share.
        let wraps = c.whole && !TEXT_BLOCKS.contains(&name);
        let has_extra = extra.is_some_and(|e| e.multicol.is_some() || e.transform_2d.is_some());
        let spec = spec.or_else(|| (painted || wraps || has_extra).then(|| bare(c)))?;
        let colours = if self.fixed {
            c.border_colour
        } else {
            c.border_colour
                .map(|ink| ink.and_then(|ink| self.drawn(ink, MAX_BORDER_FADE)))
        };
        let multicol = extra.and_then(|e| e.multicol.clone()).map(|mut mc| {
            if let (Some(vw), Length::Em(g)) = (reflow_vw, mc.gap) {
                mc.gap = Length::Em(g * (css::BASE_WIDTH / vw));
            }
            Arc::new(mc)
        });
        Some(Boxed {
            fill: fill_rgba
                .is_none()
                .then(|| contrast.map(|v| v.min(MAX_TEXT_FILL)))
                .flatten(),
            fill_rgba,
            colours,
            backdrop,
            finish: if self.fixed {
                finish
            } else {
                finish.as_deref().map(|f| Arc::new(capped(f)))
            },
            rule,
            whole: c.whole,
            border_box: extra.is_some_and(|e| e.border_box),
            clip: extra.is_some_and(|e| e.clip),
            multicol,
            transform: extra.and_then(|e| e.transform_2d),
            interaction,
            ..spec
        })
    }

    /// Merges inline `:hover` text styles into the enclosing box.
    pub(super) fn merge_inline_hover(&mut self, el: &El, c: &Computed, d: &Decl) {
        if !self.sheets.iter().any(|s| s.has_hover) {
            return;
        }
        let vars = d.vars.clone().unwrap_or_else(|| Arc::from([]));
        let Some(hover_decl) = css::declared_hover(&self.sheets, &self.path(el), &vars) else {
            return;
        };
        let delta = css::anim::build_hover_delta(&hover_decl, c.size);
        if delta.text_colour.is_none() && delta.text_shadow.is_none() {
            return;
        }
        let Some(open) = self.boxes.last_mut() else {
            return;
        };
        let mut inter = open
            .spec
            .interaction
            .as_deref()
            .cloned()
            .unwrap_or_default();
        let h = inter.hover.get_or_insert_with(Default::default);
        if delta.text_colour.is_some() {
            h.text_colour = delta.text_colour;
        }
        if delta.text_shadow.is_some() {
            h.text_shadow = delta.text_shadow;
        }
        open.spec.interaction = Some(Arc::new(inter));
        if let Some(old_arc) = open.arc.as_ref() {
            let new_arc = Arc::new(open.spec.clone());
            for block in self.blocks.iter_mut().skip(open.first) {
                for b in &mut block.frame.boxes {
                    if Arc::ptr_eq(b, old_arc) {
                        *b = Arc::clone(&new_arc);
                    }
                }
            }
            open.arc = Some(new_arc);
        }
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
            repeat_axis: c.extra.as_deref().and_then(|e| e.repeat_axis),
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
        let shadows = c.shadows.iter().map(|s| {
            let colour = self.fixed.then(|| {
                let [r, g, b] = s.tint.0.or(c.ink).unwrap_or(self.book_ink);
                let a = (s.tint.1.clamp(0.0, 1.0) * 255.0).round() as u8;
                [r, g, b, a]
            });
            mareader_reflow::Shadow {
                at: s.at.map(em),
                blur: em(s.blur).max(0.0),
                spread: em(s.spread),
                strength: self.strength(s.tint, c),
                inset: s.inset,
                colour,
            }
        });
        let shadows = shadows
            .filter(|s| s.colour.is_some_and(|c| c[3] > 0) || s.strength >= 0.01)
            .collect();
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
        let painted = open.spec.fill.is_some()
            || open.spec.fill_rgba.is_some()
            || open.spec.backdrop.is_some()
            || shows;
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

/// True RGBA background fill for a fixed-layout element.
pub(super) fn fixed_fill_rgba(c: &Computed) -> Option<[u8; 4]> {
    match c.extra.as_deref().and_then(|e| e.fill_rgba) {
        Some(rgba) => (rgba[3] > 0).then_some(rgba),
        None => c.fill.map(|[r, g, b]| [r, g, b, 255]),
    }
}

/// The box `name` draws or sits in; a cell's column sets its width.
pub(super) fn boxed_spec(
    name: &str,
    c: &Computed,
    d: &Decl,
    (cell, reflow_body, sheet_h, reflow_vw): (bool, bool, Option<f32>, Option<f32>),
) -> Option<Boxed> {
    if matches!(name, "tr" | "thead" | "tbody" | "tfoot" | "caption" | "hr") {
        return None;
    }
    let mut edges = c.border_style.map(edge);
    for (e, clear) in edges.iter_mut().zip(c.border_clear) {
        if clear && *e != Edge::None {
            *e = Edge::Clear;
        }
    }
    let mut border = c.border.map(|w| w.max(0.0) / ROOT_PX);
    for (w, e) in border.iter_mut().zip(&edges) {
        if *e == Edge::None {
            *w = 0.0;
        }
    }
    let side = |i: usize| match (c.margin.get(i).copied().unwrap_or(Len::Auto), reflow_vw) {
        (Len::Auto, _) => None,
        (Len::Percent(p) | Len::Vw(p), _) => Some(Length::Percent(p)),
        (Len::Mix { px, pct, .. }, _) => Some(Length::Mix {
            em: px / ROOT_PX,
            percent: pct,
        }),
        (Len::Px(px), Some(vw)) if px >= 64.0 => Some(Length::Percent((px / vw) * 100.0)),
        _ => Some(Length::Em(c.margin_px(i) / ROOT_PX)),
    };
    let zero = Some(Length::Em(0.0));
    // Reflowable body horizontal margins sit inside mareader's page margins.
    let margin = if cell || reflow_body {
        [zero; 2]
    } else {
        [side(3), side(1)]
    };
    let width = match c.width {
        _ if cell => None,
        Len::Px(px) if px > 0.0 => Some(Length::Em(px / ROOT_PX)),
        Len::Percent(p) | Len::Vw(p) if p > 0.0 => Some(Length::Percent(p)),
        Len::Mix { px, pct, .. } => mixed(px, pct),
        _ => None,
    };
    let flex = matches!(c.display, Display::Flex | Display::Grid);
    let centre = match d.height {
        Some(Len::Vh(v)) if flex && v > 0.0 => Some((v / 100.0).min(1.0)),
        _ => None,
    };
    let s = reflow_vw.map_or(1.0, |vw| (css::BASE_WIDTH / vw).min(1.0));
    let [pt, pr, pb, pl] = c.padding.map(|p| p.max(0.0) / ROOT_PX);
    let padding = [pt, pr * s, pb, pl * s];
    let flat = margin.iter().all(|m| m.is_none() || *m == zero);
    let drawn = border.iter().any(|w| *w > 0.0);
    let height = match c.height {
        _ if matches!(d.height, Some(Len::Vh(_))) => None,
        Len::Px(px) if px > 0.0 && !cell => Some(px / ROOT_PX),
        Len::Percent(p) if p > 0.0 && !cell => sheet_h.map(|h| (h * p / 100.0) / ROOT_PX),
        _ => None,
    };
    let apart = padding[1] > 0.0 || padding[3] > 0.0 || width.is_some() || centre.is_some();
    (drawn || apart || !flat).then_some(Boxed {
        margin,
        border,
        edges,
        padding,
        width,
        centre,
        height,
        radius: radius(c),
        ..Boxed::default()
    })
}
