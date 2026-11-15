//! Documents made in memory, so a test can read one without a file.
//!
//! A `DocId` comes only from opening, and opening is asynchronous; a test that
//! needs a handle should not wait on a worker to get one. The sample itself is
//! drawn by `mareader-pdf`, so nothing here makes a pixel.

use std::sync::Arc;

use mareader_pdf::sample::pdf;
use mareader_session::{Budget, DocId, Document, Event, Session, Source};

/// A `DocId` for a one-page sample that opened here, synchronously.
///
/// The only failure is a sample that would not open: a broken build, not a
/// condition a caller handles.
pub fn doc() -> Result<DocId, &'static str> {
    let mut s = Session::new(0, || {});
    let bytes = Arc::new(pdf(1, "t"));
    s.open(Source::Bytes(bytes), None, Default::default());
    opened(&mut s)
}

/// A sample book that opened here, with the id it opened under.
///
/// `search` keys a job by the id and the document together, so a test that
/// asks one for a query needs the pair that opening made.
pub fn book(pages: usize) -> Result<(DocId, Arc<Document>), &'static str> {
    let mut made = books(1, pages)?;
    made.pop().ok_or("the sample didn't open")
}

/// `count` sample books opened in one session, so their ids differ.
pub fn books(count: usize, pages: usize) -> Result<Vec<(DocId, Arc<Document>)>, &'static str> {
    let mut session = Session::new(0, || {});
    let mut made = Vec::new();
    for _ in 0..count {
        let bytes = Arc::new(pdf(pages, "t"));
        session.open(Source::Bytes(bytes), None, Default::default());
        let id = opened(&mut session)?;
        let entry = session.get(id).ok_or("the sample didn't open")?;
        made.push((id, Arc::clone(&entry.doc)));
    }
    Ok(made)
}

/// The id of the book that just opened, synchronously, in `session`.
fn opened(session: &mut Session) -> Result<DocId, &'static str> {
    let budget = Budget {
        items: 1,
        bytes: usize::MAX,
    };
    session
        .poll(budget)
        .into_iter()
        .find_map(|e| match e {
            Event::Opened { result: Ok(id), .. } => Some(id),
            _ => None,
        })
        .ok_or("the sample didn't open")
}
