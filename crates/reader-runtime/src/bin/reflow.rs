//! The reflow pane artifact's entry (`reflow.html`): one Markdown or text
//! pane, in its own frame; pdf.js is never loaded here
//! (docs/pane-runtimes.md).

fn main() {
    reader_runtime::pane_frame::boot(reader_runtime::pane_wire::PaneKind::Reflow);
}
