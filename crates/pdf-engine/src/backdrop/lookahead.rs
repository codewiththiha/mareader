//! The look-ahead: resolve the pages the reader is approaching before the
//! reader arrives.

use super::{Session, feed_state, publish, spawn_engine, with};
use crate::api;

/// The pages whose colour the session wants known: the pair the reader
/// is straddling plus the one after it, so the colour is resolved before
/// the reader arrives. Pure — the test exercises exactly this choice.
pub(super) fn lookahead_wants(s: &Session) -> Vec<u32> {
    if !s.blend_on || s.num_pages == 0 {
        return Vec::new();
    }
    let base = s.position.floor().max(1.0) as u32;
    let mut wants = Vec::new();
    for page in [base, base + 1, base + 2] {
        if (1..=s.num_pages).contains(&page)
            && !s.palette.contains(page)
            && !s.sampling.contains(&page)
        {
            wants.push(page);
        }
    }
    wants
}

/// Resolve (offscreen) the pages [`lookahead_wants`] names, one spawn each,
/// all generation-guarded so a sample for one book cannot land in the next.
pub(super) fn ensure_lookahead() {
    let pages = with(|s| {
        let wants = lookahead_wants(s);
        for page in &wants {
            s.sampling.insert(*page);
        }
        wants
    });
    for page in pages {
        spawn_engine(move || async move {
            let epoch = with(|s| s.epoch);
            let frame = api::sample_paper_page(page).await.ok().flatten();
            let changed = with(|s| {
                if s.epoch != epoch {
                    return false;
                }
                s.sampling.remove(&page);
                match &frame {
                    Some(f) => feed_state(s, f),
                    None => false, // unreadable page: nothing to learn
                }
            });
            if changed {
                publish();
            }
        });
    }
}
