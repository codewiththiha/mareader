//! Drivers shared by EPUB integration suites.

use mareader_epub::Book;

pub(crate) fn text(book: &Book) -> String {
    let pages = 0..book.page_count();
    let lines = pages.flat_map(|p| book.text(p));
    lines
        .map(|l| l.chars.iter().map(|c| c.ch).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn every_page_draws(book: &Book) {
    for page in 0..book.page_count() {
        let pixels = book.render(page, 0.5, [0, 0, 397, 561]).unwrap();
        assert!(pixels.rgba.chunks(4).any(|p| p[0] < 128), "page {page}");
    }
}
