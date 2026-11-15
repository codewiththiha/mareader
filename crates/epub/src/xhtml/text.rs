use super::*;

impl State<'_> {
    /// Text, its block's first letter and line styled as rules say.
    pub(super) fn characters(&mut self, input: &str) {
        let pending = self.initial.is_some() || self.first_line.is_some();
        let fresh = pending && self.table.is_none() && !self.top().pre && self.fresh();
        if !fresh || input.chars().all(is_html_space) || self.top().hidden {
            return self.read_text(input);
        }
        let line = self.first_line.take().map(|(_, d)| self.emphasis_with(d));
        let body = input.trim_start_matches(is_html_space);
        let split = first_letter(body);
        match self.initial.take().filter(|_| split > 0) {
            Some((_, d)) => {
                let (letter, rest) = body.split_at(split);
                self.read_text(input.get(..input.len() - body.len()).unwrap_or_default());
                self.forced = Some(d);
                self.start(&synthetic("span"));
                self.read_text(letter);
                self.end("span");
                self.read_text(rest);
            }
            None => self.read_text(input),
        }
        if let (Some(line), Some(Content::Text(t))) =
            (line, self.current.as_mut().map(|b| &mut b.content))
        {
            t.first_line = Some(line);
        }
    }

    /// Whether the open block has no text yet.
    pub(super) fn fresh(&self) -> bool {
        match self.current.as_ref().map(|b| &b.content) {
            None => true,
            Some(Content::Text(t)) => t.text.trim().is_empty() && t.inlines.is_empty(),
            Some(_) => false,
        }
    }

    /// The emphasis text styled by `d` would take, here.
    pub(super) fn emphasis_with(&mut self, d: Decl) -> Emphasis {
        self.forced = Some(d);
        self.start(&synthetic("span"));
        let emphasis = self.emphasis();
        self.end("span");
        emphasis
    }

    pub(super) fn read_text(&mut self, input: &str) {
        let style = self.top();
        if self.stack.last().is_some_and(|o| o.name == "style") {
            if self.style_text.len() + input.len() <= MAX_SHEET {
                self.style_text.push_str(input);
            }
            return;
        }
        if style.hidden || style.graphic {
            return;
        }
        if style.note {
            return self.read_note(input);
        }
        let emphasis = self.emphasis();
        let face = self.run_face();
        if style.pre {
            let mut text = input.replace('\r', "").replace('\t', TAB);
            if std::mem::take(&mut self.pre_opened) && text.starts_with('\n') {
                text.remove(0);
            }
            if let Some(t) = self.target() {
                let emphasis = faced(t, face, emphasis);
                put(t, &text, emphasis, style.link);
            }
            return;
        }
        if input.chars().all(is_html_space) {
            self.space |= !input.is_empty();
            return;
        }
        let mut space = self.space;
        let Some(target) = self.target() else {
            return;
        };
        let mut text = String::with_capacity(input.len());
        for c in input.chars() {
            if is_html_space(c) {
                space = true;
                continue;
            }
            let after_picture = target
                .inlines
                .last()
                .is_some_and(|i| i.at == target.text.len());
            let line_start = text.is_empty()
                && !after_picture
                && (target.text.is_empty() || target.text.ends_with('\n'));
            if std::mem::take(&mut space) && !line_start {
                text.push(' ');
            }
            text.push(c);
        }
        let emphasis = faced(target, face, emphasis);
        put(target, &text, emphasis, style.link);
        self.space = space;
    }

    /// Ruby note text, its spaces collapsed.
    pub(super) fn read_note(&mut self, input: &str) {
        let words: Vec<&str> = input
            .split(is_html_space)
            .filter(|w| !w.is_empty())
            .collect();
        if words.is_empty() || self.note.len() > MAX_NOTE {
            return;
        }
        if !self.note.is_empty() && input.starts_with(is_html_space) {
            self.note.push(' ');
        }
        self.note.push_str(&words.join(" "));
    }

    /// Bytes in the text being read, if a block is open.
    pub(super) fn text_len(&mut self) -> usize {
        if self.current.is_none() && self.table.is_none() {
            return 0;
        }
        self.target().map_or(0, |t| t.text.len())
    }

    /// The note just read, set over the base before it.
    pub(super) fn annotate(&mut self) {
        let note = std::mem::take(&mut self.note);
        let Some((start, Some(end))) = self.ruby else {
            return;
        };
        let Some(t) = self.target() else {
            return;
        };
        let base = t.text.get(start..end.min(t.text.len())).unwrap_or_default();
        let lead = base.len() - base.trim_start().len();
        let range = start + lead..start + base.trim_end().len();
        let apart = t.ruby.last().is_none_or(|a| a.range.end <= range.start);
        let fits = !note.is_empty() && note.len() <= MAX_NOTE;
        if fits && !range.is_empty() && apart {
            t.ruby.push(Annotation { range, text: note });
        }
        let next = t.text.len();
        self.ruby = Some((next, None));
    }

    /// The book's family for the run being read, where it isn't its block's.
    pub(super) fn run_face(&mut self) -> Option<Vec<String>> {
        let own = self.stack.last().and_then(|o| o.computed.family.clone())?;
        let block = self.block_computed().and_then(|c| c.family.clone());
        if block.as_deref() == Some(&*own) {
            return None;
        }
        Some(self.faces(Some(&own))).filter(|f| !f.is_empty())
    }

    pub(super) fn line_break(&mut self) {
        let pre = self.top().pre;
        let in_table = self.table.is_some();
        let blank = (!in_table && !self.top().hidden).then(|| self.block_style().line_px());
        let target = if pre || in_table {
            self.target()
        } else {
            self.current.as_mut().and_then(|b| match &mut b.content {
                Content::Text(t) => Some(t),
                _ => None,
            })
        };
        match target.filter(|t| pre || !t.text.is_empty()) {
            Some(t) => t.push("\n", Emphasis::default()),
            // A break with nothing before it holds a blank line open.
            None => {
                if let Some(line) = blank {
                    self.gap.fixed(line);
                }
            }
        }
        self.space = false;
    }

    /// A block boundary: new block outside tables, new line in a cell.
    pub(super) fn boundary(&mut self, name: &str) {
        let Some(table) = self.table.as_mut() else {
            return self.finish();
        };
        let structural = matches!(
            name,
            "table" | "tr" | "td" | "th" | "thead" | "tbody" | "tfoot" | "caption"
        );
        if !structural
            && let Some(cell) = table.cell()
            && !cell.text.is_empty()
            && !cell.text.ends_with('\n')
        {
            cell.push("\n", Emphasis::default());
        }
        self.space = false;
    }

    /// Ends elements HTML closes implicitly, like `<p>` before `<div>`.
    pub(super) fn close_implied(&mut self, name: &str) {
        let closes: &[&str] = match name {
            "li" => &["p", "li"],
            "dt" | "dd" => &["p", "dt", "dd"],
            "tr" => &["p", "tr", "td", "th"],
            "td" | "th" => &["p", "td", "th"],
            _ if is_block(name) => &["p"],
            _ => &[],
        };
        while let Some(at) = self.stack.iter().rposition(|o| is_block(&o.name)) {
            let innermost = self.stack.get(at).map(|o| o.name.as_str());
            if !innermost.is_some_and(|n| closes.contains(&n)) {
                break;
            }
            self.close_from(at);
        }
    }
}
