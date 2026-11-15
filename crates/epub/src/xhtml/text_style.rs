use super::*;

impl State<'_> {
    /// The computed style of the innermost open block element.
    pub(super) fn block_computed(&self) -> Option<&Computed> {
        let open = self.stack.iter().rev().find(|o| is_block(&o.name));
        open.or(self.stack.last()).map(|o| &o.computed)
    }

    pub(super) fn block_style(&self) -> Computed {
        self.block_computed().cloned().unwrap_or_default()
    }

    /// The look of a block starting now; the chapter decides if it shows.
    pub(super) fn look(&mut self) -> Arc<Look> {
        let c = self.block_style();
        self.block_line = c.line_px();
        let faces = self.faces(c.family.as_deref());
        let look = Look {
            designed: false,
            size: c.size / ROOT_PX,
            line_height: match c.line {
                Line::Normal => None,
                Line::Factor(f) => Some(f),
                Line::Px(px) => Some(px / c.size),
            },
            letter: c.letter / ROOT_PX,
            word: c.word / ROOT_PX,
            indent: c.indent.px(c.size, css::BASE_WIDTH).unwrap_or(0.0) / ROOT_PX,
            weight: f32::from(c.weight),
            faces,
            justify: c.align == TextAlign::Justify,
            last: c.last,
        };
        if let Some(known) = self.looks.iter().rev().take(8).find(|l| ***l == look) {
            return Arc::clone(known);
        }
        let look = Arc::new(look);
        self.looks.push(Arc::clone(&look));
        look
    }

    /// `family` as faces: the book's own by their registered names.
    pub(super) fn faces(&mut self, family: Option<&[String]>) -> Vec<String> {
        let mut out = Vec::new();
        for name in family.unwrap_or_default() {
            let lower = name.to_lowercase();
            if GENERIC.contains(&lower.as_str()) {
                // Sans and mono stay apart from the reader's body face.
                if matches!(lower.as_str(), "sans-serif" | "monospace") {
                    out.push(lower);
                }
                break;
            }
            let shipped: Vec<FontFace> = (self.sheets.iter())
                .flat_map(|s| &s.faces)
                .filter(|f| f.family.to_lowercase() == lower)
                .cloned()
                .collect();
            if shipped.is_empty() {
                out.push(name.clone());
                continue;
            }
            for face in shipped {
                if !self.refs.faces.contains(&face) {
                    self.refs.faces.push(face);
                }
            }
            out.push(format!("{BOOK_FACE}{lower}"));
        }
        out
    }

    /// How text written now differs from its block: weight, slant, size.
    pub(super) fn emphasis(&self) -> Emphasis {
        let style = self.top();
        let c = self.stack.last().map(|o| &o.computed);
        let (weight, size) = c.map_or((400, ROOT_PX), |c| (c.weight, c.size));
        let block = self.block_computed();
        let (block_weight, block_size) = block.map_or((400, ROOT_PX), |c| (c.weight, c.size));
        let ratio = size / block_size.max(1.0);
        let sized = (ratio - 1.0).abs() > 0.02;
        // Raised by the nearest inline that says so; decorations add up.
        let inline = self.stack.iter().rev().take_while(|o| !is_block(&o.name));
        let script = inline
            .map(|o| o.computed.script)
            .find(|s| *s != Script::Normal);
        let (underline, strike) = self.stack.iter().fold((false, false), |(u, s), o| {
            (u || o.computed.decoration.0, s || o.computed.decoration.1)
        });
        Emphasis {
            italic: c.is_some_and(|c| c.italic),
            bold: weight >= 600 && weight > block_weight,
            light: weight < 600 && block_weight >= 600,
            code: style.code,
            size: if sized {
                (ratio * 100.0).round().clamp(1.0, 1000.0) as u16
            } else {
                0
            },
            script: script.unwrap_or_default(),
            case: c.map_or(Case::Normal, Computed::case),
            underline,
            strike,
            colour: self.text_colour(c),
            shadow: c.and_then(|c| self.text_shadow(c)),
            hidden: c.is_some_and(|c| c.invisible),
            features: c.map_or_else(Features::default, Computed::features),
            face: 0,
        }
    }

    /// The text's shadow, unless it barely differs from the page.
    pub(super) fn text_shadow(&self, c: &Computed) -> Option<TextShadow> {
        let s = c.text_shadow.as_ref()?;
        let strength = self.strength(s.tint, c);
        let share = |l: Len| l.px(c.size, css::BASE_WIDTH).unwrap_or(0.0) / c.size.max(1.0) * 100.0;
        (strength >= MIN_FILL).then(|| TextShadow {
            at: s.at.map(|l| share(l).round().clamp(-127.0, 127.0) as i8),
            blur: share(s.blur).round().clamp(0.0, 255.0) as u8,
            strength: (strength * 255.0).round() as u8,
        })
    }

    /// Text's colour as drawn, faded toward its ground by opacity.
    pub(super) fn text_colour(&self, c: Option<&Computed>) -> Option<css::Rgb> {
        let c = c?;
        if c.opacity >= 0.99 {
            return c.ink.and_then(|ink| self.drawn(ink, MAX_FADE));
        }
        let ink = c.ink.unwrap_or(self.book_ink);
        let ground = self.ground();
        let a = c.opacity.clamp(0.0, 1.0);
        let mixed: css::Rgb = std::array::from_fn(|i| {
            let (f, g) = (ink.get(i).copied(), ground.get(i).copied());
            let (f, g) = (f32::from(f.unwrap_or(0)), f32::from(g.unwrap_or(255)));
            (f * a + g * (1.0 - a)).round().clamp(0.0, 255.0) as u8
        });
        self.drawn(mixed, MAX_FADE)
    }

    /// The background text sits on: its nearest block's, or the page.
    pub(super) fn ground(&self) -> css::Rgb {
        let ground = self
            .stack
            .iter()
            .rev()
            .find_map(|o| match is_block(&o.name) {
                true => o.computed.fill,
                false => None,
            });
        ground.unwrap_or(self.page)
    }

    /// `ink` as the page draws it, given the background it sits on.
    pub(super) fn drawn(&self, ink: css::Rgb, most: f32) -> Option<css::Rgb> {
        drawn(ink, (self.ground(), self.book_ink, self.page), most)
    }

    /// The colour a data table's grid takes: its first drawn border's.
    pub(super) fn rule(&mut self, c: &Computed) {
        let seen = c.border.iter().zip(&c.border_style).zip(&c.border_colour);
        let ink = seen
            .filter(|((w, s), _)| **w > 0.0 && **s != BorderStyle::None)
            .find_map(|(_, ink)| *ink);
        let drawn = ink.and_then(|ink| self.drawn(ink, MAX_BORDER_FADE));
        if let Some(t) = self.table.as_mut().filter(|t| t.nested == 0) {
            t.table.rule = t.table.rule.or(drawn);
        }
    }
}
