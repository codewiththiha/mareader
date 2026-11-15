use crate::clock::Field;
use crate::test_support::opened;
use crate::{BookId, DeviceId, Format, Home, Item, Library, ShelfId};

const DEVICE: DeviceId = DeviceId(7);

fn library(count: u128) -> (Library, Vec<BookId>) {
    let mut lib = Library::new(DEVICE);
    let ids = (1..=count)
        .map(|n| {
            lib.record_open(
                opened(&format!("/b{n}.pdf"), n, Format::Pdf, None),
                n as u64,
            )
        })
        .collect();
    (lib, ids)
}

fn books(ids: &[BookId]) -> Vec<Item> {
    ids.iter().map(|b| Item::Book(*b)).collect()
}

fn shelf(id: ShelfId) -> Item {
    Item::Shelf(id)
}

#[test]
fn new_books_and_shelves_join_the_end_of_their_level() {
    let (mut lib, ids) = library(2);
    let s = lib.create_shelf("S", None, 10).unwrap();
    let c = lib.record_open(opened("/b3.pdf", 3, Format::Pdf, None), 11);
    let top = lib.arranged(None, Home::Everything);
    assert_eq!(top, [books(&ids), vec![shelf(s), Item::Book(c)]].concat());
    // On a shelf, an inner shelf and books share one order too.
    let inner = lib.create_shelf("Inner", Some(s), 12).unwrap();
    lib.file(ids[0], s, 13);
    assert_eq!(
        lib.arranged(Some(s), Home::Everything),
        [shelf(inner), Item::Book(ids[0])]
    );
}

#[test]
fn shelves_and_books_mix_anywhere_and_moves_keep_their_order() {
    let (mut lib, ids) = library(4);
    let s = lib.create_shelf("S", None, 10).unwrap();
    assert!(lib.arrange(None, &[shelf(s)], Some(Item::Book(ids[1])), 20));
    let want = [Item::Book(ids[0]), shelf(s), Item::Book(ids[1])];
    assert_eq!(lib.arranged(None, Home::Everything)[..3], want[..]);
    // Two carried together land together, in the order given.
    let pair = [Item::Book(ids[3]), Item::Book(ids[0])];
    assert!(lib.arrange(None, &pair, Some(shelf(s)), 21));
    let order = lib.arranged(None, Home::Everything);
    assert_eq!(order[..3], [pair[0], pair[1], shelf(s)]);
    // Unshelved leaves shelved books out, in the same order.
    lib.file(ids[3], s, 22);
    let unshelved = lib.arranged(None, Home::Unshelved);
    assert_eq!(unshelved.first(), Some(&Item::Book(ids[0])));
    assert!(!unshelved.contains(&Item::Book(ids[3])));
}

#[test]
fn a_level_from_before_keys_keeps_its_order_when_first_arranged() {
    let (mut lib, ids) = library(3);
    let s = lib.create_shelf("S", None, 10).unwrap();
    // As an older build saved them: no keys at all.
    for id in &ids {
        lib.books.get_mut(id).unwrap().key = Field::default();
    }
    lib.shelves.get_mut(&s).unwrap().key = Field::default();
    let before = lib.arranged(None, Home::Everything);
    assert_eq!(before, [vec![shelf(s)], books(&ids)].concat());
    assert!(lib.arrange(None, &[Item::Book(ids[2])], Some(Item::Book(ids[0])), 20));
    let after = lib.arranged(None, Home::Everything);
    let want = [
        shelf(s),
        Item::Book(ids[2]),
        Item::Book(ids[0]),
        Item::Book(ids[1]),
    ];
    assert_eq!(after, want);
    assert!(lib.books().all(|b| !b.key.value.is_empty()));
}

#[test]
fn a_twin_sits_right_behind_its_book() {
    let (mut lib, ids) = library(3);
    let twin = lib.twin(ids[0], 10).unwrap();
    let order = lib.arranged(None, Home::Everything);
    assert_eq!(
        order[..3],
        [Item::Book(ids[0]), Item::Book(twin), Item::Book(ids[1])]
    );
}

#[test]
fn a_moved_shelf_joins_its_new_level_at_the_end() {
    let (mut lib, ids) = library(1);
    let a = lib.create_shelf("A", None, 10).unwrap();
    let b = lib.create_shelf("B", None, 11).unwrap();
    lib.file(ids[0], a, 12);
    lib.move_shelf(b, Some(a), 13).unwrap();
    assert_eq!(
        lib.arranged(Some(a), Home::Everything),
        [Item::Book(ids[0]), shelf(b)]
    );
}

#[test]
fn moving_nothing_there_or_before_itself_changes_nothing() {
    let (mut lib, ids) = library(2);
    let s = lib.create_shelf("S", None, 10).unwrap();
    assert!(!lib.arrange(Some(s), &[Item::Book(ids[0])], None, 20));
    let me = Item::Book(ids[0]);
    assert!(!lib.arrange(None, &[me], Some(me), 21));
}

#[test]
fn importing_many_books_stays_cheap_and_ordered() {
    let (lib, ids) = library(3_000);
    let order = lib.arranged(None, Home::Everything);
    assert_eq!(order, books(&ids));
    assert!(lib.books().all(|b| b.key.value.len() <= 60));
}
