use super::{BookAction, Sheet, State};

#[test]
fn bulk_removal_rechecks_books_after_confirmation() {
    let mut state = State::default();
    state.seed(8);
    state.model.check_present(|path| {
        !path.to_string_lossy().starts_with("/books/3-")
            && !path.to_string_lossy().starts_with("/books/7-")
    });
    let ids: Vec<_> = state
        .model
        .books()
        .filter(|b| b.missing())
        .map(|b| b.id)
        .collect();
    assert_eq!(ids.len(), 2);
    let mut out = Vec::new();
    state.book_act(BookAction::AskRemoveMissing(ids.clone()), &mut out);
    assert!(matches!(state.sheet, Some(Sheet::MissingRemoval { .. })));
    state
        .model
        .check_present(|path| !path.to_string_lossy().starts_with("/books/7-"));
    let recovered = ids
        .iter()
        .copied()
        .find(|id| !state.model.book(*id).unwrap().missing())
        .unwrap();
    let absent = ids.iter().copied().find(|id| *id != recovered).unwrap();
    state.book_act(
        BookAction::RemoveMissing {
            books: ids,
            forget: false,
        },
        &mut out,
    );
    assert!(state.model.book(recovered).is_some());
    assert!(state.model.book(absent).is_none());
}
