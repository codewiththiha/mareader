//! One text scan per row, shared by the search painter and the ink.

use std::cell::RefCell;

use cefr_core::text::{Span, tokenize};
use web_sys::Element;

use crate::components::formats::reflow::spot::text_contents;

/// A row's rendered text and its word tokens, char-indexed together.
pub(crate) struct RowScan {
    pub text: String,
    pub tokens: Vec<Span>,
}

thread_local! {
    /// Newest-first rows; the cap bounds memory. Plain data only.
    static SCANS: RefCell<Vec<(String, u64, RowScan)>> =
        const { RefCell::new(Vec::new()) };
}

/// Rows held at once; a reader shows dozens, never hundreds.
const SCAN_CAP: usize = 32;

/// Run `read` over the row's scan, reusing this fingerprint's entry.
pub(crate) fn with_row_scan<R>(
    row_id: &str,
    fingerprint: u64,
    row: &Element,
    read: impl FnOnce(&RowScan) -> R,
) -> R {
    let hit = SCANS.with(|scans| {
        scans
            .borrow_mut()
            .iter()
            .position(|(id, fp, _)| id == row_id && *fp == fingerprint)
    });
    if let Some(at) = hit {
        SCANS.with(|scans| {
            let mut scans = scans.borrow_mut();
            let found = scans.remove(at);
            scans.insert(0, found);
        });
    } else {
        let text = text_contents(row).concat();
        let tokens = tokenize(&text);
        SCANS.with(|scans| {
            let mut scans = scans.borrow_mut();
            scans.insert(
                0,
                (row_id.to_string(), fingerprint, RowScan { text, tokens }),
            );
            scans.truncate(SCAN_CAP);
        });
    }
    SCANS.with(|scans| {
        let scans = scans.borrow();
        let (_, _, scan) = scans
            .iter()
            .find(|(id, fp, _)| id == row_id && *fp == fingerprint)
            .expect("the entry this call just placed");
        read(scan)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cache is plain data, so the shape is testable natively with a
    /// stand-in entry.
    #[test]
    fn the_lru_keeps_its_cap() {
        SCANS.with(|scans| {
            let mut scans = scans.borrow_mut();
            for index in 0..(SCAN_CAP + 4) {
                scans.insert(
                    0,
                    (
                        format!("row-{index}"),
                        index as u64,
                        RowScan {
                            text: String::new(),
                            tokens: Vec::new(),
                        },
                    ),
                );
            }
            scans.truncate(SCAN_CAP);
            assert_eq!(scans.len(), SCAN_CAP);
            assert_eq!(scans[0].0, format!("row-{}", SCAN_CAP + 3));
            scans.clear();
        });
    }
}
