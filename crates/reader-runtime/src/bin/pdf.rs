//! The PDF pane artifact's entry (`pdf.html`): one pane, pdf.js beside it.

fn main() {
    reader_runtime::pane_frame::boot(reader_runtime::pane_wire::PaneKind::Pdf);
}
