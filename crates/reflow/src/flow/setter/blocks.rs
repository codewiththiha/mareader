use super::*;

impl<'a> Setter<'a> {
    pub(in crate::flow) fn block(&mut self, source: usize, block: &Block) -> (Set, Laid) {
        let frame = &block.frame;
        let spot = frame.spot.as_deref().copied().filter(|_| self.fixed);
        // A positioned box with a set height holds its pictures to it.
        let height = spot.and_then(|s| s.height).unwrap_or(self.height);
        let column = self.column;
        // Every chapter the book styles is laid out by its CSS.
        let designed = self.look(frame).is_some();
        let em = self.em_of(frame);
        let (left, room, mut boxes, cell) = match designed {
            true => self.boxes(frame, column),
            false => (0.0, column, ThinVec::new(), None),
        };
        // Deep nesting stops indenting at half the column.
        let indent = (frame.inset * em).clamp(0.0, room / 2.0);
        // Right to left, lists and quotes indent from the right.
        let rtl = right_to_left(block);
        let x = if rtl { left } else { left + indent };
        let width = room - indent;
        let (x, width, float, beside) =
            self.float_place(frame, designed, em, (x, width), &mut boxes);
        let (draw, laid, units, starts) = self.block_content(
            block,
            frame,
            (width, beside),
            (designed, em),
            (column, height),
        );
        let cover = matches!(block.content, Content::Image { cover: true, .. });
        let heads = match &block.content {
            Content::Table(t) => t.rows.iter().take_while(|r| r.head).count(),
            _ => 0,
        };
        let heads = u16::try_from(heads).unwrap_or(u16::MAX);
        let set = Set {
            source,
            x: if cover { 0.0 } else { x },
            width: if cover { column } else { width },
            bars: frame
                .bars
                .iter()
                .map(|b| (b * em).min(column / 2.0))
                .map(|b| if rtl { column - b - BAR_ROOM } else { b })
                .collect(),
            rtl,
            draw,
            units,
            starts: starts.into_iter().collect(),
            keep_with_next: block.is_heading() || (designed && frame.keep.with_next),
            // A fixed page is one page whatever its blocks ask.
            break_before: block.break_before && !self.fixed,
            tight: frame.tight,
            before: (self.look(frame)).map(|_| self.before(block, em)),
            lines: matches!(block.content, Content::Text(_)),
            orphans: lines_kept(frame.keep.orphans, designed),
            widows: lines_kept(frame.keep.widows, designed),
            whole: designed && frame.keep.whole,
            heads,
            boxes,
            cell: cell.map(Box::new),
            float: float.map(Box::new),
            beside: self.exclusion.filter(|_| designed),
            clear: false,
            place: self.place_of(frame).filter(|_| designed).map(Box::new),
            rotate: if designed { frame.rotate } else { 0.0 },
            hidden: frame.hidden,
            turned: self.turned,
            sheet: frame.sheet.clone().filter(|_| self.fixed),
            spot: spot
                .zip(frame.spot.as_ref())
                .map(|(s, a)| Box::new((Arc::as_ptr(a) as usize, s))),
            em,
            extra: set_extra(frame, designed, em),
        };
        (set, laid)
    }

    /// Containing room left by the first `depth` ancestor boxes.
    pub(super) fn box_room(&self, frame: &Frame, depth: usize) -> (f32, f32) {
        let em = self.em_of(frame);
        let (mut x, mut width) = (0.0, self.column);
        for b in frame.boxes.iter().take(depth) {
            let p = place(b, (x, width), em);
            let inside = (b.border[1] + b.border[3] + b.padding[1] + b.padding[3]) * em;
            x = p.x + (b.border[3] + b.padding[3]) * em;
            width = (p.width - inside).max(1.0);
        }
        (x, width)
    }

    /// Sets a float's blocks at its outer box width; gives its wrap.
    pub(super) fn float_group(
        &mut self,
        group: &[(usize, &Block)],
        f: &Floated,
    ) -> (Vec<Set>, Option<Wrap>) {
        (self.exclusion, self.float_box) = (None, None);
        let Some((_, first)) = group.first() else {
            return (Vec::new(), None);
        };
        let depth = usize::from(f.depth);
        let em = self.em_of(&first.frame);
        let (avail_x, avail_w) = self.box_room(&first.frame, depth);
        let own_spec = first.frame.boxes.get(depth);
        let side = |m: Option<Length>| m.map_or(0.0, |l| resolve(l, avail_w, em)).max(0.0);
        let (ml, mr) = own_spec.map_or((0.0, 0.0), |s| (side(s.margin[0]), side(s.margin[1])));
        let max_w = (avail_w - ml - mr).max(1.0);
        let mut want: f32 = 0.0;
        let mut explicit: Option<f32> = None;
        for &(i, b) in group {
            let (set, laid) = self.block(i, b);
            if let Some(own) = set.boxes.get(depth) {
                if own_spec.is_some_and(|s| s.width.is_some()) {
                    explicit.get_or_insert(own.width);
                }
                let overhead = (set.x - own.x) + ((own.x + own.width) - (set.x + set.width));
                want = want.max(natural(&set, &laid) + overhead.max(0.0));
            } else {
                want = want.max(natural(&set, &laid));
            }
        }
        let float_w = explicit.unwrap_or(want).clamp(1.0, max_w);
        let left = if f.right {
            avail_x + avail_w - mr - float_w
        } else {
            avail_x + ml
        };
        self.float_box = Some((left, float_w));
        let sets: Vec<Set> = group.iter().map(|&(i, b)| self.block(i, b).0).collect();
        self.float_box = None;
        let own = sets.first().and_then(|s| s.boxes.get(depth));
        let (near, far) = own.map_or((left, left + float_w), |p| (p.x, p.x + p.width));
        let edge = if f.right {
            near - ml.max(FLOAT_GAP * em)
        } else {
            far + mr.max(FLOAT_GAP * em)
        };
        let mut height = 0.0;
        for (n, set) in sets.iter().enumerate() {
            let gap = gap(set, self.typo);
            height += span_height(set, 0..set.units.len()) + gap;
            if n == 0 {
                height += own.map_or(0.0, |p| p.top);
            }
        }
        height += own.map_or(0.0, |p| p.bottom);
        let within = Holders::of(&first.frame, depth);
        let exclusion = Exclusion {
            right: f.right,
            edge,
            height: 0.0,
        };
        let wrap = Wrap {
            exclusion,
            height,
            within,
        };
        (sets, Some(wrap))
    }

    fn float_place(
        &mut self,
        frame: &Frame,
        designed: bool,
        em: f32,
        (mut x, mut width): (f32, f32),
        boxes: &mut [Placed],
    ) -> (f32, f32, Option<FloatAt>, Option<Beside>) {
        let float = self.float_box.filter(|_| designed).map(|(fx, fw)| {
            let depth = frame.float.as_ref().map_or(0, |f| usize::from(f.depth));
            if let Some((ox, ow)) = boxes.get(depth).map(|p| (p.x, p.width)) {
                // Shift the float's outer box while keeping inner offsets.
                let (dx, dw) = (fx - ox, fw - ow);
                if let Some(p) = boxes.get_mut(depth) {
                    (p.x, p.width) = (fx, fw);
                }
                for p in boxes.iter_mut().skip(depth + 1) {
                    p.x += dx;
                    p.width = (p.width + dw).max(1.0);
                }
                x += dx;
                width = (width + dw).max(1.0);
            } else {
                (x, width) = (fx, fw);
            }
            FloatAt {
                key: float_key(frame).unwrap_or(0),
                depth,
                x: fx,
                width: fw,
            }
        });
        let beside = self.exclusion.filter(|_| designed).map(|e| {
            let (l, r) = match e.right {
                true => (0.0, x + width - e.edge),
                false => (e.edge - x, 0.0),
            };
            let most = (width - 2.0 * em).max(0.0);
            Beside {
                left: l.clamp(0.0, most),
                right: r.clamp(0.0, most),
                height: e.height,
            }
        });
        (x, width, float, beside)
    }

    fn block_content(
        &mut self,
        block: &Block,
        frame: &Frame,
        (width, beside): (f32, Option<Beside>),
        (designed, em): (bool, f32),
        (column, height): (f32, f32),
    ) -> (Draw, Laid, Vec<(f32, f32)>, Vec<usize>) {
        let edged = || {
            frame
                .boxes
                .iter()
                .any(|b| b.rule || b.edges.iter().any(|e| e.draws()))
        };
        match &block.content {
            Content::Text(text) => self.text(text, frame, width, beside),
            Content::Rule => {
                let unit = (0.0, RULE_HEIGHT * em);
                (Draw::Rule, Laid::None, vec![unit], vec![0])
            }
            Content::Space(h) if designed => {
                let unit = (0.0, (h * em).max(0.0));
                (Draw::Space, Laid::None, vec![unit], vec![0])
            }
            // Read as prose, a bordered blank is a plain rule.
            Content::Space(_) if edged() => {
                let unit = (0.0, RULE_HEIGHT * em);
                (Draw::Rule, Laid::None, vec![unit], vec![0])
            }
            Content::Space(_) => (Draw::Space, Laid::None, vec![(0.0, 0.0)], vec![0]),
            Content::Image {
                picture,
                cover,
                width: asked,
                links,
            } => {
                // Vertical: laid on its side, then drawn standing.
                let turned = self.turned;
                let (aspect, natural) = match turned {
                    true => (sideways(picture.aspect()), picture.height),
                    false => (picture.aspect(), picture.width),
                };
                let (rect, unit) = if *cover {
                    cover_rect(aspect, column, height)
                } else {
                    fit_image_rect(
                        (aspect, natural as f32),
                        (asked, frame.picture_fit(), frame.align),
                        (width, height, em),
                    )
                };
                let picture = picture.clone();
                let draw = Draw::Image {
                    picture,
                    rect,
                    turned,
                    links: links.iter().cloned().collect(),
                };
                (draw, Laid::None, vec![unit], vec![0])
            }
            Content::Table(table) => {
                let (grid, cells, units) = self.table(table, frame, width);
                let rows = (0..units.len()).collect();
                (Draw::Table(grid), Laid::Table(cells), units, rows)
            }
        }
    }

    /// Places the boxes and cell in `column`: content left, width, both.
    pub(super) fn boxes(
        &self,
        frame: &Frame,
        column: f32,
    ) -> (f32, f32, ThinVec<Placed>, Option<CellAt>) {
        let em = self.em_of(frame);
        let (mut x, mut width) = (0.0, column);
        let mut placed = ThinVec::with_capacity(frame.boxes.len());
        let mut at = None;
        let depth = frame.cell.as_ref().map(|c| usize::from(c.depth));
        let mut bounds = None;
        // A positioned box leaves the flow: the boxes outside it stay there.
        let spot = frame.spot.as_deref().filter(|_| self.fixed);
        let spot_depth = spot.map(|s| usize::from(s.depth));
        for (i, b) in frame.boxes.iter().enumerate() {
            if let Some(s) = spot.filter(|_| spot_depth == Some(i)) {
                (x, width) = (s.x, s.width.max(1.0));
                placed.clear();
            }
            if depth == Some(i) {
                at = frame
                    .cell
                    .as_ref()
                    .map(|c| enter(c, (&mut x, &mut width), em, self.place_of(frame)));
                bounds = Some((x, x + width));
            }
            let mut p = place(b, (x, width), em);
            if let Some((left, right)) = bounds {
                // Inside a cell, negative margins stop at its edges.
                let end = (p.x + p.width).min(right);
                p.x = p.x.max(left);
                p.width = (end - p.x).max(1.0);
            }
            let inside = (b.border[1] + b.border[3] + b.padding[1] + b.padding[3]) * em;
            x = p.x + (b.border[3] + b.padding[3]) * em;
            width = (p.width - inside).max(1.0);
            if let Some(mc) = b.multicol.as_deref()
                && !frame.column_span()
            {
                width = multicol::geometry(mc, width, em).width;
            }
            placed.push(p);
        }
        if let Some(s) = spot.filter(|_| spot_depth.is_some_and(|d| d >= frame.boxes.len())) {
            (x, width) = (s.x, s.width.max(1.0));
            placed.clear();
        }
        if at.is_none() && depth.is_some_and(|d| d >= frame.boxes.len()) {
            at = frame
                .cell
                .as_ref()
                .map(|c| enter(c, (&mut x, &mut width), em, self.place_of(frame)));
        }
        (x, width, placed, at)
    }
}

fn fit_image_rect(
    (aspect, natural): (f32, f32),
    (asked, picture_fit, align): (&Option<Length>, Option<[Option<Length>; 2]>, Align),
    (width, height, em): (f32, f32, f32),
) -> ([f32; 4], (f32, f32)) {
    let [fit_h, max_h] = picture_fit.unwrap_or([None, None]);
    let limit_h = max_h.map_or(height, |l| resolve(l, height, em).min(height));
    let explicit_h = fit_h.map(|l| resolve(l, height, em).min(limit_h));
    match (asked, explicit_h) {
        (Some(w_len), Some(h_px)) => {
            let w = resolve(*w_len, width, em).max(1.0).min(width);
            let h = h_px.max(1.0);
            let x = match align {
                Align::Start => 0.0,
                Align::Center => (width - w) / 2.0,
                Align::End => width - w,
            };
            ([x, 0.0, w, h], (0.0, h))
        }
        (None, Some(h_px)) => {
            let wanted = if aspect > 0.0 && aspect.is_finite() {
                h_px / aspect
            } else {
                natural
            };
            image_rect(aspect, align, (wanted, width), limit_h)
        }
        (Some(w_len), None) => {
            image_rect(aspect, align, (resolve(*w_len, width, em), width), limit_h)
        }
        (None, None) => image_rect(aspect, align, (natural, width), limit_h),
    }
}

fn set_extra(frame: &Frame, designed: bool, em: f32) -> Option<Box<SetExtra>> {
    if !designed {
        return None;
    }
    let e = frame.extra.as_deref()?;
    Some(Box::new(SetExtra {
        rel_offset: [e.rel_offset[0] * em, e.rel_offset[1] * em],
        column_span: e.column_span,
        transform: e.transform,
        interaction: e.interaction.clone(),
    }))
}
