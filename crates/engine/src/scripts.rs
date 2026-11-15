//! A book's scripts: the pages they made, their fields, and the clicks sent.

use std::collections::{BTreeMap, HashMap};

use mareader_pdf::Bounds;

/// What the reader did on a scripted page.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Click(u32),
    Value(u32, String),
    /// A click on the first element a selector matches, from screenshots.
    Pick(String),
}

/// A text field on a scripted page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Field {
    pub value: String,
    pub hint: String,
    pub multiline: bool,
    pub secret: bool,
}

/// A field open for typing, on its mark.
#[derive(Debug, Clone, PartialEq)]
pub struct Editing {
    pub chapter: usize,
    pub node: u32,
    pub text: String,
    pub field: Field,
    pub page: usize,
    pub at: Bounds,
    pub focused: bool,
}

/// One pane's scripted pages as their scripts left them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Paged {
    /// Each scripted chapter's markup, by its number in the book.
    pub pages: BTreeMap<usize, String>,
    /// Counts changes to `pages`, to key the blocks laid for them.
    pub version: u64,
    pub fields: HashMap<(usize, u32), Field>,
    /// What the reader did, waiting for the scripts.
    pub asks: Vec<(usize, Ask)>,
    pub editing: Option<Editing>,
    /// The scripts' latest errors, for screenshots to report.
    pub errors: Vec<String>,
    /// Changes taken from the scripts so far.
    pub changes: usize,
}

impl Paged {
    /// A click on element `node` of `chapter`; a field opens to type in.
    pub fn click(&mut self, chapter: usize, node: u32, mark: Option<(usize, Bounds)>) {
        let field = self.fields.get(&(chapter, node));
        if let (Some(field), Some((page, at))) = (field, mark) {
            self.editing = Some(Editing {
                chapter,
                node,
                text: field.value.clone(),
                field: field.clone(),
                page,
                at,
                focused: false,
            });
            return;
        }
        self.asks.push((chapter, Ask::Click(node)));
    }

    /// Takes what chapter `chapter`'s scripts made; alerts for the reader.
    pub fn take(&mut self, chapter: usize, page: Option<String>, fields: Vec<(u32, Field)>) {
        self.changes += 1;
        if let Some(page) = page {
            self.pages.insert(chapter, page);
            self.version += 1;
        }
        self.fields.retain(|(c, _), _| *c != chapter);
        self.fields
            .extend(fields.into_iter().map(|(n, f)| ((chapter, n), f)));
    }

    /// Writes back what was typed, and asks the scripts for its value.
    pub fn commit(&mut self) {
        if let Some(e) = self.editing.take() {
            if let Some(f) = self.fields.get_mut(&(e.chapter, e.node)) {
                f.value.clone_from(&e.text);
            }
            self.asks.push((e.chapter, Ask::Value(e.node, e.text)));
        }
    }
}
