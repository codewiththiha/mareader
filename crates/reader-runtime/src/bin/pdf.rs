//! The PDF pane artifact's entry (`pdf.html`): one pane, in its own frame,
//! with pdf.js and its worker beside it (docs/pane-runtimes.md).

fn main() {
    reader_runtime::pane_frame::boot(reader_runtime::pane_wire::PaneKind::Pdf);
}
