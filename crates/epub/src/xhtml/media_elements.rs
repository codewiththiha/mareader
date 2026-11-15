use super::*;

impl State<'_> {
    /// A hidden clip a trigger may play or show: a card that never shows.
    pub(super) fn unseen_clip(&mut self, name: &str, tag: &Tag) {
        let id = attr(tag, "id").map(str::trim).filter(|id| !id.is_empty());
        let wanted = !self.triggers.observers.is_empty();
        if !wanted || !matches!(name, "video" | "audio") || self.media.is_some() {
            return;
        }
        self.media = Some(media::Pending {
            depth: self.stack.len(),
            video: name == "video",
            src: attr(tag, "src").map(|s| s.trim().to_owned()),
            id: id.map(str::to_owned),
            unseen: true,
            ..Default::default()
        });
    }

    /// Shows or hides an element as the reader's triggers left it.
    pub(super) fn trigger_target(&self, tag: &Tag, computed: &mut Computed) {
        let id = attr(tag, "id").map(str::trim).unwrap_or_default();
        let Some(&visible) = self.overrides.get(id) else {
            return;
        };
        let name: &str = &tag.name;
        if !visible {
            computed.invisible = true;
            return;
        }
        if computed.display == Display::None {
            computed.display = match name {
                "table" => Display::Table,
                _ if is_block(name) => Display::Block,
                _ => Display::Inline,
            };
        }
        computed.invisible = false;
    }

    /// Notes an element triggers show or hide.
    pub(super) fn add_target(&mut self, tag: &Tag) {
        let id = attr(tag, "id").map(str::trim).unwrap_or_default();
        if !self.triggers.targets.contains(id) || self.refs.targets.len() >= MAX_PLACES {
            return;
        }
        let key = format!("{}#{id}", self.base);
        if self.refs.target_keys.contains_key(&key) {
            return;
        }
        let chapter = self.refs.chapters.len();
        self.refs
            .target_keys
            .insert(key.clone(), self.refs.targets.len());
        self.refs.targets.push(trigger::Target { key, chapter });
    }

    /// A link to what clicking `tag` sets off, if triggers observe it.
    pub(super) fn observer_link(&mut self, tag: &Tag) -> Option<u32> {
        // A scripted page marks what its scripts answer clicks on.
        if let Some(chapter) = self.script_at
            && let Some(node) = attr(tag, script::NODE).and_then(|n| n.trim().parse().ok())
        {
            return script::link(chapter, node);
        }
        let id = attr(tag, "id").map(str::trim)?;
        let actions = self.triggers.observers.get(id)?;
        let base = self.base;
        let keyed = actions.iter().map(|(a, t)| (*a, format!("{base}#{t}")));
        let keyed: Vec<_> = keyed.collect();
        if self.refs.hrefs.len() >= MAX_LINKS {
            return None;
        }
        let n = u32::try_from(self.refs.triggers.len()).ok()?;
        self.refs.triggers.push(keyed);
        self.add_href(Href::Trigger(n))
    }

    /// Ends the cards, switches and source pickers closed just now.
    pub(super) fn closed_embeds(&mut self) {
        let depth = self.stack.len();
        if self.media.as_ref().is_some_and(|m| depth <= m.depth)
            && let Some(m) = self.media.take()
        {
            self.media_card(m);
        }
        self.switches.retain(|s| s.0 < depth);
        if self.sources.as_ref().is_some_and(|s| depth <= s.0) {
            self.sources = None;
        }
    }

    /// A `<source>`: the clip a card plays, or a picture's pick.
    pub(super) fn source(&mut self, tag: &Tag) {
        if let Some(m) = self.media.as_ref() {
            let src = attr(tag, "src").map(str::trim).filter(|s| !s.is_empty());
            let Some(src) = src else { return };
            // The first that plays, else the first named, for its caption.
            let better = match m.src.clone() {
                Some(old) => self.clip(&old) != Clip::Plays && self.clip(src) == Clip::Plays,
                None => true,
            };
            if better && let Some(m) = self.media.as_mut() {
                m.src = Some(src.to_owned());
            }
            return;
        }
        if let Some((_, picked)) = self.sources.as_mut()
            && picked.is_none()
        {
            let fits = attr(tag, "media").is_none_or(query::media);
            let typed = attr(tag, "type").is_none_or(media::decodes);
            if fits && typed {
                *picked = attr(tag, "srcset").and_then(media::best_candidate);
            }
        }
    }

    /// A card's `<track>` of subtitles or captions; other kinds are skipped.
    pub(super) fn track(&mut self, tag: &Tag) {
        let Some(m) = self.media.as_mut() else {
            return;
        };
        let kind = attr(tag, "kind").map(|k| k.trim().to_ascii_lowercase());
        let shown = kind
            .as_deref()
            .is_none_or(|k| matches!(k, "subtitles" | "captions" | ""));
        let src = attr(tag, "src").map(str::trim).filter(|s| !s.is_empty());
        let (Some(src), true) = (src, shown) else {
            return;
        };
        if m.tracks.len() >= subtitles::MAX_TRACKS {
            return;
        }
        let owned = |n: &str| {
            attr(tag, n)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        m.tracks.push(media::TrackTag {
            src: src.to_owned(),
            language: owned("srclang"),
            label: owned("label"),
            default: attr(tag, "default").is_some(),
        });
    }

    /// A card's tracks in the book, and a link to its transcript if room.
    pub(super) fn subtitled(&mut self, m: &media::Pending, clip: Option<String>) {
        let tracks: Vec<subtitles::Track> = (m.tracks.iter())
            .filter_map(|t| {
                let path = href::resolve(self.base, &t.src)?;
                self.library.media_type(&path)?;
                Some(subtitles::Track {
                    path,
                    language: t.language.clone(),
                    label: t.label.clone(),
                    default: t.default,
                })
            })
            .collect();
        let full = self.refs.cards.len() >= subtitles::MAX_CARDS;
        if tracks.is_empty() || full || m.invisible {
            return;
        }
        let Ok(card) = u32::try_from(self.refs.cards.len()) else {
            return;
        };
        // A table's cell or a fixed page has no room for the lines.
        let room = self.table.is_none() && !self.fixed;
        let toggle = room
            .then(|| self.add_href(Href::Transcript { card, track: None }))
            .flatten();
        let mut picks = Vec::new();
        for track in (0..tracks.len()).filter(|_| toggle.is_some()) {
            let track = u32::try_from(track).ok();
            picks.extend(self.add_href(Href::Transcript { card, track }));
        }
        let mut caption = None;
        if let Some(toggle) = toggle.filter(|_| picks.len() == tracks.len()) {
            let before = self.blocks.len();
            self.caption("Show transcript", Some(toggle), false);
            caption = (self.blocks.len() > before).then(|| self.first + self.blocks.len() - 1);
        }
        self.refs.cards.push(subtitles::Card {
            clip,
            tracks,
            caption,
            toggle: toggle.unwrap_or_default(),
            picks,
        });
    }

    /// An `<object>` or `<embed>` drawn as its picture or card; false if not.
    pub(super) fn embedded(
        &mut self,
        (src, kind): (Option<&str>, Option<&str>),
        c: &Computed,
    ) -> bool {
        let Some(src) = src.map(str::trim).filter(|s| !s.is_empty()) else {
            return false;
        };
        let path = href::resolve(self.base, src);
        let declared = path.as_deref().and_then(|p| self.library.media_type(p));
        let kind = kind.map(str::to_owned).or(declared).unwrap_or_default();
        if media::plays(&kind) {
            let video = kind.trim().to_ascii_lowercase().starts_with("video/");
            let src = Some(src.to_owned());
            let width = picture_width(c);
            let depth = self.stack.len();
            let pending = media::Pending {
                depth,
                video,
                src,
                width,
                ..Default::default()
            };
            self.media_card(pending);
            return true;
        }
        let image = kind.is_empty() || kind.trim().to_ascii_lowercase().starts_with("image/");
        let Some(picture) = self.load(src).filter(|_| image) else {
            return false;
        };
        self.picture(Some(picture), None, (picture_width(c), Some(c)));
        true
    }

    /// `<video>` or `<audio>` as a card: its poster or a drawn player.
    pub(super) fn media_card(&mut self, m: media::Pending) {
        if let Some(id) = &m.id {
            let plays = m.src.as_deref().filter(|s| self.clip(s) == Clip::Plays);
            let path = plays.and_then(|s| href::resolve(self.base, s.trim()));
            let key = format!("{}#{}", self.base, id.trim());
            if self.refs.clips.len() < MAX_PLACES {
                self.refs.clips.insert(key, (path, m.video));
            }
        }
        if self.top().hidden || m.unseen {
            // A trigger may show it, so its link is known beforehand.
            let src = m.src.filter(|s| self.clip(s) == Clip::Plays);
            if let Some(src) = src.filter(|_| !self.triggers.observers.is_empty()) {
                self.media_link(&src, m.video);
            }
            return;
        }
        let clip = m.src.as_deref().map_or(Clip::Gone, |s| self.clip(s));
        let target = (m.src.as_deref())
            .filter(|_| clip == Clip::Plays)
            .and_then(|s| self.media_link(s, m.video));
        let name = media::label(m.title.as_deref(), m.src.as_deref().unwrap_or_default());
        let what = if m.video { "video" } else { "audio" };
        let named = |s: String| match name.is_empty() {
            true => s,
            false => format!("{s}: {name}"),
        };
        let text = match clip {
            _ if target.is_some() => named(format!("Play the {what}")),
            Clip::Held => named(format!("The {what} can't play here")),
            _ => format!("The {what} isn't in this book"),
        };
        let clip = (m.src.as_deref())
            .filter(|_| target.is_some())
            .and_then(|s| href::resolve(self.base, s.trim()));
        let target = target.filter(|_| !m.invisible);
        if self.table.is_some() {
            self.caption(&text, target, m.invisible);
            return self.subtitled(&m, clip);
        }
        let poster = m.poster.as_deref().filter(|_| m.video);
        let art = poster.and_then(|p| self.poster(p)).or_else(|| {
            let card = if m.video {
                media::video_card()
            } else {
                media::audio_card()
            };
            Picture::svg(card, Vec::new()).ok().map(Arc::new)
        });
        if let Some(picture) = art {
            self.finish();
            let full = if m.video {
                Length::Percent(100.0)
            } else {
                Length::Em(20.0)
            };
            let mut block = Block::new(Content::Image {
                picture,
                cover: false,
                width: m.width.or(Some(full)),
                links: target.map(Area::whole).into_iter().collect(),
            });
            block.frame = self.frame();
            block.frame.align = Align::Center;
            block.frame.hidden = m.invisible;
            self.emit(block);
        }
        self.caption(&text, target, m.invisible);
        self.subtitled(&m, clip);
    }

    /// Poster `src` with a play mark over it; a vector one as it is.
    pub(super) fn poster(&mut self, src: &str) -> Option<Arc<Picture>> {
        let picture = self.load(src)?;
        if picture.vector() {
            return Some(picture);
        }
        let data = Arc::new(picture.bytes().to_vec());
        let card = media::poster_card(picture.width, picture.height);
        Picture::svg(card, vec![(media::POSTER.to_owned(), data)])
            .ok()
            .map(Arc::new)
    }

    /// A centred line of small text, a link to `target` if there is one.
    pub(super) fn caption(&mut self, text: &str, target: Option<u32>, hidden: bool) {
        let small = Emphasis {
            size: 90,
            hidden,
            ..Emphasis::default()
        };
        if self.table.is_some() {
            if let Some(cell) = self.target() {
                put(cell, text, small, target);
            }
            return;
        }
        self.finish();
        if let Some(t) = self.target() {
            put(t, text, small, target);
        }
        if let Some(b) = self.current.as_mut() {
            b.frame.align = Align::Center;
        }
        self.finish();
    }

    /// The links of `tag`'s image map over `picture`, as shares of it.
    pub(super) fn map_areas(&mut self, tag: &Tag, picture: &Picture) -> Vec<Area> {
        let name = attr(tag, "usemap").map(|m| m.trim().trim_start_matches('#'));
        let Some(areas) = name.and_then(|n| self.maps.get(n)).cloned() else {
            return Vec::new();
        };
        let size = |n: &str, natural: u32| {
            let given = attr(tag, n).and_then(|v| v.trim().trim_end_matches("px").parse().ok());
            given
                .filter(|v: &f32| *v > 0.0)
                .unwrap_or(natural.max(1) as f32)
        };
        let (w, h) = (size("width", picture.width), size("height", picture.height));
        let mut out = Vec::new();
        for area in areas {
            let Some(target) = self.link_to(&area.href) else {
                continue;
            };
            let rect = match area.bounds {
                Some([x0, y0, x1, y1]) => {
                    let clamp = |v: f32, side: f32| (v / side).clamp(0.0, 1.0);
                    let (l, t) = (clamp(x0, w), clamp(y0, h));
                    [l, t, clamp(x1, w) - l, clamp(y1, h) - t]
                }
                None => [0.0, 0.0, 1.0, 1.0],
            };
            if rect[2] > 0.0 && rect[3] > 0.0 {
                out.push(Area { rect, target });
            }
        }
        out
    }
}
