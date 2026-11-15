use super::*;

impl State<'_> {
    pub(super) fn role(&self) -> Role {
        let level = self.stack.iter().rev().find_map(|o| heading(&o.name));
        match level {
            Some(level) => Role::Heading(level),
            None if self.top().pre => Role::Preformatted,
            None => Role::Paragraph,
        }
    }

    pub(super) fn emit(&mut self, mut block: Block) {
        block.break_before = std::mem::take(&mut self.chapter);
        if let Content::Text(t) = &block.content {
            let boxed = self.laying > 0 || self.stack.iter().any(|o| o.signals);
            let n = t.text.chars().count();
            self.signals.text += n;
            self.signals.boxed += if boxed { n } else { 0 };
        }
        let at = Anchor {
            block: self.first + self.blocks.len(),
            offset: 0,
        };
        for key in self.pending.drain(..) {
            self.refs.places.entry(key).or_insert(at);
        }
        // `break-before: avoid` holds the block before to this one.
        let last = self.blocks.last_mut().filter(|_| !block.break_before);
        if let Some(prev) = last.filter(|_| block.frame.keep.with_last) {
            prev.frame.keep.with_next = true;
        }
        self.blocks.push(block);
    }

    /// Notes where `id` is: mid-paragraph, or the next block.
    pub(super) fn mark(&mut self, id: &str) {
        let id = id.trim();
        if id.is_empty() || self.refs.places.len() + self.pending.len() >= MAX_PLACES {
            return;
        }
        let key = format!("{}#{id}", self.base);
        let written = match self.current.as_ref().map(|b| &b.content) {
            Some(Content::Text(t)) if self.table.is_none() => t.text.len(),
            _ => 0,
        };
        if written == 0 {
            return self.pending.push(key);
        }
        let at = Anchor {
            block: self.first + self.blocks.len(),
            offset: written,
        };
        self.refs.places.entry(key).or_insert(at);
    }

    /// Past what's written: mid-paragraph, else the next block.
    pub(super) fn end_anchor(&self) -> Anchor {
        let block = self.first + self.blocks.len();
        if self.table.is_some() {
            // A table is one block; what ends in it ends with it.
            return Anchor {
                block: block + 1,
                offset: 0,
            };
        }
        let offset = match self.current.as_ref().map(|b| &b.content) {
            Some(Content::Text(t)) => t.text.len(),
            _ => 0,
        };
        Anchor { block, offset }
    }

    /// `href`'s index in [`Refs::hrefs`], resolved from this chapter.
    pub(super) fn link_to(&mut self, href: &str) -> Option<u32> {
        let href = href.trim();
        let fragment = href.split_once('#').map(|(_, f)| href::percent_decode(f));
        let fragment = fragment.filter(|f| !f.is_empty());
        let target = if let (true, Some(f)) = (href.starts_with('#'), &fragment) {
            Href::Inside(format!("{}#{f}", self.base))
        } else if let Some(path) = href::resolve(self.base, href) {
            match fragment {
                Some(f) => Href::Inside(format!("{path}#{f}")),
                None => Href::Inside(path),
            }
        } else if href.contains(':') {
            Href::Outside(href.to_owned())
        } else {
            return None;
        };
        self.add_href(target)
    }

    /// `target`'s index in [`Refs::hrefs`], once added.
    pub(super) fn add_href(&mut self, target: Href) -> Option<u32> {
        if self.refs.hrefs.len() >= MAX_LINKS {
            return None;
        }
        let index = u32::try_from(self.refs.hrefs.len()).ok()?;
        self.refs.hrefs.push(target);
        Some(index)
    }

    /// Whether the clip at `src` plays, by the type the book gives it.
    pub(super) fn clip(&mut self, src: &str) -> Clip {
        let src = src.trim();
        if src.starts_with("https://") || src.starts_with("http://") {
            return Clip::Plays;
        }
        let Some(path) = href::resolve(self.base, src) else {
            return Clip::Gone;
        };
        match self.library.media_type(&path) {
            Some(t) if media::extension(&t).is_some() => Clip::Plays,
            Some(_) => Clip::Held,
            None => Clip::Gone,
        }
    }

    /// A link that plays `src`: in the book, or at a web address.
    pub(super) fn media_link(&mut self, src: &str, video: bool) -> Option<u32> {
        let src = src.trim();
        let target = match href::resolve(self.base, src) {
            Some(path) => Href::Media { path, video },
            None if src.starts_with("https://") || src.starts_with("http://") => {
                Href::Outside(src.to_owned())
            }
            None => return None,
        };
        self.add_href(target)
    }

    pub(super) fn finish(&mut self) {
        if let Some(mut block) = self.current.take() {
            let written = matches!(&block.content, Content::Text(t) if !t.text.is_empty() || !t.inlines.is_empty());
            if let Content::Text(text) = &mut block.content {
                text.trim_end();
            }
            if !block.is_blank() {
                self.emit(block);
            } else if written {
                // A line of only spaces still takes its height.
                self.gap
                    .fixed(block.frame.before * ROOT_PX + self.block_line);
            }
        }
        self.space = false;
    }

    /// The text that characters go to now, if any.
    pub(super) fn target(&mut self) -> Option<&mut Text> {
        if self.table.is_some() {
            return self.table.as_mut().and_then(TableBuild::cell);
        }
        if self.current.is_none() {
            let mut frame = self.frame();
            frame.marker = self.marker.take().map(String::into_boxed_str);
            frame.marker_colour = self.marker_colour.take();
            frame.marker_picture = self.marker_picture.take();
            let role = self.role();
            if let Some(look) = frame.look.clone() {
                self.observe_text(&look, role);
            }
            let mut block = Block::new(Content::Text(Text::new(role)));
            block.frame = frame;
            self.current = Some(block);
        }
        match self.current.as_mut().map(|b| &mut b.content) {
            Some(Content::Text(text)) => Some(text),
            _ => None,
        }
    }
}
