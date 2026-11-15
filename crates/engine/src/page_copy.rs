//! Copies whole pages' text, read off the frame when it may take a while.
//!
//! Reading a page's text layer can outlast a frame, so the reading runs on a
//! helper thread and asks for a frame through the [`Waker`] it was attached
//! with, as `dict`, `vocab` and `sounds` do. The clipboard is the window's:
//! [`PageCopy::poll`] hands the text back rather than setting it.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use mareader_session::Document;

use crate::session::Waker;
use crate::toast::Toast;

/// Pages this few, or a book's, are read in the frame.
const INLINE: usize = 4;

/// The one copy being read; a newer one replaces it. No `Debug`: a [`Waker`]
/// is a closure, and `Sounds` drops the derive for the same reason.
pub struct PageCopy {
    reading: Option<Receiver<Copied>>,
    /// Asks the window for a frame when a copy lands; a no-op until attached.
    waker: Waker,
}

/// What the helper thread finished reading.
#[derive(Debug)]
struct Copied {
    text: String,
    pages: usize,
}

/// A read copy and the toast that reports it.
#[derive(Debug)]
pub struct Read {
    /// The pages' text, for the window to put on the clipboard.
    pub text: String,
    /// What the reader is told; the window pushes it.
    pub toast: Toast,
}

impl Default for PageCopy {
    fn default() -> Self {
        Self {
            reading: None,
            waker: Arc::new(|| {}),
        }
    }
}

impl PageCopy {
    /// Keeps `waker` to ask for a frame when a copy lands.
    pub fn attach(&mut self, waker: Waker) {
        self.waker = waker;
    }

    /// Whether a page-copy helper is still reading pages.
    pub fn busy(&self) -> bool {
        self.reading.is_some()
    }

    /// Starts reading `pages` of `document`, to copy them once read.
    pub fn start(&mut self, document: Arc<Document>, pages: Vec<usize>) {
        let (tx, rx) = channel();
        let inline = pages.len() <= INLINE || document.is_reflowable();
        let wake = Arc::clone(&self.waker);
        let work = move || {
            let copied = Copied {
                text: joined(&document, &pages),
                pages: pages.len(),
            };
            let _ = tx.send(copied);
            wake();
        };
        if inline || cfg!(target_arch = "wasm32") {
            work();
        } else {
            let named = std::thread::Builder::new().name("page-copy".into());
            // Unspawned, the copy just never lands.
            let _ = named.spawn(work);
        }
        self.reading = Some(rx);
    }

    /// The copy that finished, with the toast that reports it.
    pub fn poll(&mut self) -> Option<Read> {
        let copied = match self.reading.as_ref()?.try_recv() {
            Ok(copied) => copied,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                self.reading = None;
                return Some(Read {
                    text: String::new(),
                    toast: Toast::info("Couldn't read those pages"),
                });
            }
        };
        self.reading = None;
        if copied.text.is_empty() {
            return Some(Read {
                text: String::new(),
                toast: Toast::info("No text to copy on these pages"),
            });
        }
        let toast = Toast::info(match copied.pages {
            1 => "Copied the page's text".to_owned(),
            n => format!("Copied the text of {n} pages"),
        });
        Some(Read {
            text: copied.text,
            toast,
        })
    }
}

/// The pages' text in order, a blank line between pages.
fn joined(document: &Document, pages: &[usize]) -> String {
    let texts = pages.iter().filter_map(|&page| {
        let layer = document.text_layer(page).ok()?;
        let text = layer.text.text(0..layer.text.len());
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    });
    texts.collect::<Vec<_>>().join("\n\n")
}

#[cfg(test)]
#[path = "page_copy_tests.rs"]
mod tests;
