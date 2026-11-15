use super::*;

impl<'a> Setter<'a> {
    pub(super) fn text(
        &mut self,
        text: &Text,
        frame: &Frame,
        width: f32,
        beside: Option<Beside>,
    ) -> (Draw, Laid, Vec<(f32, f32)>, Vec<usize>) {
        let em = self.typo.font_size;
        let style = self.text_style(text.role, frame);
        let shaded = text.role == Role::Preformatted;
        let (pad, pad_y) = if shaded {
            (PRE_PAD.0 * em, PRE_PAD.1 * em)
        } else {
            (0.0, 0.0)
        };
        let inner = (width - 2.0 * pad).max(em);
        // The book's families first, the reader's after.
        let laid = self.scaled(frame);
        // Overridden, the book's families give way to the reader's.
        let named = self.look(frame).filter(|_| !self.typo.override_font);
        let faces = named
            .filter(|l| !l.faces.is_empty())
            .map(|l| self.look_faces(l, laid, style.faces));
        let style = match &faces {
            Some(faces) => Style {
                faces: faces.as_slice(),
                ..style
            },
            None => style,
        };
        let runs: Vec<Stack> = match named {
            Some(_) => (text.faces.iter())
                .map(|names| {
                    let look = Look {
                        faces: names.clone(),
                        ..Look::default()
                    };
                    self.look_faces(&look, laid, style.faces)
                })
                .collect(),
            None => Vec::new(),
        };
        let style = Style {
            runs: &runs,
            ..style
        };
        let (initial, beside) = self.initial_beside(text, style, beside);
        let mut para = layout_text(self.fonts, text, style, (Some(inner), beside));
        if let Some(extra) = text.first_line.filter(|_| style.sized) {
            para = self.first_line(text, (extra, para), style, (inner, beside));
        }
        self.notes(text, &mut para, style);
        let mut units = line_units(&para.layout);
        // The padding rides on the first and last lines.
        if let Some(first) = units.first_mut() {
            first.0 -= pad_y;
        }
        if let Some(last) = units.last_mut() {
            last.1 += pad_y;
        }
        let marker = self.marker_layout(frame, style);
        let starts = std::mem::take(&mut para.starts);
        let para = Box::new(para);
        let initial = initial.map(|(i, _)| Box::new(i));
        let laid = Laid::Text {
            para,
            marker,
            initial,
        };
        (Draw::Text { pad, shaded }, laid, units, starts)
    }

    fn marker_layout(&mut self, frame: &Frame, style: Style<'_>) -> Option<Box<Para>> {
        frame.marker.as_deref().map(|m| {
            let mut t = Text::new(Role::Paragraph);
            let colour = frame.marker_colour;
            t.push(
                m,
                Emphasis {
                    colour,
                    ..Emphasis::default()
                },
            );
            let style = Style {
                align: Align::Start,
                pre: false,
                justify: false,
                hyphens: None,
                indent: 0.0,
                ..style
            };
            if let Some(picture) = &frame.marker_picture {
                t.inlines.push(marker_inline(picture, t.text.len()));
                t.push(WORD_JOINER, Emphasis::default());
            }
            Box::new(layout_text(self.fonts, &t, style, (None, None)))
        })
    }

    fn initial_beside(
        &mut self,
        text: &Text,
        style: Style<'_>,
        beside: Option<Beside>,
    ) -> (Option<(Initial, Beside)>, Option<Beside>) {
        let initial = self.initial(text, style);
        let beside = match &initial {
            Some((_, room)) => Some(match beside {
                Some(b) => Beside {
                    left: b.left + room.left,
                    height: b.height.max(room.height),
                    ..b
                },
                None => *room,
            }),
            None => beside,
        };
        (initial, beside)
    }

    /// Lays `para`'s ruby notes small, each in its base's emphasis.
    pub(super) fn notes(&mut self, text: &Text, para: &mut Para, style: Style<'_>) {
        let small = Style {
            size: (style.size * RUBY_SCALE).clamp(MIN_SIZE, MAX_SIZE),
            line_height: 1.0,
            align: Align::Start,
            pre: false,
            justify: false,
            hyphens: None,
            indent: 0.0,
            letter: 0.0,
            word: 0.0,
            last: None,
            dir: Dir::Auto,
            ..style
        };
        for (note, own) in para.ruby.iter_mut().zip(&text.ruby) {
            let at = own.range.start;
            let base = (text.spans.iter())
                .find(|s| s.range.start <= at && at < s.range.end)
                .map(|s| s.emphasis)
                .unwrap_or_default();
            let mut t = Text::new(Role::Paragraph);
            let plain = Emphasis {
                size: 0,
                script: Script::Normal,
                underline: false,
                strike: false,
                case: Case::Normal,
                ..base
            };
            t.push(&note.text, plain);
            t.faces.clone_from(&text.faces);
            note.para = Some(Box::new(layout_text(self.fonts, &t, small, (None, None))));
        }
    }

    /// The drop cap `text` starts with, and the room its lines leave it.
    pub(super) fn initial(&mut self, text: &Text, style: Style<'_>) -> Option<(Initial, Beside)> {
        let letter = text
            .text
            .get(..text.initial)
            .filter(|l| style.sized && !l.is_empty())?;
        let own = (text.spans.first()).filter(|s| s.range.start == 0);
        let emphasis = own.map(|s| s.emphasis).unwrap_or_default();
        let size = style.size * f32::from(emphasis.size.max(100)) / 100.0;
        let mut t = Text::new(Role::Paragraph);
        let plain = Emphasis {
            size: 0,
            script: Script::Normal,
            underline: false,
            ..emphasis
        };
        t.push(letter, plain);
        let big = Style {
            size: size.clamp(MIN_SIZE, MAX_SIZE),
            line_height: 1.0,
            align: Align::Start,
            pre: false,
            justify: false,
            hyphens: None,
            indent: 0.0,
            ..style
        };
        let para = layout_text(self.fonts, &t, big, (None, None));
        let line = style.size * style.line_height;
        // It spans the lines from the first's capitals to its own baseline.
        let drop = ((size - style.size) * CAP_HEIGHT / line.max(1.0)).round();
        let lines = (drop as usize + 1).clamp(1, MAX_DROP);
        let room = Beside {
            left: para.layout.width() + DROP_GAP * style.size,
            right: 0.0,
            height: (lines - 1) as f32 * line + 1.0,
        };
        Some((Initial { para, line: lines }, room))
    }

    /// Lays `text` again with `extra` over what its first line holds.
    pub(super) fn first_line(
        &mut self,
        text: &Text,
        (extra, mut para): (Emphasis, Para),
        style: Style<'_>,
        (inner, beside): (f32, Option<Beside>),
    ) -> Para {
        let mut end = para.starts.get(1).copied().unwrap_or(text.text.len());
        // A styled line may hold less; a few passes settle it.
        for _ in 0..FIRST_LINE_PASSES {
            let styled = over_first_line(text, end, extra);
            para = layout_text(self.fonts, &styled, style, (Some(inner), beside));
            let held = para.starts.get(1).copied().unwrap_or(text.text.len());
            if held >= end {
                break;
            }
            end = held;
        }
        para
    }

    pub(super) fn table(
        &mut self,
        table: &Table,
        frame: &Frame,
        width: f32,
    ) -> (Grid, Vec<Vec<Para>>, Units) {
        let em = self.typo.font_size;
        let columns = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(1);
        let column = width / columns.max(1) as f32;
        let (pad_x, pad_y) = (CELL_PAD.0 * em, CELL_PAD.1 * em);
        let inner = (column - 2.0 * pad_x).max(em);
        let mut units = Vec::with_capacity(table.rows.len());
        let mut rows = Vec::with_capacity(table.rows.len());
        let mut heads = ThinVec::with_capacity(table.rows.len());
        let mut y = 0.0;
        for row in &table.rows {
            let style = self.cell_style(frame, row.head);
            let cells: Vec<Para> = row
                .cells
                .iter()
                .map(|c| layout_text(self.fonts, c, style, (Some(inner), None)))
                .collect();
            let tallest = cells.iter().map(|c| c.layout.height()).fold(0.0, f32::max);
            let h = tallest + 2.0 * pad_y;
            units.push((y, y + h));
            y += h;
            rows.push(cells);
            heads.push(row.head);
        }
        let pad = [pad_x, pad_y];
        let rule = table.rule;
        (
            Grid {
                column,
                pad,
                heads,
                rule,
            },
            rows,
            units,
        )
    }
}
