use super::*;

impl State<'_> {
    /// Counts a list item; `value` resets the count.
    /// Gives its number and the list's marker.
    pub(super) fn next_item(&mut self, value: Option<u32>) -> (u32, ListStyle) {
        let depth = self
            .stack
            .iter()
            .filter(|o| o.name == "ul" || o.name == "ol")
            .count();
        let list = self
            .stack
            .iter_mut()
            .rev()
            .find(|o| o.name == "ul" || o.name == "ol");
        let Some(list) = list else {
            return (1, ListStyle::Symbol('•'));
        };
        list.counter = value.unwrap_or_else(|| list.counter.saturating_add(1));
        let style = match depth % 3 {
            _ if list.name == "ol" => ListStyle::Decimal,
            2 => ListStyle::Symbol('◦'),
            0 => ListStyle::Symbol('▪'),
            _ => ListStyle::Symbol('•'),
        };
        (list.counter, style)
    }

    /// Sets a list item's marker: its text, picture and colour.
    pub(super) fn list_marker(
        &mut self,
        tag: &Tag,
        (computed, style): (&Computed, &Style),
        marker: Option<Decl>,
    ) {
        let value = attr(tag, "value").and_then(|v| v.trim().parse::<u32>().ok());
        // An unmarked item still counts, as CSS counters do.
        let (n, own) = self.next_item(value);
        let list_style = computed.list_style.clone().unwrap_or(own);
        let n = i32::try_from(n).unwrap_or(i32::MAX);
        let mut text = generated::marker(n, &list_style).filter(|_| !style.unmarked);
        let picture = (computed.list_image.as_deref()).and_then(|url| self.css_picture(url));
        let mut ink = computed.ink;
        if let Some(d) = marker {
            if let Some(Generated::Items(items)) = &d.content {
                let (shown, _) = self.content_text(items);
                text = Some(shown).filter(|s| !s.trim().is_empty());
            }
            ink = computed.child(&d).ink.or(ink);
        }
        // A hidden item's marker hides with it.
        self.marker_picture = picture.filter(|_| !computed.invisible);
        self.marker = match self.marker_picture.is_some() {
            true => Some(String::new()),
            false => text.filter(|_| !computed.invisible),
        };
        self.marker_colour = ink.and_then(|ink| self.drawn(ink, MAX_FADE));
    }

    /// A picture a stylesheet names, by archive path or `data:` URL.
    pub(super) fn css_picture(&mut self, url: &str) -> Option<Arc<Picture>> {
        match url.starts_with("data:") {
            true => href::data_url(url)
                .and_then(|b| Picture::probe(b).ok())
                .map(Arc::new),
            false => self.library.picture(url),
        }
    }

    /// Applies `d`'s counter resets, increments and sets at `depth`.
    pub(super) fn count(&mut self, d: &Decl, depth: usize) {
        let each = |c: &Option<css::Counters>| c.as_deref().unwrap_or_default().to_vec();
        let resets = each(&d.counter_reset).into_iter().map(|c| (c, Step::Reset));
        let adds = each(&d.counter_increment)
            .into_iter()
            .map(|c| (c, Step::Add));
        let sets = each(&d.counter_set).into_iter().map(|c| (c, Step::Set));
        for ((name, n), step) in resets.chain(adds).chain(sets) {
            // A reset replaces a sibling's; the rest take the innermost.
            let found = match step {
                Step::Reset => (self.counters.iter()).rposition(|c| c.0 == name && c.2 == depth),
                _ => self.counters.iter().rposition(|c| c.0 == name),
            };
            let full = self.counters.len() >= MAX_COUNTERS;
            match found.and_then(|at| self.counters.get_mut(at)) {
                Some(c) if step == Step::Add => c.1 = c.1.saturating_add(n),
                Some(c) => c.1 = n,
                None if full => {}
                None if step == Step::Reset => self.counters.push((name, n, depth)),
                // Never reset, a counter counts from the root.
                None => self.counters.push((name, n, 0)),
            }
        }
    }

    /// A counter's value; `list-item` is the list's own count.
    pub(super) fn counter(&self, name: &str) -> i32 {
        if let Some(c) = self.counters.iter().rev().find(|c| c.0 == name) {
            return c.1;
        }
        let list = self
            .stack
            .iter()
            .rev()
            .find(|o| o.name == "ul" || o.name == "ol");
        match list.filter(|_| name == "list-item") {
            Some(list) => i32::try_from(list.counter).unwrap_or(i32::MAX),
            None => 0,
        }
    }

    /// The next quote mark, opening or closing, by how many are open.
    pub(super) fn quote(&mut self, open: bool) -> String {
        let marks = self.stack.last().and_then(|o| o.computed.quotes.clone());
        let marks = marks.unwrap_or_else(generated::default_quotes);
        if !open {
            if self.quotes == 0 {
                return String::new();
            }
            self.quotes -= 1;
        }
        let depth = self.quotes.min(marks.len().saturating_sub(1));
        let pair = marks.get(depth);
        if open {
            self.quotes = self.quotes.saturating_add(1);
        }
        pair.map(|p| if open { p.0.clone() } else { p.1.clone() })
            .unwrap_or_default()
    }

    /// `content` parts as text, and the pictures after each text run.
    pub(super) fn content_text(&mut self, items: &[Item]) -> (String, Vec<(usize, String)>) {
        let (mut text, mut pictures) = (String::new(), Vec::new());
        for item in items {
            match item {
                Item::Text(t) => text.push_str(t),
                Item::Attr(name) => {
                    let el = self.stack.last().map(|o| &o.el);
                    let attrs = el.into_iter().flat_map(|el| &el.attrs);
                    if let Some((_, v)) = attrs.into_iter().find(|(k, _)| k == name) {
                        text.push_str(v);
                    }
                }
                Item::Counter(name, style) => {
                    text.push_str(&generated::format(self.counter(name), style));
                }
                Item::Counters(name, joint, style) => {
                    let all: Vec<String> = (self.counters.iter())
                        .filter(|c| c.0 == *name)
                        .map(|c| generated::format(c.1, style))
                        .collect();
                    match all.is_empty() {
                        true => text.push_str(&generated::format(self.counter(name), style)),
                        false => text.push_str(&all.join(joint)),
                    }
                }
                Item::OpenQuote => text.push_str(&self.quote(true)),
                Item::CloseQuote => text.push_str(&self.quote(false)),
                Item::NoOpenQuote => self.quotes = self.quotes.saturating_add(1),
                Item::NoCloseQuote => self.quotes = self.quotes.saturating_sub(1),
                Item::Url(url) => pictures.push((text.len(), url.clone())),
            }
        }
        (text, pictures)
    }

    /// Shows a generated box styled by `d`, as the top element's child.
    pub(super) fn generate(&mut self, d: Decl) {
        let Some(Generated::Items(items)) = d.content.clone() else {
            return;
        };
        if self.top().hidden || self.top().graphic {
            return;
        }
        self.count(&d, self.stack.len());
        let (text, pictures) = self.content_text(&items);
        let block = matches!(
            d.display,
            Some(Display::Block | Display::Flex | Display::Grid | Display::Table)
        );
        let edged = (d.border_style.iter()).any(|s| s.is_some_and(|s| s != BorderStyle::None));
        let painted = d.background == Some(true) || d.background_image == Some(true);
        let shows = edged || painted || d.height.is_some();
        // An empty box drawing nothing, as a clearfix, is skipped.
        if text.trim().is_empty() && pictures.is_empty() && !(block && shows) {
            return;
        }
        let name = if block { "div" } else { "span" };
        let mut d = d;
        if block {
            d.display = Some(Display::Block);
        }
        self.forced = Some(d);
        self.start(&synthetic(name));
        let mut from = 0;
        for (at, url) in pictures {
            self.read_text(text.get(from..at).unwrap_or_default());
            from = at;
            let picture = self.css_picture(&url);
            self.picture(picture, None, (None, None));
        }
        self.read_text(text.get(from..).unwrap_or_default());
        self.end(name);
    }

    /// Shows the `::after` boxes of open elements from `at`, innermost first.
    pub(super) fn generate_after(&mut self, at: usize) {
        loop {
            let open = |o: &Open| o.after.is_some();
            let Some(i) = (at..self.stack.len())
                .rev()
                .find(|&i| self.stack.get(i).is_some_and(open))
            else {
                return;
            };
            let after = self.stack.get_mut(i).and_then(|o| o.after.take());
            if i + 1 < self.stack.len() {
                self.close_from(i + 1);
            }
            if let Some(d) = after {
                self.generate(*d);
            }
        }
    }

    /// Marks a short, large floated run starting the block as its drop cap.
    pub(super) fn mark_initial(&mut self) {
        let Some(Content::Text(t)) = self.current.as_mut().map(|b| &mut b.content) else {
            return;
        };
        let len = t.text.len();
        let short = (1..=3).contains(&t.text.trim().chars().count()) && t.inlines.is_empty();
        let large =
            (t.spans.first()).is_some_and(|s| s.range == (0..len) && s.emphasis.size >= 150);
        if short && large && !t.text.starts_with(' ') {
            t.initial = len;
        }
    }
}
