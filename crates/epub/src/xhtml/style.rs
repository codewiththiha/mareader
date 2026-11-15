use super::*;

impl State<'_> {
    pub(super) fn style_for(&self, name: &str, c: &Computed) -> Style {
        let mut st = self.top();
        match name {
            "code" | "kbd" | "samp" | "tt" => st.code = true,
            "pre" | "listing" | "xmp" => st.pre = true,
            "svg" | "math" => st.graphic = true,
            "head" | "style" | "script" | "title" | "template" | "rp" => {
                st.hidden = true;
            }
            "rt" | "rtc" => st.note = true,
            _ => {}
        }
        st.align = c.align.align();
        st.sided = c.align.sided();
        st.unmarked = c.unmarked;
        st.hidden |= c.display == Display::None;
        st
    }

    /// `tag` as selectors see it, placed among its parent's children.
    pub(super) fn element(&self, tag: &Tag, name: &str) -> El {
        let (count, types) = match self.stack.last() {
            Some(o) => (o.count, o.types.as_slice()),
            None => (self.top_count, &[][..]),
        };
        let typed = types.iter().find(|(t, _)| t == name).map_or(0, |t| t.1);
        let attrs: Vec<(String, String)> = (tag.attrs.iter())
            .take(MAX_ATTRS)
            .map(|a| {
                (
                    a.name.local.to_ascii_lowercase().to_string(),
                    a.value.to_string(),
                )
            })
            .collect();
        let value = |n: &str| attrs.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        El {
            tag: name.to_owned(),
            id: value("id"),
            classes: (value("class").unwrap_or_default())
                .split_ascii_whitespace()
                .take(MAX_CLASSES)
                .map(str::to_owned)
                .collect(),
            index: count + 1,
            type_index: typed + 1,
            attrs,
        }
    }

    /// The cascade for `el`, about to open inside the stack.
    pub(super) fn declared(&self, el: &El, tag: &Tag) -> Decl {
        let inherited = match self.stack.last() {
            Some(open) => Arc::clone(&open.computed.vars),
            None => Arc::from([]),
        };
        css::declared(
            &self.sheets,
            &self.path(el),
            &hints(tag, &el.tag),
            (attr(tag, "style"), self.base),
            &inherited,
        )
    }

    /// `:root`'s custom properties, writing mode and direction, read now
    /// the head's sheets are in.
    pub(super) fn root_vars(&mut self) {
        let Some(root) = self.root.take() else {
            return;
        };
        let path = [Ctx {
            el: &root,
            before: &[],
        }];
        let none: Vars = Arc::from([]);
        let d = css::declared(&self.sheets, &path, &Decl::default(), (None, ""), &none);
        let Some(open) = self.stack.first_mut().filter(|o| o.name == "html") else {
            return;
        };
        if let Some(vars) = d.vars {
            open.computed.vars = vars;
        }
        open.computed.writing = d.writing.unwrap_or(open.computed.writing);
        open.computed.dir = d.dir.unwrap_or(open.computed.dir);
    }

    /// The generated boxes rules give `el`, about to open; `d` is its own.
    pub(super) fn pseudos(&self, el: &El, d: &Decl) -> Pseudos {
        let path = self.path(el);
        let vars = d.vars.clone().unwrap_or_else(|| Arc::from([]));
        let get = |which| css::declared_pseudo(&self.sheets, &path, which, &vars);
        let block = is_block(&el.tag);
        Pseudos {
            before: get(PseudoEl::Before).filter(generates),
            after: get(PseudoEl::After).filter(generates),
            letter: block.then(|| get(PseudoEl::FirstLetter)).flatten(),
            line: block.then(|| get(PseudoEl::FirstLine)).flatten(),
            marker: (el.tag == "li").then(|| get(PseudoEl::Marker)).flatten(),
        }
    }

    /// The open elements as selectors see them, then `el`.
    pub(super) fn path<'s>(&'s self, el: &'s El) -> Vec<Ctx<'s>> {
        fn siblings(kids: &[El], open: bool) -> &[El] {
            match open {
                true => kids.get(..kids.len().saturating_sub(1)).unwrap_or_default(),
                false => kids,
            }
        }
        let mut path: Vec<Ctx<'_>> = Vec::with_capacity(self.stack.len() + 1);
        let mut kids = self.top.as_slice();
        for open in &self.stack {
            path.push(Ctx {
                el: &open.el,
                before: siblings(kids, true),
            });
            kids = &open.kids;
        }
        path.push(Ctx {
            el,
            before: siblings(kids, false),
        });
        path
    }

    /// Records `el` among its parent's children.
    pub(super) fn adopt(&mut self, el: &El) {
        let (kids, count) = match self.stack.last_mut() {
            Some(o) => {
                match o.types.iter_mut().find(|(t, _)| *t == el.tag) {
                    Some(t) => t.1 += 1,
                    None => o.types.push((el.tag.clone(), 1)),
                }
                (&mut o.kids, &mut o.count)
            }
            None => (&mut self.top, &mut self.top_count),
        };
        if kids.len() >= MAX_SIBLINGS {
            kids.remove(0);
        }
        kids.push(el.clone());
        *count += 1;
    }

    /// Notes what makes the chapter designed rather than prose.
    pub(super) fn observe(&mut self, name: &str, c: &Computed, d: &Decl) -> bool {
        let block = is_block(name) && !matches!(name, "html" | "body");
        let cell = matches!(
            name,
            "table" | "td" | "th" | "tr" | "thead" | "tbody" | "tfoot" | "caption" | "hr"
        );
        let width = match c.width {
            Len::Auto => false,
            Len::Percent(p) => p < 99.0,
            _ => true,
        };
        let strong = matches!(c.display, Display::Flex | Display::Grid)
            || (block && !cell && c.border.iter().any(|b| *b > 0.0))
            || (block && c.background)
            || (block && !cell && width)
            || matches!(d.height, Some(Len::Vh(_)));
        self.signals.strong |= strong;
        strong
    }

    /// Notes a text block's size, spacing and faces for the chapter.
    pub(super) fn observe_text(&mut self, look: &Look, role: Role) {
        self.signals.faces |= look.faces.iter().any(|f| f.starts_with(BOOK_FACE));
        self.signals.letters |= look.letter >= 0.1 * look.size;
        let size = (look.size * 20.0).round() as u32 * 5;
        if role == Role::Paragraph && !self.signals.sizes.contains(&size) {
            self.signals.sizes.push(size);
        }
    }

    /// A chapter keeps a few sheets, once each, imports first.
    pub(super) fn add_sheet(&mut self, sheet: Arc<Stylesheet>, depth: usize) {
        if depth < MAX_IMPORT_DEPTH {
            for path in &sheet.imports {
                if let Some(imported) = self.library.stylesheet(path) {
                    self.add_sheet(imported, depth + 1);
                }
            }
        }
        let known = self.sheets.iter().any(|s| Arc::ptr_eq(s, &sheet));
        if !known && self.sheets.len() < MAX_SHEETS {
            self.sheets.push(sheet);
            self.styled = true;
        }
    }

    /// Whether `name` starts content this reader shows otherwise or not.
    pub(super) fn holds_back(
        &mut self,
        name: &str,
        tag: &Tag,
        (c, shown): (&Computed, bool),
    ) -> bool {
        let depth = self.stack.len();
        let local = name.split_once(':').map(|(_, l)| l);
        let switch = self.switches.last_mut().filter(|s| s.0 + 1 == depth);
        match (local, switch) {
            (Some("switch"), _) => self.switches.push((depth, false)),
            (Some("case"), Some(sw)) => {
                let ns = attr(tag, "required-namespace").unwrap_or_default();
                let show = !sw.1 && media::drawn_namespace(ns);
                sw.1 |= show;
                return !show;
            }
            (Some("default"), Some(sw)) => return sw.1,
            _ => {}
        }
        if !shown {
            self.unseen_clip(name, tag);
            return false;
        }
        match name {
            "picture" => self.sources = Some((depth, None)),
            "video" | "audio" if self.media.is_none() => {
                let owned = |n: &str| attr(tag, n).map(|v| v.trim().to_owned());
                self.media = Some(media::Pending {
                    depth,
                    video: name == "video",
                    src: owned("src").filter(|s| !s.is_empty()),
                    poster: owned("poster"),
                    title: owned("title").or_else(|| owned("aria-label")),
                    width: picture_width(c).or_else(|| attr_width(tag)),
                    tracks: Vec::new(),
                    id: owned("id").filter(|s| !s.is_empty()),
                    unseen: false,
                    invisible: c.invisible,
                });
                return true;
            }
            "object" => return self.embedded((attr(tag, "data"), attr(tag, "type")), c),
            "iframe" => {
                if let Some(src) = attr(tag, "src").filter(|s| !s.trim().is_empty()) {
                    let target = self.link_to(src);
                    let name = media::label(attr(tag, "title"), src);
                    let text = format!("Open the embedded page: {name}");
                    self.caption(&text, target, c.invisible);
                }
                return true;
            }
            _ => {}
        }
        false
    }
}
