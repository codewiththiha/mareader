//! The library artifact's entry: standalone boot; no PDF engine on
//! this shared page.

fn main() {
    // Standalone means no frame marker (§6): the hosted boot answers
    // the Shell's offer.
    if !library_runtime::frame::boot_if_hosted() {
        library_runtime::run_standalone();
    }
}
