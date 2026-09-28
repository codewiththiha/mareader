//! The pane's handle onto the PDF engine session.
//!
//! Every document-level engine call a pane makes goes through here, guarded
//! by the pane's (and the session's) lifecycle as captured when the handle
//! was made: work ops no-op once disposal began, teardown ops stay admitted
//! until `Disposed`. The engine behind it is still the module-wide
//! `pdf_engine::api` singleton — one document at a time per artifact — and
//! per-pane engine sessions are the format split's job, not this handle's.

/// The guarded engine-session handle. Made by
/// [`crate::pane::handle::PaneHandle::pdf`]; capture it fresh at each use.
#[derive(Clone, Copy)]
pub struct PdfSessionHandle {
    work: bool,
    teardown: bool,
}

impl PdfSessionHandle {
    pub(crate) fn new(work: bool, teardown: bool) -> Self {
        Self { work, teardown }
    }

    /// Whether the pane may start document work. The open flow checks this
    /// before an open (it owns what a refused open means for the UI).
    pub fn work_admitted(&self) -> bool {
        self.work
    }

    /// Open a document through the engine session.
    pub async fn open(
        &self,
        path: &str,
    ) -> Result<pdf_engine::types::OpenResult, pdf_engine::api::EngineError> {
        pdf_engine::api::open(path).await
    }

    pub async fn destroy(&self) {
        if !self.teardown {
            return;
        }
        let _ = pdf_engine::api::destroy().await;
    }

    pub fn sweep(&self) {
        if !self.teardown {
            return;
        }
        pdf_engine::api::sweep();
    }

    pub fn sweep_snapshots(&self) {
        if !self.teardown {
            return;
        }
        pdf_engine::api::sweep_snapshots();
    }
}
