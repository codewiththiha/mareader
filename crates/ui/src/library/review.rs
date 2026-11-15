//! The AI's plan for loose books, to apply or let go.

use std::collections::BTreeMap;

use egui::{Align, Layout, RichText, ScrollArea, Ui, vec2};
use mareader_library::{BookId, Library};

use super::Action;
use super::counted::counted;
use super::organizing::{Proposal, Spot};
use crate::icons::Icon;
use crate::modal::{buttons, primary, title};
use crate::theme::{self, Tokens};

/// Titles a row names before "and N more".
const NAMED: usize = 3;
const LIST_H: f32 = 300.0;

/// A shelf in the plan's tree, with the books it gets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub depth: usize,
    /// A standing shelf shows its whole path; a new one, its name.
    pub name: String,
    pub new: bool,
    pub titles: Vec<String>,
}

/// The plan as a tree: standing shelves first, each with its new ones.
pub fn rows(model: &Library, plan: &Proposal) -> Vec<Row> {
    let mut titles: BTreeMap<Spot, Vec<String>> = BTreeMap::new();
    for (book, spot) in &plan.placed {
        if let Some(t) = title_of(model, *book) {
            titles.entry(*spot).or_default().push(t);
        }
    }
    let parent = |i: usize| plan.shelves.get(i).and_then(|(_, p)| *p);
    let mut roots: Vec<(String, Spot)> = Vec::new();
    for spot in titles
        .keys()
        .copied()
        .chain(plan.shelves.iter().filter_map(|(_, p)| *p))
    {
        if let Spot::Shelf(id) = spot
            && !roots.iter().any(|(_, s)| *s == spot)
        {
            let path = model.path_to(id);
            let names: Vec<&str> = path.iter().map(|s| s.name.value.as_str()).collect();
            roots.push((names.join(" › "), spot));
        }
    }
    roots.sort_by_key(|(name, _)| name.to_lowercase());
    let fresh = (0..plan.shelves.len()).filter(|&i| parent(i).is_none());
    let mut fresh: Vec<(String, Spot)> = fresh
        .filter_map(|i| plan.shelves.get(i).map(|(n, _)| (n.clone(), Spot::New(i))))
        .collect();
    fresh.sort_by_key(|(name, _)| name.to_lowercase());
    let mut out = Vec::new();
    let mut stack: Vec<(usize, String, Spot)> = (roots.into_iter().chain(fresh))
        .rev()
        .map(|(name, spot)| (0, name, spot))
        .collect();
    while let Some((depth, name, spot)) = stack.pop() {
        out.push(Row {
            depth,
            name,
            new: matches!(spot, Spot::New(_)),
            titles: titles.remove(&spot).unwrap_or_default(),
        });
        let children = (0..plan.shelves.len())
            .rev()
            .filter(|&i| parent(i) == Some(spot));
        for i in children {
            if let Some((name, _)) = plan.shelves.get(i) {
                stack.push((depth + 1, name.clone(), Spot::New(i)));
            }
        }
    }
    out
}

fn title_of(model: &Library, book: BookId) -> Option<String> {
    model.book(book).map(|b| b.title.value.clone())
}

/// The sheet's body; Apply or Discard come back as actions.
pub(super) fn show(ui: &mut Ui, model: &Library, plan: &Proposal, t: &Tokens) -> Option<Action> {
    let rows = rows(model, plan);
    let filed: usize = rows.iter().map(|r| r.titles.len()).sum();
    let new = rows.iter().filter(|r| r.new).count();
    title(
        ui,
        &format!(
            "Organize {}",
            counted(filed + plan.left.len(), "book", "books")
        ),
        t,
    );
    let mut said = format!(
        "The AI files {} on {}",
        counted(filed, "book", "books"),
        counted(rows.len(), "shelf", "shelves")
    );
    if new > 0 {
        said.push_str(&format!(", {new} of them new"));
    }
    said.push_str(". Nothing moves until you apply it.");
    ui.label(RichText::new(said).size(t.text_small).color(t.muted));
    ui.add_space(4.0);
    ScrollArea::vertical()
        .max_height(LIST_H)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            for row in &rows {
                shelf_row(ui, row, t);
            }
            let left: Vec<String> = (plan.left.iter())
                .filter_map(|b| title_of(model, *b))
                .collect();
            if !left.is_empty() {
                ui.separator();
                let row = Row {
                    depth: 0,
                    name: "Stay on no shelf".into(),
                    new: false,
                    titles: left,
                };
                shelf_row(ui, &row, t);
            }
        });
    ui.add_space(8.0);
    let mut act = None;
    buttons(ui, |ui| {
        let tip = "Make the new shelves and file the books; you can undo it";
        let apply = ui.add_enabled_ui(filed > 0, |ui| primary(ui, "Apply", t.accent, t));
        if apply.inner.on_hover_text(tip).clicked() {
            act = Some(Action::ApplyOrganized);
        }
        let tip = "Let the plan go; nothing moves";
        if ui.button("Discard").on_hover_text(tip).clicked() {
            act = Some(Action::DiscardOrganized);
        }
    });
    act
}

/// A shelf's line, its count at the end, and the first titles beneath.
fn shelf_row(ui: &mut Ui, row: &Row, t: &Tokens) {
    // Its titles sit close under it, apart from the next shelf.
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        shelf_lines(ui, row, t);
    });
}

fn shelf_lines(ui: &mut Ui, row: &Row, t: &Tokens) {
    let indent = row.depth as f32 * 18.0;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        let icon = Icon::Shelf.image(ui, 15.0).tint(t.muted);
        ui.add(icon);
        let name = RichText::new(&row.name).font(theme::medium(t.text_small + 1.0));
        ui.add(egui::Label::new(name.color(t.ink)).truncate());
        if row.new {
            new_tag(ui, t);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let n = counted(row.titles.len(), "book", "books");
            ui.label(RichText::new(n).size(t.text_small).color(t.muted));
        });
    });
    if row.titles.is_empty() {
        return;
    }
    let mut named: Vec<&str> = row.titles.iter().take(NAMED).map(String::as_str).collect();
    let more = row.titles.len().saturating_sub(NAMED);
    let more = format!("and {more} more");
    if row.titles.len() > NAMED {
        named.push(&more);
    }
    ui.horizontal(|ui| {
        ui.add_space(indent + 21.0);
        let text = RichText::new(named.join(", ")).size(t.text_small - 1.0);
        ui.add(egui::Label::new(text.color(t.muted)).truncate());
    });
}

fn new_tag(ui: &mut Ui, t: &Tokens) {
    let text = RichText::new("New").size(10.5).color(t.accent);
    let tag = egui::Frame::new()
        .fill(t.accent.gamma_multiply(0.12))
        .corner_radius(t.radius)
        .inner_margin(egui::Margin::symmetric(5, 1));
    tag.show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        ui.label(text);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::LibraryState;

    #[test]
    fn the_tree_puts_new_shelves_under_their_parents_with_their_books() {
        let mut state = LibraryState::default();
        state.seed_copies(4);
        let model = state.model_mut();
        let Ok(fiction) = model.create_shelf("Fiction", None, 1) else {
            panic!("shelf")
        };
        let mut books: Vec<BookId> = model.books().map(|b| b.id).collect();
        books.sort();
        let [a, b, c, d] = books.as_slice() else {
            panic!("four books")
        };
        let plan = Proposal {
            shelves: vec![
                ("Science".into(), None),
                ("Classics".into(), Some(Spot::Shelf(fiction))),
                ("Rivers".into(), Some(Spot::New(0))),
            ],
            placed: vec![
                (*a, Spot::New(1)),
                (*b, Spot::New(2)),
                (*c, Spot::Shelf(fiction)),
            ],
            left: vec![*d],
        };
        let rows = rows(state.model(), &plan);
        let shape: Vec<(usize, &str, bool, usize)> = (rows.iter())
            .map(|r| (r.depth, r.name.as_str(), r.new, r.titles.len()))
            .collect();
        assert_eq!(
            shape,
            vec![
                (0, "Fiction", false, 1),
                (1, "Classics", true, 1),
                (0, "Science", true, 0),
                (1, "Rivers", true, 1),
            ]
        );
    }
}
