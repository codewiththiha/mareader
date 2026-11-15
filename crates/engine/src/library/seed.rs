//! Made-up books and copies, so shots and tests start from a filled shelf.

use std::path::PathBuf;

use mareader_library::{Copied, Format, Found, Opened, Position};

use super::{State, wall_ms};

/// Titles the made-up books wear.
const SEEDS: [(&str, Option<&str>); 10] = [
    ("The Raven", Some("Edgar Allan Poe")),
    ("Middlemarch", Some("George Eliot")),
    ("A Field Guide to Ferns", None),
    ("Notes on Typography", Some("Ada Bringhurst")),
    ("The Voyage Out", Some("Virginia Woolf")),
    ("Collected Essays, Volume 10", Some("M. Aurel")),
    ("Letters from the Coast", None),
    ("An Atlas of Rivers", Some("Lena Ortiz")),
    ("Collected Essays, Volume 2", Some("M. Aurel")),
    ("Practical Rust", Some("Sam Okafor")),
];

impl State {
    /// Adds made-up imported copies, on no shelf, for shots and tests.
    pub fn seed_copies(&mut self, count: usize) {
        let from = self.model.len();
        let pairs: Vec<(Found, Copied)> = (from..from + count)
            .map(|i| {
                let (title, author) = SEEDS
                    .get(i % SEEDS.len())
                    .copied()
                    .unwrap_or(("Book", None));
                let measure = mareader_library::Measure {
                    content: mareader_library::ContentId(i as u128 + 1),
                    stamp: Default::default(),
                };
                let path = PathBuf::from(format!("/store/{i}-{title}.epub"));
                let found = Found {
                    path: path.clone(),
                    format: Format::Epub,
                    measure,
                    title: Some(title.to_owned()),
                    author: author.map(str::to_owned),
                };
                (found, Copied { path, measure })
            })
            .collect();
        self.model.land_copies(&pairs, None, wall_ms());
    }

    /// Adds made-up books for shots: some started, one missing.
    pub fn seed(&mut self, count: usize) {
        let formats = [Format::Epub, Format::Pdf, Format::Markdown, Format::Text];
        let now = wall_ms();
        let from = self.model.len();
        for i in from..from + count {
            let (title, author) = SEEDS
                .get(i % SEEDS.len())
                .copied()
                .unwrap_or(("Book", None));
            let format = formats
                .get(i % formats.len())
                .copied()
                .unwrap_or(Format::Pdf);
            let opened = Opened {
                path: PathBuf::from(format!("/books/{i}-{title}")),
                measure: mareader_library::Measure {
                    content: mareader_library::ContentId(i as u128 + 1),
                    stamp: Default::default(),
                },
                format,
                title: title.to_owned(),
                author: author.map(str::to_owned),
            };
            let at = now.saturating_sub(i as u64 * 7_000_000);
            let id = self.model.record_open(opened, at);
            if i % 3 != 2 {
                let pages = 40 + 37 * i;
                let position = Position {
                    page: pages * (i - from + 1) / (count + 2),
                    pages,
                    place: None,
                    spine: None,
                };
                self.model.record_position(id, position, at);
            }
        }
        // The fourth seeded book has lost its file.
        self.model
            .check_present(|p| !p.to_string_lossy().starts_with("/books/3-"));
    }
}
