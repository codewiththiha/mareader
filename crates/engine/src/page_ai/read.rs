//! A book's PDF pages read on a thread, so the frame never waits.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use mareader_library::PageSpan;
use mareader_session::Document;

use super::extract::{self, Source};
use crate::session::Waker;

/// A page read off the frame; none if it has no text.
pub type Read = (usize, Option<(Source, PageSpan)>);

/// Reads PDF pages' text on a thread, so the frame never waits.
pub fn pages(
    wake: Waker,
    document: Arc<Document>,
    pages: Vec<usize>,
    inline: bool,
) -> Receiver<Read> {
    let (tx, rx) = channel();
    let work = move || {
        let mut layers = HashMap::new();
        for page in pages {
            let near = [page.checked_sub(1), Some(page), Some(page + 1)];
            for p in near.into_iter().flatten() {
                layers
                    .entry(p)
                    .or_insert_with(|| document.text_layer(p).ok());
            }
            let layer = |p: usize| layers.get(&p).and_then(Option::as_ref);
            let found = layer(page).map(|own| {
                let near: Vec<_> = [page.checked_sub(1), Some(page + 1)]
                    .into_iter()
                    .flatten()
                    .filter_map(layer)
                    .map(|l| &l.text)
                    .collect();
                (extract::pdf(&own.text, &near), PageSpan::Page { page })
            });
            if tx.send((page, found)).is_err() {
                return;
            }
            wake();
        }
    };
    if inline || cfg!(target_arch = "wasm32") {
        work();
    } else {
        // Unspawned, the run just asks nothing more.
        let named = std::thread::Builder::new().name("page-ai-read".into());
        let _ = named.spawn(work);
    }
    rx
}

/// Reads a PDF group's pages on a thread, as one source.
pub fn group(
    wake: Waker,
    document: Arc<Document>,
    (from, to): (usize, usize),
    inline: bool,
) -> Receiver<Option<Source>> {
    let (tx, rx) = channel();
    let work = move || {
        let layers: Vec<_> = (from.saturating_sub(1)..=to + 1)
            .map(|p| (p, document.text_layer(p).ok()))
            .collect();
        let layer = |p: usize| {
            let found = layers.iter().find(|(q, _)| *q == p);
            found.and_then(|(_, l)| l.as_ref())
        };
        let pages: Vec<(Source, bool)> = (from..=to)
            .filter_map(|page| {
                let own = layer(page)?;
                let near: Vec<_> = [page.checked_sub(1), Some(page + 1)]
                    .into_iter()
                    .flatten()
                    .filter_map(layer)
                    .map(|l| &l.text)
                    .collect();
                let runs_on = page > from && own.text.continues();
                Some((extract::pdf(&own.text, &near), runs_on))
            })
            .collect();
        let found = (!pages.is_empty()).then(|| extract::join(pages));
        let _ = tx.send(found);
        wake();
    };
    if inline || cfg!(target_arch = "wasm32") {
        work();
    } else {
        let named = std::thread::Builder::new().name("page-ai-group".into());
        // Unspawned, the group just counts as without text.
        let _ = named.spawn(work);
    }
    rx
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
