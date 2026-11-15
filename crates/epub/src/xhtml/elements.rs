use super::*;

impl State<'_> {
    /// Returns whether the tokenizer should read raw text next.
    pub(super) fn start(&mut self, tag: &Tag) -> bool {
        let name: &str = &tag.name;
        let forced = self.forced.take();
        let generated = forced.is_some();
        let el = self.start_element(tag, name, generated);
        let quiet = self.top().hidden || self.top().graphic;
        let (declared, pseudos, mut computed) = self.start_computed(tag, &el, forced, quiet);
        let shown = self.start_shown(tag, &el, &mut computed, &declared, generated);
        let (before, after) = match shown {
            true => (pseudos.before, pseudos.after),
            false => (None, None),
        };
        if name == "hr" && (before.is_some() || after.is_some()) && self.table.is_none() {
            self.ornament(declared, (before, after));
            return false;
        }
        if name == "hr" && shown && self.table.is_none() && styled_rule(&declared) {
            self.rule_box(declared);
            return false;
        }
        let signals = self.start_page(name, &computed, &declared, shown);
        let (placed, id) = self.start_position(name, tag, &computed, shown);
        if self.start_void(name, tag, &computed, shown) {
            return false;
        }
        let (extent, boxed, part, floated) =
            self.start_block(name, tag, &computed, &declared, (shown, placed), id);
        let empty = tag.self_closing && before.is_none() && after.is_none();
        if self.close_unopened(name, tag, &computed, (shown, empty), boxed, part) {
            return false;
        }
        if matches!(name, "ul" | "ol") && !self.in_list() {
            self.list_opened = true;
        }
        let (counter, mut style) = self.start_style(name, tag, &computed, shown, pseudos.marker);
        self.start_inline(
            name,
            tag,
            &computed,
            (shown, generated),
            (pseudos.letter, pseudos.line),
            &mut style,
        );
        let passage = self.start_passage(name, tag);
        self.stack.push(Open {
            name: name.to_owned(),
            style,
            computed,
            el,
            kids: Vec::new(),
            count: 0,
            types: Vec::new(),
            counter,
            extent,
            boxed,
            part,
            floated,
            signals,
            after: after.map(Box::new),
            passage,
        });
        if name == "style" {
            self.style_text.clear();
            self.style_applies = attr(tag, "media").is_none_or(query::media);
        }
        if let Some(d) = before {
            self.generate(d);
        }
        if tag.self_closing {
            self.end(name);
            return false;
        }
        is_raw(name)
    }

    fn start_element(&mut self, tag: &Tag, name: &str, generated: bool) -> El {
        if !generated {
            self.close_implied(name);
        }
        let el = self.element(tag, name);
        match name {
            "html" if self.stack.is_empty() => self.root = Some(el.clone()),
            "body" => self.root_vars(),
            "meta" if self.fixed && attr(tag, "name").is_some_and(|n| n.trim() == "viewport") => {
                self.viewport = attr(tag, "content").and_then(viewport).or(self.viewport);
            }
            "source" => self.source(tag),
            "track" => self.track(tag),
            _ => {}
        }
        el
    }

    fn start_computed(
        &mut self,
        tag: &Tag,
        el: &El,
        forced: Option<Decl>,
        quiet: bool,
    ) -> (Decl, Pseudos, Computed) {
        let (declared, pseudos) = match forced {
            Some(d) => (d, Pseudos::default()),
            None if quiet => (self.declared(el, tag), Pseudos::default()),
            None => {
                let declared = self.declared(el, tag);
                let pseudos = self.pseudos(el, &declared);
                (declared, pseudos)
            }
        };
        self.styled |= attr(tag, "style").is_some();
        let computed = match self.stack.last() {
            Some(open) => open.computed.child(&declared),
            None => Computed::default().child(&declared),
        };
        (declared, pseudos, computed)
    }

    fn start_shown(
        &mut self,
        tag: &Tag,
        el: &El,
        computed: &mut Computed,
        declared: &Decl,
        generated: bool,
    ) -> bool {
        if !generated {
            self.trigger_target(tag, computed);
            self.add_target(tag);
        }
        if !generated {
            self.adopt(el);
        }
        let shown = !self.top().hidden && computed.display != Display::None;
        if shown && !generated {
            self.count(declared, self.stack.len());
        }
        shown
    }

    fn start_page(
        &mut self,
        name: &str,
        computed: &Computed,
        declared: &Decl,
        shown: bool,
    ) -> bool {
        let signals = shown && self.observe(name, computed, declared);
        if matches!(name, "html" | "body") {
            self.page = computed.fill.unwrap_or(self.page);
            self.book_ink = computed.ink.unwrap_or(self.book_ink);
            // Only the chapter's root turns it; inner boxes stay level.
            self.turned = computed.writing && !self.fixed;
        }
        if name == "body" && self.fixed && self.sheet.is_none() {
            self.open_sheet(computed);
        }
        signals
    }

    fn close_unopened(
        &mut self,
        name: &str,
        tag: &Tag,
        computed: &Computed,
        (shown, empty): (bool, bool),
        boxed: bool,
        part: Part,
    ) -> bool {
        // `<video src='…'/>` has no content to read, so its card shows now.
        if empty && !is_void(name) {
            self.holds_back(name, tag, (computed, shown));
            self.closed_embeds();
        }
        if is_void(name) || empty || self.stack.len() >= MAX_DEPTH {
            if shown && is_block(name) && self.table.is_none() {
                self.finish();
                let own = name == "p";
                match boxed {
                    true => self.close_box(computed, own),
                    false => self.space_below(computed, own),
                }
                self.end_part(part);
            }
            self.place_end(self.stack.len());
            return true;
        }
        false
    }

    fn start_position<'a>(
        &mut self,
        name: &str,
        tag: &'a Tag,
        computed: &Computed,
        shown: bool,
    ) -> (bool, Option<&'a str>) {
        let placed = shown && self.place_start(computed);
        if computed.combine && self.combining.is_none() && !is_block(name) {
            self.combining = Some((self.stack.len(), self.text_len()));
        }
        let id = attr(tag, "id").or_else(|| attr(tag, "name").filter(|_| name == "a"));
        if let Some(id) = id.filter(|_| !is_block(name)) {
            self.mark(id);
        }
        self.start_ruby(name);
        (placed, id)
    }

    fn start_ruby(&mut self, name: &str) {
        match name {
            "ruby" => self.ruby = Some((self.text_len(), None)),
            "rt" | "rtc" if !self.top().note => {
                let end = self.text_len();
                if let Some((_, base)) = &mut self.ruby {
                    *base = Some(end);
                }
                self.note.clear();
            }
            _ => {}
        }
    }

    fn start_void(&mut self, name: &str, tag: &Tag, computed: &Computed, shown: bool) -> bool {
        match name {
            "br" => {
                self.line_break();
                return true;
            }
            "hr" => {
                self.boundary(name);
                if self.table.is_none() && shown {
                    self.space_above(computed);
                    let mut rule = Block::new(Content::Rule);
                    rule.frame = self.frame();
                    rule.frame.hidden = computed.invisible;
                    self.emit(rule);
                    self.space_below(computed, false);
                }
                return true;
            }
            "img" => {
                let width = picture_width(computed);
                let alt = attr(tag, "alt");
                let picked = self.sources.as_mut().and_then(|s| s.1.take());
                let set = attr(tag, "srcset").and_then(media::best_candidate);
                let tries = [picked.as_deref(), attr(tag, "src"), set.as_deref()];
                let picture = tries.into_iter().flatten().find_map(|s| self.load(s));
                if let Some(p) = &picture {
                    self.areas = self.map_areas(tag, p);
                }
                let link = self.observer_link(tag);
                self.areas.extend(link.map(Area::whole));
                self.inline_link = link;
                if shown {
                    self.picture(picture, alt, (width, Some(computed)));
                }
                self.areas.clear();
                self.inline_link = None;
                self.place_end(self.stack.len());
                return true;
            }
            "embed" => {
                if shown {
                    self.embedded((attr(tag, "src"), attr(tag, "type")), computed);
                }
                self.place_end(self.stack.len());
                return true;
            }
            "image" => {
                let href = attr(tag, "href").or_else(|| attr(tag, "xlink:href"));
                self.image(href, None, (None, None));
            }
            "link" => {
                self.link(tag);
                return true;
            }
            _ => {}
        }
        false
    }

    fn start_block(
        &mut self,
        name: &str,
        tag: &Tag,
        computed: &Computed,
        declared: &Decl,
        (shown, placed): (bool, bool),
        id: Option<&str>,
    ) -> (Option<(String, usize, bool)>, bool, Part, bool) {
        let mut extent = None;
        if is_block(name) {
            self.boundary(name);
            if let Some(id) = id {
                self.mark(id);
                let key = format!("{}#{}", self.base, id.trim());
                extent = Some((key, self.first + self.blocks.len(), is_note(tag)));
            }
        }
        let (mut boxed, mut part) = (false, Part::None);
        if is_block(name) && shown && computed.break_before && self.table.is_none() {
            self.chapter = true;
        }
        let floats = computed.float != Float::None && is_block(name) && shown;
        let floated = floats && self.float.is_none() && self.table.is_none() && !tag.self_closing;
        if floated {
            let depth = u8::try_from(self.boxes.len()).unwrap_or(u8::MAX);
            let right = computed.float == Float::Right;
            self.float = Some((self.stack.len(), Arc::new(Floated { right, depth })));
        }
        if is_block(name) && shown && self.table.is_none() {
            part = self.grid_start(name, tag, computed);
            // A positioned box's place is its own: no margins, no width.
            let own = placed.then(|| Computed {
                width: Len::Auto,
                margin: [Len::Px(0.0); 4],
                ..computed.clone()
            });
            let c = own.as_ref().unwrap_or(computed);
            boxed = self.open_block(tag, (c, declared), part);
        }
        if matches!(
            name,
            "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" | "caption"
        ) {
            self.start_table(name, tag, (shown, computed.invisible));
            if matches!(name, "table" | "td" | "th") {
                self.rule(computed);
            }
        }
        (extent, boxed, part, floated)
    }

    fn start_style(
        &mut self,
        name: &str,
        tag: &Tag,
        computed: &Computed,
        shown: bool,
        marker: Option<Decl>,
    ) -> (u32, Style) {
        let counter = attr(tag, "start")
            .and_then(|s| s.trim().parse::<u32>().ok())
            .map_or(0, |s| s.saturating_sub(1));
        let mut style = self.style_for(name, computed);
        style.hidden |= self.holds_back(name, tag, (computed, shown));
        if name == "li" {
            self.list_marker(tag, (computed, &style), marker);
        }
        (counter, style)
    }

    fn start_inline(
        &mut self,
        name: &str,
        tag: &Tag,
        computed: &Computed,
        (shown, generated): (bool, bool),
        (letter, line): (Option<Decl>, Option<Decl>),
        style: &mut Style,
    ) {
        let floated_inline = !is_block(name) && computed.float != Float::None;
        if floated_inline && shown && self.table.is_none() && self.drop.is_none() && self.fresh() {
            self.drop = Some(self.stack.len());
        }
        if is_block(name) && shown {
            let depth = self.stack.len();
            if let Some(d) = letter {
                self.initial = Some((depth, d));
            }
            if let Some(d) = line {
                self.first_line = Some((depth, d));
            }
        }
        if let Some(href) = attr(tag, "href").filter(|_| name == "a") {
            style.link = self.link_to(href);
            if let Some(i) = style.link.filter(|_| is_noteref(tag)) {
                self.refs.noterefs.insert(i);
            }
        }
        if !generated && let Some(link) = self.observer_link(tag) {
            style.link = Some(link);
        }
        self.pre_opened = name == "pre";
    }

    fn start_passage(&self, name: &str, tag: &Tag) -> Option<Arc<Passage>> {
        passage_kind(name, tag)
            .filter(|&kind| {
                !self
                    .stack
                    .iter()
                    .flat_map(|o| &o.passage)
                    .any(|p| p.kind == kind)
            })
            .map(|kind| Arc::new(Passage { kind }))
    }

    /// An `<hr>` showing generated content: a centred ornament, no line.
    pub(super) fn ornament(&mut self, mut d: Decl, (before, after): (Option<Decl>, Option<Decl>)) {
        d.border_style = [Some(BorderStyle::None); 4];
        d.display = Some(Display::Block);
        self.forced = Some(d);
        self.start(&synthetic("div"));
        for d in [before, after].into_iter().flatten() {
            self.generate(d);
        }
        self.end("div");
    }

    /// An `<hr>` the book draws itself: a box of its borders and ground.
    pub(super) fn rule_box(&mut self, mut d: Decl) {
        d.display = Some(Display::Block);
        self.forced = Some(d);
        self.rule_next = true;
        self.start(&synthetic("div"));
        self.end("div");
        self.rule_next = false;
    }
}
