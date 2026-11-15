use super::*;

impl State<'_> {
    pub(super) fn end(&mut self, name: &str) {
        if name == "style" && self.stack.last().is_some_and(|o| o.name == "style") {
            let text = std::mem::take(&mut self.style_text);
            if self.style_applies {
                let sheet = Stylesheet::parse(&text, self.base);
                self.add_sheet(Arc::new(sheet), 0);
            }
        }
        if let Some(at) = self.stack.iter().rposition(|o| o.name == name) {
            self.generate_after(at);
        }
        if is_block(name) {
            self.boundary(name);
        }
        self.end_table(name);
        // Close the innermost match and anything left open in it.
        if let Some(at) = self.stack.iter().rposition(|o| o.name == name) {
            self.close_from(at);
        }
        self.closed_embeds();
        if matches!(name, "ul" | "ol") && !self.in_list() {
            self.list_opened = false;
        }
        match name {
            "rt" | "rtc" if !self.top().note => self.annotate(),
            "ruby" => self.ruby = None,
            _ => {}
        }
        if let Some((_, from)) = self.combining.filter(|c| self.stack.len() <= c.0) {
            self.combining = None;
            self.combine(from);
        }
    }

    /// Sets the text read since `from` across the vertical line.
    pub(super) fn combine(&mut self, from: usize) {
        let Some(t) = self.target() else {
            return;
        };
        let run = t.text.get(from..).unwrap_or_default();
        let lead = run.len() - run.trim_start().len();
        let range = from + lead..from + run.trim_end().len();
        let apart = t.combine.last().is_none_or(|r| r.end <= range.start);
        if !range.is_empty() && apart && t.combine.len() < MAX_COMBINED {
            t.combine.push(range);
        }
    }

    /// Pops the open elements from `at`, ending their extents.
    pub(super) fn close_from(&mut self, at: usize) {
        self.generate_after(at);
        if self.drop.is_some_and(|d| d >= at) {
            self.drop = None;
            self.mark_initial();
        }
        let ends = |o: &Open| o.extent.is_some() || o.boxed || o.part != Part::None;
        let spot = |p: &Place| p.depth >= at && p.spot.is_some();
        let placed = self.places.last().is_some_and(spot);
        if (placed || self.stack.iter().skip(at).any(ends)) && self.table.is_none() {
            self.finish();
        }
        let end = self.first + self.blocks.len();
        let closed: Vec<Open> = self.stack.drain(at..).collect();
        let ended = self.end_anchor();
        for id in closed.iter().filter_map(|o| o.el.id.as_deref()) {
            let id = id.trim();
            if !id.is_empty() && self.refs.ends.len() < MAX_PLACES {
                let key = format!("{}#{id}", self.base);
                self.refs.ends.entry(key).or_insert(ended);
            }
        }
        self.counters.retain(|c| c.2 <= at);
        if self.initial.as_ref().is_some_and(|i| i.0 >= at) {
            self.initial = None;
        }
        if self.first_line.as_ref().is_some_and(|i| i.0 >= at) {
            self.first_line = None;
        }
        for open in closed.iter().rev() {
            if is_block(&open.name) && !open.style.hidden && self.table.is_none() {
                let own = open.name == "p";
                match open.boxed {
                    true => self.close_box(&open.computed, own),
                    false => self.space_below(&open.computed, own),
                }
                self.end_part(open.part);
                if open.computed.break_after {
                    self.chapter = true;
                }
            }
            if open.floated {
                self.float = None;
            }
        }
        self.place_end(at);
        for (key, start, note) in closed.into_iter().filter_map(|o| o.extent) {
            if end > start && self.refs.extents.len() < MAX_PLACES {
                let blocks = start..end;
                self.refs.extents.insert(key, Extent { blocks, note });
            }
        }
    }

    /// Ends an element's part in the layout grid.
    pub(super) fn end_part(&mut self, part: Part) {
        match part {
            Part::None => {}
            Part::Cell => self.end_cell(),
            Part::Row => self.end_row(),
            Part::Table => {
                self.end_row();
                self.grid = None;
            }
            // A flex row let go of has nothing left to end.
            _ if self.grid.as_ref().is_none_or(|g| g.flex.is_none()) => {}
            Part::FlexItem => self.end_cell(),
            Part::FlexBox => {
                self.end_row();
                self.grid = None;
            }
            Part::Stack => {}
            // The space below the item parts it from the next.
            Part::StackItem => {
                let below = self.gap;
                self.end_cell();
                self.end_row();
                (self.grid, self.gap) = (None, below);
            }
        }
    }

    /// A block's top margin, then its border and padding, above what follows.
    pub(super) fn space_above(&mut self, c: &Computed) {
        self.gap.margin(c.margin_px(0), false);
        let inside = c.border[0] + c.padding[0];
        if inside > 0.0 {
            self.gap.fixed(inside);
        }
    }

    /// `own`: the margin is a paragraph's, which a reader's gap may replace.
    pub(super) fn space_below(&mut self, c: &Computed, own: bool) {
        let inside = c.border[2] + c.padding[2];
        if inside > 0.0 {
            self.gap.fixed(inside);
        }
        self.gap.margin(c.margin_px(2), own);
    }

    /// Marks every look of the chapter as designed.
    pub(super) fn design(&mut self) {
        // Keeping the old looks keeps their addresses unique.
        let old = std::mem::take(&mut self.looks);
        let new: HashMap<usize, Arc<Look>> = (old.iter())
            .map(|l| {
                let look = Look {
                    designed: true,
                    ..(**l).clone()
                };
                (Arc::as_ptr(l) as usize, Arc::new(look))
            })
            .collect();
        for block in self.blocks.iter_mut() {
            if let Some(look) = &mut block.frame.look {
                match new.get(&(Arc::as_ptr(look) as usize)) {
                    Some(designed) => *look = Arc::clone(designed),
                    None => Arc::make_mut(look).designed = true,
                }
            }
        }
    }

    pub(super) fn eof(&mut self) {
        self.finish();
        self.flush_table();
        self.close_from(0);
        if self.signals.designed() {
            self.design();
        } else if !self.styled {
            // Unstyled, as converted text is, it reads in the reader's type.
            for block in self.blocks.iter_mut() {
                block.frame.look = None;
            }
        }
        // Ids after the last block mark where the next chapter starts.
        let at = Anchor {
            block: self.first + self.blocks.len(),
            offset: 0,
        };
        for key in self.pending.drain(..) {
            self.refs.places.entry(key).or_insert(at);
        }
    }
}
