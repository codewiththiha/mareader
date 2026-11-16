use super::*;

mod row;
mod text;

pub(super) fn layout_text(
    fonts: &mut Fonts,
    text: &Text,
    style: Style<'_>,
    (width, beside): (Option<f32>, Option<Beside>),
) -> Para {
    let hyphenated = style.hyphens.map(|lang| hyphen::hyphenate(text, lang));
    let text = hyphenated.as_ref().map_or(text, |h| &h.text);
    let Fonts { fonts, layout } = fonts;
    let ink = Ink {
        tone: style.tone,
        code: false,
        colour: None,
        shadow: None,
        hidden: false,
    };
    let (shown, small) = cased(text);
    let mut b = layout.ranged_builder(fonts, &shown, 1.0, false);
    b.set_base_direction(match style.dir {
        Dir::Auto => BaseDirection::Auto,
        Dir::Ltr => BaseDirection::Ltr,
        Dir::Rtl => BaseDirection::Rtl,
    });
    b.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Borrowed(
        style.faces,
    ))));
    b.push_default(StyleProperty::FontSize(style.size));
    b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
        style.line_height,
    )));
    b.push_default(StyleProperty::FontWeight(FontWeight::new(style.weight)));
    b.push_default(StyleProperty::Brush(ink));
    if style.letter != 0.0 {
        b.push_default(StyleProperty::LetterSpacing(style.letter));
    }
    if style.word != 0.0 {
        b.push_default(StyleProperty::WordSpacing(style.word));
    }
    if style.pre {
        b.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
    }
    for note in &text.ruby {
        // Room above the base for its note.
        let tall = LineHeight::FontSizeRelative(style.line_height + RUBY_ROOM);
        b.push(StyleProperty::LineHeight(tall), note.range.clone());
    }
    text::spans(&mut b, text, style, ink);
    text::small_caps(&mut b, small, style, text);
    let (upright, combine) = match style.turned {
        true => (
            upright_runs(&shown, style.orient),
            (text.combine.iter())
                .map(|r| clip(&shown, r.clone()))
                .filter(|r| !r.is_empty())
                .collect(),
        ),
        false => (Vec::new(), Vec::new()),
    };
    if style.turned {
        turned_features(&mut b, text, (&upright, &combine));
    }
    text::inlines(&mut b, text, style, &shown);
    text::finish(
        b,
        &shown,
        text,
        style,
        hyphenated.as_ref(),
        (width, beside),
        (upright, combine),
    )
}
impl Cursor {
    pub(super) fn break_page(&mut self) {
        self.settle();
        if !self.page.is_empty() {
            // A box running on is cut here and goes on overleaf.
            for open in &mut self.open {
                self.deco.extend(open.deco(self.y, false));
                (open.top, open.continued, open.centre) = (0.0, true, None);
            }
            self.pages.push(std::mem::take(&mut self.page));
            self.decos.push(std::mem::take(&mut self.deco));
        }
        (self.y, self.float_bottom) = (0.0, 0.0);
    }

    /// Ends cells spanning down early: what follows starts below them.
    pub(super) fn settle(&mut self) {
        for h in std::mem::take(&mut self.hangs) {
            self.y = self.y.max(h.bottom);
        }
    }

    /// A row of `grid` ended: the cells it ends sit across their rows.
    pub(super) fn row_done(&mut self, grid: usize) {
        let mut y = self.y;
        for mut h in std::mem::take(&mut self.hangs) {
            h.rows = h.rows.saturating_sub(1);
            if h.grid == grid && h.rows > 0 {
                self.hangs.push(h);
                continue;
            }
            // Taller than its rows: the last of them grows.
            let room = self.y - h.bottom;
            y = y.max(h.bottom);
            let shift = match h.valign {
                _ if room <= 0.0 => continue,
                VAlign::Top => continue,
                VAlign::Middle => room / 2.0,
                VAlign::Bottom => room,
            };
            for i in &h.pieces {
                if let Some(p) = self.page.get_mut(*i) {
                    p.dy += shift;
                }
            }
            for d in self.deco.get_mut(h.decos).unwrap_or_default() {
                d.rect[1] += shift;
            }
        }
        self.y = y;
    }

    /// Places `units` of block `block` at the cursor.
    pub(super) fn place(&mut self, block: &Set, index: usize, units: Range<usize>) {
        let (Some(&(top, _)), Some(&(_, bottom))) = (
            block.units.get(units.start),
            units.end.checked_sub(1).and_then(|u| block.units.get(u)),
        ) else {
            return;
        };
        let off = block.rel_offset();
        self.page.push(Piece {
            block: index,
            units,
            dx: off[0],
            dy: self.y - top + off[1],
        });
        self.y += bottom - top;
    }

    /// How many of the open boxes `boxes` is still in.
    pub(super) fn common(&self, boxes: &[Placed]) -> usize {
        let same = |(o, b): &(&OpenBox, &Placed)| o.placed.key == b.key;
        self.open.iter().zip(boxes).take_while(same).count()
    }

    /// Closes the open boxes after the first `keep`, below what is placed.
    pub(super) fn close(&mut self, keep: usize) {
        while self.open.len() > keep {
            let Some(open) = self.open.pop() else {
                break;
            };
            let mut bottom = self.y + open.placed.bottom;
            if let (Some((pieces, decos)), Some(share)) = (open.centre, open.placed.centre) {
                // Its content sits mid-way down the height it fills.
                let fill = (share * self.height).min(self.height - open.top);
                let shift = ((fill - (bottom - open.top)) / 2.0).max(0.0);
                for piece in self.page.iter_mut().skip(pieces) {
                    piece.dy += shift;
                }
                for deco in self.deco.iter_mut().skip(decos) {
                    deco.rect[1] += shift;
                }
                self.y += shift;
                bottom += 2.0 * shift;
            }
            if let Some(least) = open.placed.height.filter(|_| !open.continued) {
                // A set height holds the box open below short content.
                let floor = open.top + open.placed.inside_top + least;
                self.y = self.y.max(floor.min(self.height));
                bottom = bottom.max(self.y + open.placed.bottom);
            }
            self.deco.extend(open.deco(bottom, true));
        }
    }

    /// Opens what `boxes` holds past the open ones; a block goes in next.
    pub(super) fn open(&mut self, boxes: &[Placed]) {
        let fresh = boxes.get(self.open.len()..).unwrap_or_default();
        // A box opening atop a page keeps its border on the page.
        let need = fresh.iter().map(|b| b.top - self.y).fold(0.0, f32::max);
        self.y += need;
        let empty = self.page.is_empty() && self.height.is_finite();
        for b in fresh {
            let centre =
                (empty && b.centre.is_some()).then_some((self.page.len(), self.deco.len()));
            self.open.push(OpenBox {
                placed: b.clone(),
                top: self.y - b.top,
                continued: false,
                centre,
            });
        }
    }
}
/// Places grid row `row` whole, cells side by side; false if too tall.
pub(super) fn place_row(
    c: &mut Cursor,
    blocks: &[Set],
    row: Range<usize>,
    (height, typo): (f32, &Typography),
) -> bool {
    let Some((first, cell)) = blocks.get(row.start).and_then(|b| Some((b, b.cell()?))) else {
        return false;
    };
    let members = blocks.get(row.clone()).unwrap_or_default();
    let Some(measure) = row::measure(members, typo, height, cell) else {
        return false;
    };
    let need = measure.need;
    if c.hangs.iter().any(|h| h.grid != cell.grid) {
        c.settle();
    }
    let outer = first.boxes.get(..cell.depth).unwrap_or(&first.boxes);
    c.close(c.common(outer));
    if first.break_before {
        c.break_page();
    }
    if first.clear {
        c.y = c.y.max(c.float_bottom);
    }
    if !c.page.is_empty() {
        c.y += cell.before.max(0.0);
        if c.y + need > height {
            c.break_page();
        }
    }
    c.open(outer);
    row::place(c, row, members, cell, measure);
    true
}

/// A book's box open on the page being filled.
pub(super) struct OpenBox {
    pub(super) placed: Placed,
    pub(super) top: f32,
    /// Begun on an earlier page: no top border here.
    pub(super) continued: bool,
    /// Centring, begun on a fresh page: the pieces and borders before it.
    pub(super) centre: Option<(usize, usize)>,
}

impl OpenBox {
    /// Its borders down to `bottom`; open there unless `closed`.
    pub(super) fn deco(&self, bottom: f32, closed: bool) -> Option<Deco> {
        let (mut border, mut edges) = (self.placed.border, self.placed.edges);
        if self.continued {
            (border[0], edges[0]) = (0.0, Edge::None);
        }
        if !closed {
            (border[2], edges[2]) = (0.0, Edge::None);
        }
        let edged = border
            .iter()
            .zip(&edges)
            .any(|(w, e)| *w > 0.0 && e.draws());
        let p = &self.placed;
        let finished = p.finish.as_ref().is_some_and(|f| f.shows());
        let drawn = edged
            || p.fill.is_some()
            || p.fill_rgba.is_some()
            || p.backdrop.is_some()
            || finished
            || p.interaction.is_some();
        let rect = [p.x, self.top, p.width, bottom - self.top];
        drawn.then(|| Deco {
            rect: rect.map(|v| if v.is_finite() { v } else { 0.0 }),
            border,
            edges,
            colours: p.colours,
            fill: p.fill,
            fill_rgba: p.fill_rgba,
            backdrop: p.backdrop.clone(),
            em: p.em,
            radius: p.radius,
            finish: p.finish.clone(),
            continued: [self.continued, !closed],
            box_key: p.key,
            clip: p.clip,
            transform: p.transform,
            interaction: p.interaction.clone(),
            z: 0,
        })
    }
}

/// Box `b` in the room at `x`, `width` across: margins, width, centring.
pub(super) fn place(b: &Arc<Boxed>, (x, width): (f32, f32), em: f32) -> Placed {
    let inside = (b.border[1] + b.border[3] + b.padding[1] + b.padding[3]) * em;
    let v_inside = (b.border[0] + b.border[2] + b.padding[0] + b.padding[2]) * em;
    let side = |m: Option<Length>| m.map(|l| resolve(l, width, em));
    let (left, right) = (side(b.margin[0]), side(b.margin[1]));
    let allow_overflow = left.is_some_and(|l| l < 0.0) || right.is_some_and(|r| r < 0.0);
    let content = match b.width {
        Some(l) => {
            let raw = resolve(l, width, em);
            let inner = if b.border_box {
                (raw - inside).max(0.0)
            } else {
                raw
            };
            if allow_overflow {
                inner
            } else {
                inner.min((width - inside).max(0.0))
            }
        }
        None => width - left.unwrap_or(0.0) - right.unwrap_or(0.0) - inside,
    };
    let outer = content.max(0.0) + inside;
    let left = match (left, right, b.width) {
        (l, _, None) => l.unwrap_or(0.0),
        (None, None, Some(_)) => ((width - outer) / 2.0).max(0.0),
        (None, Some(r), Some(_)) => (width - outer - r).max(0.0),
        (Some(l), _, Some(_)) => l,
    };
    let height = b.height.map(|h| {
        let px = h * em;
        if b.border_box {
            (px - v_inside).max(0.0)
        } else {
            px
        }
    });
    Placed {
        key: Arc::as_ptr(b) as usize,
        x: x + left,
        width: outer,
        border: b.border.map(|w| w * em),
        padding: b.padding.map(|w| w * em),
        edges: b.edges,
        colours: b.colours,
        top: b.top * em,
        bottom: b.bottom * em,
        centre: b.centre,
        inside_top: (b.border[0] + b.padding[0]) * em,
        height,
        fill: b.fill,
        fill_rgba: b.fill_rgba,
        backdrop: b.backdrop.clone(),
        em,
        radius: b.radius.map_or(0.0, |r| resolve(r, outer, em).max(0.0)),
        finish: b.finish.clone(),
        whole: b.whole,
        clip: b.clip,
        multicol: b.multicol.clone(),
        transform: b.transform,
        interaction: b.interaction.clone(),
    }
}
