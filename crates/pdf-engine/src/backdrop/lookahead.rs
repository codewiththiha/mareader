//! The look-ahead: resolve the pages the reader is approaching.
use super::{Paper, land_sample, publish, slot, spawn_engine};
use crate::session::PdfSession;

/// The pages whose colour the session wants known.
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

/// Resolve the pages [`lookahead_wants`] names, epoch-guarded.
pub(super) fn ensure_lookahead(session: &PdfSession) {
    let (epoch, pages) = session.with_paper(|s| {
        let wants = lookahead_wants(s);
        for page in &wants {
            s.start_sample(*page);
        }
        (s.epoch, wants)
    });
    for page in pages {
        sample_page(session, epoch, page);
    }
}

pub(super) fn sample_page(session: &PdfSession, epoch: u64, page: u32) {
    spawn_engine(session, move |session| async move {
        let frame = session.sample_paper_page(page).await.ok().flatten();
        if land_sample(&session, epoch, page, frame.as_ref()) {
            publish(&session);
        }
    });
}
