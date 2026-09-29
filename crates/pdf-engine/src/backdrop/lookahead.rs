//! The look-ahead: resolve the pages the reader is approaching before the
//! reader arrives. Per session — the planner reads, and the in-flight set
//! lives on, the session's own paper state.

use super::{Paper, land_sample, publish, slot, spawn_engine};
use crate::session::PdfSession;

/// The pages whose colour the session wants known: the pair the reader
/// is straddling plus the one after it, so the colour is resolved before
/// the reader arrives. Pure — the test exercises exactly this choice.
pub(super) fn lookahead_wants(s: &Paper) -> Vec<u32> {
    if !s.blend_on || s.num_pages == 0 {
        return Vec::new();
    }
    let base = s.position.floor().max(1.0) as u32;
    let mut wants = Vec::new();
    for page in [base, base + 1, base + 2] {
        if (1..=s.num_pages).contains(&page)
            && !s.palettes[slot(s.config.area)].contains(page)
            && !s.sampling.contains(&page)
        {
            wants.push(page);
        }
    }
    wants
}

/// Resolve (offscreen) the pages [`lookahead_wants`] names, one spawn each,
/// all session- and epoch-guarded so a sample for one document cannot land
/// in the next, nor in a session disposed while it ran.
pub(super) fn ensure_lookahead(session: &PdfSession) {
    let (epoch, pages) = session.with_paper(|s| {
        let wants = lookahead_wants(s);
        for page in &wants {
            s.start_sample(*page);
        }
        (s.epoch, wants)
    });
    for page in pages {
        spawn_engine(session, move |session| async move {
            let frame = session.sample_paper_page(page).await.ok().flatten();
            if land_sample(&session, epoch, page, frame.as_ref()) {
                publish(&session);
            }
        });
    }
}
