use super::*;

impl State<'_> {
    pub(super) fn top(&self) -> Style {
        self.stack.last().map(|o| o.style).unwrap_or_default()
    }

    pub(super) fn in_list(&self) -> bool {
        self.stack.iter().any(|o| o.name == "ul" || o.name == "ol")
    }

    /// Indents and quote bars from the open lists and quotes.
    pub(super) fn frame(&mut self) -> Frame {
        let mut f = Frame::default();
        for open in &self.stack {
            match open.name.as_str() {
                "ul" | "ol" | "dd" => f.inset += LIST_INSET,
                "blockquote" => {
                    if f.bars.len() < MAX_BARS {
                        f.bars.push(f.inset);
                    }
                    f.inset += QUOTE_INSET;
                    f.soft = true;
                }
                _ => {}
            }
        }
        f.align = self.top().align;
        f.sided = self.top().sided;
        f.tight = self.in_list() && !std::mem::take(&mut self.list_opened);
        f.look = Some(self.look());
        f.boxes = self.box_arcs();
        f.cell = self.cell();
        f.float = self.float.as_ref().map(|(_, f)| Arc::clone(f));
        let turns = self.stack.iter().map(|o| o.computed.rotate);
        f.rotate = turns.sum::<f32>() % 360.0;
        let (before, outer) = self.gap.take_both();
        (f.before, f.outer) = (before / ROOT_PX, outer / ROOT_PX);
        if let Some(c) = self.block_computed() {
            f.keep = Keep {
                orphans: c.orphans,
                widows: c.widows,
                whole: c.whole,
                with_next: c.keep_next,
                with_last: c.keep_last,
            };
            f.dir = c.dir;
            f.orient = c.orient;
        }
        f.passages = self
            .stack
            .iter()
            .filter_map(|o| o.passage.clone())
            .collect();
        f.turned = self.turned;
        f.sheet = self.sheet.clone();
        f.spot = self.places.last().and_then(|p| p.spot.clone());
        f
    }

    /// The open boxes, each made at its first block with its top inside.
    pub(super) fn box_arcs(&mut self) -> ThinVec<Arc<Boxed>> {
        let (total, first) = (self.gap.total(), self.blocks.len());
        let mut arcs = ThinVec::with_capacity(self.boxes.len());
        for b in &mut self.boxes {
            if b.arc.is_none() {
                let mut spec = b.spec.clone();
                if spec.border[0] + spec.padding[0] > 0.0 {
                    spec.top = (total - b.gap.total()).max(0.0) / ROOT_PX;
                }
                (b.arc, b.first) = (Some(Arc::new(spec)), first);
            }
            arcs.extend(b.arc.clone());
        }
        arcs
    }

    /// The grid cell being read, if any.
    pub(super) fn cell(&self) -> Option<Cell> {
        let row = self.grid.as_ref()?.row.as_ref()?;
        Some(Cell {
            row: Arc::clone(&row.placeholder),
            column: row.column?,
            depth: row.boxes,
        })
    }

    /// Whether the table `tag` starting now only lays things out.
    pub(super) fn lays_out(&self, tag: &Tag) -> bool {
        let scanned = self.layouts.get(self.tables).copied().unwrap_or(false);
        self.table.is_none() && (scanned || typed(tag, "role", &["presentation", "none"]))
    }

    /// Starts `name`'s part in the layout grid, if it has one.
    pub(super) fn grid_start(&mut self, name: &str, tag: &Tag, c: &Computed) -> Part {
        if name == "table" {
            if self.grid.is_some() && !tag.self_closing && self.lays_out(tag) {
                self.dissolve_flex();
            }
            if self.grid.is_some() || tag.self_closing || !self.lays_out(tag) {
                return Part::None;
            }
            let depth = self.stack.len();
            let both = c.margin.get(1).zip(c.margin.get(3));
            let columns = Columns {
                flex: None,
                fill: !matches!(c.width, Len::Auto),
                centre: both.is_some_and(|(r, l)| *r == Len::Auto && *l == Len::Auto),
            };
            self.grid = Some(GridBuild {
                depth,
                row: None,
                columns: Arc::new(columns),
                flex: None,
                taken: Vec::new(),
                single: false,
            });
            return Part::Table;
        }
        if let Some(part) = self.flex_start(name, tag, c) {
            return part;
        }
        // Only the grid's own rows and cells, not a nested table's.
        let table = self.stack.iter().rposition(|o| o.name == "table");
        if self.grid.as_ref().is_none_or(|g| Some(g.depth) != table) {
            return Part::None;
        }
        match name {
            "tr" => {
                self.end_row();
                let row = RowBuild::new(self.blocks.len());
                if let Some(grid) = self.grid.as_mut() {
                    grid.row = Some(row);
                }
                Part::Row
            }
            "td" | "th" => self.cell_start(tag, c),
            _ => Part::None,
        }
    }

    fn cell_start(&mut self, tag: &Tag, c: &Computed) -> Part {
        let tr = self.stack.iter().rev().find(|o| o.name == "tr");
        let valign = c.valign.or(tr.and_then(|o| o.computed.valign));
        let fresh = self.grid.as_ref().and_then(|g| g.row.as_ref());
        let fresh = fresh.is_none_or(|r| r.widths.is_empty());
        // The first cell takes the space above the row.
        let before = match fresh {
            true => {
                self.box_arcs();
                self.gap.take() / ROOT_PX
            }
            false => std::mem::take(&mut self.gap).total() / ROOT_PX,
        };
        let (first, boxes) = (self.blocks.len(), self.boxes.len());
        let Some(grid) = self.grid.as_mut() else {
            return Part::None;
        };
        let row = grid.row.get_or_insert_with(|| RowBuild::new(first));
        if fresh {
            row.before = before;
        }
        if row.widths.len() < MAX_COLUMNS {
            // The first column no cell above still spans down into.
            let span = |name: &str| {
                let n = attr(tag, name).and_then(|v| v.trim().parse::<u16>().ok());
                n.unwrap_or(1).clamp(1, MAX_COLUMNS as u16)
            };
            let (across, down) = (span("colspan"), span("rowspan").min(MAX_SPAN_DOWN));
            let taken = |c: usize| grid.taken.get(c).is_some_and(|t| *t > 0);
            let mut start = usize::from(row.next);
            while taken(start) && start < MAX_COLUMNS {
                start += 1;
            }
            let end = (start + usize::from(across)).min(MAX_COLUMNS);
            if down > 1 {
                grid.taken.resize(grid.taken.len().max(end), 0);
                for t in grid.taken.iter_mut().take(end).skip(start) {
                    *t = down;
                }
            }
            row.widths.push(cell_width(c));
            row.valign.push(valign.unwrap_or_default());
            row.starts.push(u16::try_from(start).unwrap_or(0));
            row.spans.push(across);
            row.down.push(down);
            row.next = u16::try_from(end).unwrap_or(u16::MAX);
        }
        let column = row.widths.len().saturating_sub(1);
        row.column = u16::try_from(column).ok();
        row.boxes = u8::try_from(boxes).unwrap_or(u8::MAX);
        Part::Cell
    }

    /// Starts a flex row's container or one of its items, if `name` is.
    pub(super) fn flex_start(&mut self, name: &str, tag: &Tag, c: &Computed) -> Option<Part> {
        let flex = c.display == Display::Flex && name != "table" && !tag.self_closing;
        let row = flex && !c.flex_column;
        // A column stack's items after the first keep its row gap above.
        let stacked = self
            .stack
            .last()
            .filter(|o| o.part == Part::Stack && o.count > 1);
        if let Some(gap) = stacked.map(|o| o.computed.row_gap).filter(|g| *g > 0.0) {
            self.gap.fixed(gap);
        }
        // A row of one inside lets go, so this row lays out instead.
        let single = self.grid.as_ref().is_some_and(|g| g.single);
        if row && (self.grid.is_none() || (single && self.dissolve_flex())) {
            let flex = Flex {
                justify: c.justify,
                gap: c.gap / ROOT_PX,
                wrap: c.wrap,
            };
            let columns = Columns {
                flex: Some(flex),
                ..Columns::default()
            };
            self.grid = Some(GridBuild {
                depth: self.stack.len(),
                row: Some(RowBuild::new(self.blocks.len())),
                columns: Arc::new(columns),
                flex: Some(c.align_items.unwrap_or(VAlign::Top)),
                taken: Vec::new(),
                single: false,
            });
            return Some(Part::FlexBox);
        }
        if let Some(part) = self.stack_item(c) {
            return Some(part);
        }
        let item = (self.grid.as_ref()).is_some_and(|g| g.depth + 1 == self.stack.len());
        if flex && !item {
            return Some(Part::Stack);
        }
        let grid = self.grid.as_ref()?;
        let align = grid.flex?;
        if grid.depth + 1 != self.stack.len() {
            return Some(Part::None);
        }
        let fresh = grid.row.as_ref().is_none_or(|r| r.widths.is_empty());
        let before = match fresh {
            true => {
                self.box_arcs();
                self.gap.take() / ROOT_PX
            }
            false => std::mem::take(&mut self.gap).total() / ROOT_PX,
        };
        let (first, boxes) = (self.blocks.len(), self.boxes.len());
        let row = self
            .grid
            .as_mut()?
            .row
            .get_or_insert_with(|| RowBuild::new(first));
        if fresh {
            row.before = before;
        }
        if row.widths.len() >= MAX_COLUMNS {
            return Some(Part::None);
        }
        let basis = match c.basis {
            Len::Auto => cell_width(c),
            Len::Px(px) if px > 0.0 => Some(Length::Em(px / ROOT_PX)),
            Len::Percent(p) if p > 0.0 => Some(Length::Percent(p)),
            Len::Mix { px, pct, .. } => mixed(px, pct),
            _ => None,
        };
        row.widths.push(basis);
        row.valign.push(c.align_self.unwrap_or(align));
        row.grow.push(c.grow);
        row.column = u16::try_from(row.widths.len() - 1).ok();
        row.boxes = u8::try_from(boxes).unwrap_or(u8::MAX);
        Some(Part::FlexItem)
    }

    /// Starts an item of the column stack holding it, as a row of one.
    pub(super) fn stack_item(&mut self, c: &Computed) -> Option<Part> {
        let parent = self.stack.last().filter(|o| o.part == Part::Stack)?;
        let p = &parent.computed;
        let stretch = (c.self_stretch).unwrap_or(p.items_stretch && c.align_self.is_none());
        let justify = match c.align_self.or(p.align_items) {
            _ if stretch => Justify::Start,
            Some(VAlign::Middle) => Justify::Center,
            Some(VAlign::Bottom) => Justify::End,
            _ => Justify::Start,
        };
        let width = cell_width(c);
        // Stretched across, an item lays out as it would in prose.
        if stretch && width.is_none() {
            return None;
        }
        let single = self.grid.as_ref().is_some_and(|g| g.single);
        if self.grid.is_some() && !(single && self.dissolve_flex()) {
            return None;
        }
        let depth = self.stack.len() - 1;
        self.box_arcs();
        let mut row = RowBuild::new(self.blocks.len());
        row.before = self.gap.take() / ROOT_PX;
        row.grow.push(0.0);
        row.widths.push(width);
        row.valign.push(VAlign::Top);
        (row.column, row.boxes) = (Some(0), u8::try_from(self.boxes.len()).unwrap_or(u8::MAX));
        let flex = Flex {
            justify,
            gap: 0.0,
            wrap: false,
        };
        let columns = Columns {
            flex: Some(flex),
            ..Columns::default()
        };
        self.grid = Some(GridBuild {
            depth,
            row: Some(row),
            columns: Arc::new(columns),
            flex: Some(VAlign::Top),
            taken: Vec::new(),
            single: true,
        });
        Some(Part::StackItem)
    }

    /// Lets go of a one-item flex row: its layout table becomes the grid.
    pub(super) fn dissolve_flex(&mut self) -> bool {
        let single = self.grid.as_ref().is_some_and(|g| {
            g.flex.is_some() && g.row.as_ref().is_none_or(|r| r.widths.len() <= 1)
        });
        if !single {
            return false;
        }
        let Some(row) = self.grid.take().and_then(|g| g.row) else {
            return true;
        };
        let mut emitted = false;
        for block in self.blocks.iter_mut().skip(row.first) {
            emitted = true;
            if block
                .frame
                .cell
                .as_ref()
                .is_some_and(|c| Arc::ptr_eq(&c.row, &row.placeholder))
            {
                block.frame.cell = None;
            }
        }
        if !emitted {
            self.gap.fixed(row.before * ROOT_PX);
        }
        true
    }

    /// Ends a grid cell: what follows is outside it.
    pub(super) fn end_cell(&mut self) {
        self.gap = Gap::default();
        if let Some(row) = self.grid.as_mut().and_then(|g| g.row.as_mut()) {
            row.column = None;
        }
    }

    /// Ends the grid row, giving its blocks the row they share.
    pub(super) fn end_row(&mut self) {
        let Some(row) = self.grid.as_mut().and_then(|g| g.row.take()) else {
            return;
        };
        let Some(grid) = self.grid.as_mut() else {
            return;
        };
        for t in &mut grid.taken {
            *t = t.saturating_sub(1);
        }
        let real = Arc::new(GridRow {
            widths: row.widths,
            valign: row.valign,
            before: row.before,
            starts: row.starts,
            spans: row.spans,
            grow: row.grow,
            down: row.down,
            columns: Arc::clone(&grid.columns),
        });
        for block in self.blocks.iter_mut().skip(row.first) {
            if let Some(cell) = block.frame.cell.as_mut()
                && Arc::ptr_eq(&cell.row, &row.placeholder)
            {
                cell.row = Arc::clone(&real);
            }
        }
        self.gap = Gap::default();
    }
}
