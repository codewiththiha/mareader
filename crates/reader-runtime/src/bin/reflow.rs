//! The reflow pane artifact's entry (`reflow.html`); pdf.js never loads here.

fn main() {
    reader_runtime::pane_frame::boot(reader_runtime::pane_wire::PaneKind::Reflow);
}
