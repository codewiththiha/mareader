use super::*;

impl State<'_> {
    pub(super) fn start_table(
        &mut self,
        tag_name: &str,
        tag: &Tag,
        (shown, invisible): (bool, bool),
    ) {
        // Rows and cells not shown take no room, as `display: none`.
        if !shown && matches!(tag_name, "tr" | "td" | "th") {
            return;
        }
        if tag_name == "table" {
            let ordinal = self.tables;
            self.tables += 1;
            // A table that only lays things out reads as plain blocks.
            let scanned = self.layouts.get(ordinal).copied().unwrap_or(false);
            let layout = scanned || typed(tag, "role", &["presentation", "none"]);
            if let Some(t) = self.table.as_mut() {
                t.nested += 1;
            } else if layout {
                self.laying += usize::from(!tag.self_closing);
                self.signals.strong = true;
            } else {
                let mut frame = self.frame();
                frame.hidden = invisible;
                self.table = Some(TableBuild {
                    table: Table::default(),
                    frame,
                    caption: Text::new(Role::Paragraph),
                    nested: 0,
                    in_cell: false,
                    in_caption: false,
                    in_head: false,
                });
            }
            return;
        }
        let Some(t) = self.table.as_mut() else {
            return;
        };
        if t.nested > 0 {
            if matches!(tag_name, "td" | "th")
                && let Some(cell) = t.cell()
            {
                cell.push(" ", Emphasis::default());
            }
            return;
        }
        match tag_name {
            "thead" => t.in_head = true,
            "tbody" | "tfoot" => t.in_head = false,
            "caption" => t.in_caption = !tag.self_closing,
            "tr" => t.table.rows.push(Row {
                cells: Vec::new(),
                head: t.in_head,
            }),
            "td" | "th" => {
                if t.table.rows.is_empty() {
                    t.table.rows.push(Row {
                        cells: Vec::new(),
                        head: t.in_head,
                    });
                }
                let in_head = t.in_head;
                if let Some(row) = t.table.rows.last_mut() {
                    let head = in_head || tag_name == "th";
                    row.head = if row.cells.is_empty() {
                        head
                    } else {
                        row.head && head
                    };
                    if row.cells.len() < MAX_COLUMNS {
                        row.cells.push(Text::new(Role::Paragraph));
                    }
                }
                t.in_cell = true;
            }
            _ => {}
        }
    }

    pub(super) fn end_table(&mut self, name: &str) {
        let Some(t) = self.table.as_mut() else {
            if name == "table" {
                self.laying = self.laying.saturating_sub(1);
            }
            return;
        };
        match name {
            "table" if t.nested > 0 => t.nested -= 1,
            "table" => self.flush_table(),
            "td" | "th" if t.nested == 0 => t.in_cell = false,
            "caption" => t.in_caption = false,
            "thead" => t.in_head = false,
            _ => {}
        }
    }

    /// The caption, centred, then the table; empty ones vanish.
    pub(super) fn flush_table(&mut self) {
        let Some(t) = self.table.take() else {
            return;
        };
        let mut caption = t.caption;
        let mut frame = t.frame;
        caption.trim_end();
        if !caption.is_blank() {
            let mut block = Block::new(Content::Text(caption));
            block.frame = Frame {
                align: Align::Center,
                ..frame.clone()
            };
            self.emit(block);
            frame.before = 0.0;
        }
        let mut table = t.table;
        for cell in table.rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
            cell.trim_end();
        }
        drop_blank_columns(&mut table);
        table.rows.retain(|r| !r.cells.is_empty());
        let mut block = Block::new(Content::Table(table));
        block.frame = frame;
        if !block.is_blank() {
            self.emit(block);
        }
    }
}
