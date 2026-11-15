use super::*;

mod blocks;
mod text;

impl<'a> Setter<'a> {
    pub(super) fn new(typo: &'a Typography, stacks: &'a Stacks, fonts: &'a mut Fonts) -> Self {
        Self {
            typo,
            stacks,
            lang: hyphen::lang(&typo.language),
            fonts,
            column: column_width(typo),
            height: PAGE_HEIGHT - 2.0 * PAD,
            float_box: None,
            exclusion: None,
            cell_place: None,
            places: HashMap::new(),
            turned: false,
            fixed: false,
            measure: column_width(typo),
        }
    }

    /// Block `frame`'s grid item place, as sized.
    pub(super) fn place_of(&self, frame: &Frame) -> Option<CellPlace> {
        let sized = || {
            item_key(frame)
                .and_then(|key| self.places.get(&key))
                .copied()
        };
        self.cell_place.or_else(sized)
    }

    /// Designed text's em: the A4 measure as a phone page, at reader scale.
    pub(super) fn design_em(&self) -> f32 {
        if self.fixed {
            return FIXED_EM;
        }
        // A pane-sized page is shown as it is: the book's root is the reader's.
        if self.typo.sheet.is_some() {
            return self.typo.font_size;
        }
        let em = READER.0;
        let phone = (self.measure / DESIGN_EMS).clamp(em, em * MAX_DESIGN_SCALE);
        phone * self.typo.font_size / em
    }

    /// The em `frame`'s book type is set in: the reader's in prose.
    pub(super) fn em_of(&self, frame: &Frame) -> f32 {
        match self.scaled(frame) {
            true => self.design_em(),
            false => self.typo.font_size,
        }
    }

    /// Sets `items` in order; floats and the text wrapping past them
    /// need what came before.
    pub(super) fn set_all(&mut self, items: &[(usize, &Block)]) -> Vec<Set> {
        let mut out = Vec::with_capacity(items.len());
        let mut wrap: Option<Wrap> = None;
        let mut sized = None;
        let mut k = 0;
        while let Some(&(i, block)) = items.get(k) {
            let grid = self.look(&block.frame).and(grid_key(&block.frame));
            if let Some(key) = grid.filter(|key| sized != Some(*key)) {
                // A grid's items are sized together, as it starts.
                let rest = items.get(k..).unwrap_or_default();
                let same = |(_, b): &&(usize, &Block)| grid_key(&b.frame) == Some(key);
                let end = k + rest.iter().take_while(same).count().max(1);
                let places = self.size_grid(items.get(k..end).unwrap_or_default());
                self.places = places;
                sized = Some(key);
            }
            let floated = self.look(&block.frame).and(block.frame.float.as_ref());
            if let Some(f) = floated {
                let key = Arc::as_ptr(f) as usize;
                let rest = items.get(k..).unwrap_or_default();
                let same = |(_, b): &(usize, &Block)| float_key(&b.frame) == Some(key);
                let end = k + rest.iter().take_while(|b| same(b)).count().max(1);
                let (sets, next) = self.float_group(items.get(k..end).unwrap_or_default(), f);
                out.extend(sets);
                wrap = next;
                k = end;
                continue;
            }
            self.exclusion = None;
            let mut clear = false;
            if let Some(w) = wrap.take() {
                let height = w.height - block.frame.before * self.em_of(&block.frame);
                let within = w.within.hold(&block.frame) && self.look(&block.frame).is_some();
                if within && matches!(block.content, Content::Text(_)) && height > 0.0 {
                    self.exclusion = Some(Exclusion {
                        height,
                        ..w.exclusion
                    });
                    let (set, _) = self.block(i, block);
                    let left = height - span_height(&set, 0..set.units.len());
                    if left > 0.0 {
                        wrap = Some(Wrap { height: left, ..w });
                    }
                    out.push(set);
                    k += 1;
                    continue;
                }
                clear = height > 0.0;
            }
            let (mut set, _) = self.block(i, block);
            set.clear = clear;
            out.push(set);
            k += 1;
        }
        self.exclusion = None;
        out
    }

    /// Sizes a grid's items to their content: table columns or flex lines.
    pub(super) fn size_grid(
        &mut self,
        group: &[(usize, &Block)],
    ) -> HashMap<(usize, u16), CellPlace> {
        let mut places = HashMap::new();
        let Some(&(_, first)) = group.first() else {
            return places;
        };
        let Some(cell) = first.frame.cell.as_ref() else {
            return places;
        };
        let em = self.em_of(&first.frame);
        let (gx, room) = self.grid_room(&first.frame);
        let mut extents: Vec<Extent> = Vec::new();
        for &(i, b) in group.iter().take(MAX_MEASURED) {
            let Some((row, item)) = item_key(&b.frame) else {
                continue;
            };
            let set = b.frame.cell.as_ref().and_then(|c| {
                let w = c.row.widths.get(usize::from(item)).copied().flatten()?;
                Some(resolve(w, room, em).max(0.0))
            });
            let at = extents.iter().position(|e| e.row == row && e.item == item);
            if at.is_some_and(|n| extents.get(n).is_some_and(|e| e.set)) {
                continue;
            }
            let (least, most) = match set {
                Some(w) => (w, w),
                None => self.measure(i, b, (gx, room)),
            };
            match at.and_then(|n| extents.get_mut(n)) {
                Some(e) => (e.least, e.most) = (e.least.max(least), e.most.max(most)),
                None => extents.push(Extent {
                    row,
                    item,
                    least,
                    most,
                    set: set.is_some(),
                }),
            }
        }
        let shared = &cell.row.columns;
        match shared.flex {
            Some(flex) => {
                let gap = flex.gap.max(0.0) * em;
                let mut rows: Vec<usize> = extents.iter().map(|e| e.row).collect();
                rows.dedup();
                for row in rows {
                    let grow = group.iter().find_map(|(_, b)| {
                        let c = b.frame.cell.as_ref()?;
                        (Arc::as_ptr(&c.row) as usize == row).then(|| c.row.grow.clone())
                    });
                    let items: Vec<&Extent> = extents.iter().filter(|e| e.row == row).collect();
                    flex_places(
                        &items,
                        (room, gap),
                        (flex, &grow.unwrap_or_default()),
                        &mut places,
                    );
                }
            }
            None => {
                let rows = group.iter().filter_map(|(_, b)| b.frame.cell.as_ref());
                table_places(
                    &extents,
                    rows,
                    (room, shared.fill, shared.centre),
                    &mut places,
                );
            }
        }
        places
    }

    /// A grid item's least and most content widths, from its block `b`.
    pub(super) fn measure(&mut self, i: usize, b: &Block, (gx, room): (f32, f32)) -> (f32, f32) {
        let depth = b.frame.cell.as_ref().map_or(0, |c| usize::from(c.depth));
        let mut extent = |width: f32| {
            self.cell_place = Some(CellPlace {
                left: 0.0,
                width,
                line: 0,
            });
            let (set, laid) = self.block(i, b);
            // A box of set width inside the item holds its content.
            let held = b.frame.boxes.iter().zip(&set.boxes).skip(depth);
            let fixed = held.filter(|(b, _)| matches!(b.width, Some(Length::Em(_))));
            match fixed.map(|(_, p)| p.width).next() {
                Some(w) => w,
                None => natural(&set, &laid) + 2.0 * (set.x - gx).max(0.0),
            }
        };
        let least = extent(1.0);
        let most = extent(room * MEASURE_ROOM);
        self.cell_place = None;
        (least.min(most), most)
    }

    /// Where a block's grid sits: the left and width its boxes leave.
    pub(super) fn grid_room(&self, frame: &Frame) -> (f32, f32) {
        let em = self.em_of(frame);
        let (mut x, mut width) = (0.0, self.column);
        let depth = frame.cell.as_ref().map_or(0, |c| usize::from(c.depth));
        for b in frame.boxes.iter().take(depth) {
            let p = place(b, (x, width), em);
            let inside = (b.border[1] + b.border[3] + b.padding[1] + b.padding[3]) * em;
            x = p.x + (b.border[3] + b.padding[3]) * em;
            width = (p.width - inside).max(1.0);
        }
        (x, width)
    }

    /// Sets a float's blocks as wide as they need; gives its wrap.
    pub(super) fn float_group(
        &mut self,
        group: &[(usize, &Block)],
        f: &Floated,
    ) -> (Vec<Set>, Option<Wrap>) {
        (self.exclusion, self.float_box) = (None, None);
        let mut room = None;
        let mut want: f32 = 0.0;
        for &(i, b) in group {
            let (set, laid) = self.block(i, b);
            room.get_or_insert((set.x, set.width));
            want = want.max(natural(&set, &laid));
        }
        let Some((x, width)) = room else {
            return (Vec::new(), None);
        };
        let want = want.clamp(1.0, width);
        let left = if f.right { x + width - want } else { x };
        self.float_box = Some((left, want));
        let sets: Vec<Set> = group.iter().map(|&(i, b)| self.block(i, b).0).collect();
        self.float_box = None;
        let em = group
            .first()
            .map_or(self.typo.font_size, |(_, b)| self.em_of(&b.frame));
        let depth = usize::from(f.depth);
        let own = sets.first().and_then(|s| s.boxes.get(depth));
        let (near, far) = own.map_or((left, left + want), |p| (p.x, p.x + p.width));
        let edge = if f.right {
            near - FLOAT_GAP * em
        } else {
            far + FLOAT_GAP * em
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
        let within = group
            .first()
            .map_or_else(Holders::default, |(_, b)| Holders::of(&b.frame, depth));
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

    pub(super) fn body(&self, frame: &Frame) -> Style<'a> {
        let typo = self.typo;
        Style {
            faces: &self.stacks.body,
            mono: &self.stacks.mono,
            size: typo.font_size,
            line_height: typo.line_height,
            weight: typo.weight,
            tone: if frame.soft { Tone::Soft } else { Tone::Body },
            align: frame.align,
            sided: frame.sided,
            pre: false,
            justify: typo.justify && frame.align == Align::Start,
            hyphens: typo.hyphenate.then_some(self.lang),
            indent: 0.0,
            letter: typo.letter_spacing * typo.font_size,
            word: typo.word_spacing,
            sized: false,
            floor: typo.min_size,
            last: None,
            dir: frame.dir,
            turned: self.turned,
            orient: frame.orient,
            runs: &[],
        }
    }

    /// The block's look, type and layout: the book's unless Book design is Off.
    pub(super) fn look<'f>(&self, frame: &'f Frame) -> Option<&'f Look> {
        let look = frame.look.as_deref()?;
        (self.fixed || self.typo.design != Design::Off).then_some(look)
    }

    /// Whether the block's type takes the design scale, as a designed page.
    pub(super) fn scaled(&self, frame: &Frame) -> bool {
        let Some(look) = self.look(frame) else {
            return false;
        };
        match self.typo.design {
            _ if self.fixed => true,
            Design::On => true,
            Design::Auto => look.designed,
            Design::Off => false,
        }
    }

    /// Book type moved by the reader's settings; the caller adds faces.
    pub(super) fn designed(&self, role: Role, frame: &Frame, look: &Look) -> Style<'a> {
        if self.fixed {
            return self.fixed_style(role, frame, look);
        }
        let (typo, em) = (self.typo, self.em_of(frame));
        let pre = role == Role::Preformatted;
        // Overridden, paragraphs take the reader's spacing outright.
        let own = typo.override_layout && role == Role::Paragraph;
        let line = match (look.line_height, role) {
            _ if own => typo.line_height,
            (Some(l), _) => l * typo.line_height / READER.1,
            (None, Role::Heading(_)) => HEADING_LINE_HEIGHT,
            (None, _) => typo.line_height,
        };
        let copy = matches!(role, Role::Paragraph | Role::Preformatted);
        let scale = if typo.override_font && copy {
            1.0
        } else {
            look.size
        };
        let size = (scale * em).max(typo.min_size).clamp(MIN_SIZE, MAX_SIZE);
        // The reader's indent adds to the book's, as in prose.
        let body = role == Role::Paragraph && !frame.soft && frame.marker.is_none();
        let indent = if body { typo.indent * size } else { 0.0 };
        let book = |v: f32| if own { 0.0 } else { v * em };
        let justify = match own {
            true => typo.justify,
            false => look.justify || typo.justify,
        };
        Style {
            faces: if pre {
                &self.stacks.mono
            } else {
                &self.stacks.body
            },
            size,
            line_height: line.max(MIN_LINE),
            weight: (look.weight + typo.weight - READER.3).clamp(100.0, 900.0),
            align: if pre { Align::Start } else { frame.align },
            pre,
            justify: justify && !pre && frame.align == Align::Start,
            hyphens: (typo.hyphenate && !pre).then_some(self.lang),
            indent: book(look.indent) + indent,
            letter: book(look.letter) + typo.letter_spacing * size,
            word: book(look.word) + typo.word_spacing,
            sized: true,
            last: look.last.filter(|_| !pre),
            ..self.body(frame)
        }
    }

    /// Book type on a fixed page: the reader's settings leave it be.
    pub(super) fn fixed_style(&self, role: Role, frame: &Frame, look: &Look) -> Style<'a> {
        let pre = role == Role::Preformatted;
        let size = (look.size * FIXED_EM).clamp(MIN_SIZE, MAX_SIZE);
        Style {
            faces: if pre {
                &self.stacks.mono
            } else {
                &self.stacks.body
            },
            size,
            line_height: look.line_height.unwrap_or(FIXED_LINE).max(MIN_LINE),
            weight: look.weight.clamp(100.0, 900.0),
            align: if pre { Align::Start } else { frame.align },
            pre,
            justify: look.justify && !pre && frame.align == Align::Start,
            hyphens: None,
            indent: look.indent * FIXED_EM,
            letter: look.letter * FIXED_EM,
            word: look.word * FIXED_EM,
            sized: true,
            floor: MIN_SIZE,
            last: look.last.filter(|_| !pre),
            ..self.body(frame)
        }
    }

    /// The space above a styled block: the book's, moved by the reader's.
    pub(super) fn before(&self, block: &Block, em: f32) -> f32 {
        let book = block.frame.before * em;
        let paragraph = matches!(&block.content, Content::Text(t) if t.role == Role::Paragraph);
        let gap = self.typo.block_gap;
        if self.fixed || !paragraph {
            book
        } else if self.typo.override_layout {
            // Overridden, the reader's gap collapses with the boxes' around.
            (block.frame.outer * em).max(gap * em)
        } else {
            // Otherwise the reader's gap adds to the book's, or takes away.
            (book + (gap - READER.2) * em).max(0.0)
        }
    }

    /// A look's families, then `rest`; prose keeps only embedded fonts.
    pub(super) fn look_faces(
        &self,
        look: &Look,
        laid: bool,
        rest: &[FontFamilyName<'static>],
    ) -> Stack {
        let mut tail = rest;
        let mut names = Vec::new();
        for name in &look.faces {
            match name.as_str() {
                "sans-serif" | "system-ui" | "ui-sans-serif" => tail = &self.stacks.heading,
                "monospace" | "ui-monospace" => tail = &self.stacks.mono,
                _ if laid || name.starts_with(BOOK_FACE) => names.push(name),
                _ => {}
            }
        }
        look_stack(&names, tail)
    }

    pub(super) fn text_style(&self, role: Role, frame: &Frame) -> Style<'a> {
        if let Some(look) = self.look(frame) {
            return self.designed(role, frame, look);
        }
        let body = self.body(frame);
        match role {
            Role::Heading(level) => Style {
                faces: &self.stacks.heading,
                size: body.size * heading_scale(level),
                line_height: HEADING_LINE_HEIGHT,
                weight: HEADING_WEIGHT,
                tone: if level >= 6 { Tone::Muted } else { body.tone },
                justify: false,
                hyphens: None,
                ..body
            },
            // Quotes and list items start flush, as mareader's do.
            Role::Paragraph if !frame.soft && frame.marker.is_none() => Style {
                indent: self.typo.indent * body.size,
                ..body
            },
            Role::Paragraph => body,
            Role::Preformatted => Style {
                faces: &self.stacks.mono,
                size: body.size * PRE_SCALE,
                line_height: PRE_LINE_HEIGHT,
                align: Align::Start,
                pre: true,
                justify: false,
                hyphens: None,
                letter: 0.0,
                word: 0.0,
                ..body
            },
        }
    }

    pub(super) fn cell_style(&self, frame: &Frame, head: bool) -> Style<'a> {
        let body = self.body(frame);
        Style {
            size: body.size * CELL_SCALE,
            weight: if head { HEAD_CELL_WEIGHT } else { body.weight },
            align: Align::Start,
            justify: false,
            ..body
        }
    }
}
