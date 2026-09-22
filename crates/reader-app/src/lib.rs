//! The document reader: everything that is true of an OPEN document, and
//! nothing that is true of a window.
//!
//! The crate is the reader half of a split whose other half is the shell —
//! the route, the library, the title bars, the settings modal and the
//! persistence that binds them. It is a library rather than a binary today,
//! and the shell renders it as one route of one window; the point of the
//! boundary is that it is already the boundary a second binary needs.
//!
//! What lives here:
//!
//!   - [`state`] — [`state::ReaderState`], the open document's reactive
//!     truth: the document and its cut, the viewer signals, the zoom
//!     pipeline's shape, the search results, the selection and the gloss
//!     marks.
//!   - [`components`] — what paints that state: the viewer and its four
//!     layouts, one module per format, the AI reading features, the search
//!     surfaces and the rail with its panels.
//!   - [`effects`] — the reactive systems that keep it in sync: navigation,
//!     the reflow measurement pipeline, the mode flip, the zoom watchers,
//!     selection tracking and search.
//!   - [`zoom`] — the zoom controller and its transition pipeline. Zoom has
//!     exactly one owner and this is it.
//!   - [`features`] — the compositions: the reader root below the title bar,
//!     the rail, and the two virtualizers.
//!   - [`dom_contract`] and [`epoch`] — the two names this crate shares with
//!     the JavaScript engine: the ids it builds page hosts under, and the
//!     generation counter that tells a stale render task to stop.
//!
//! What does NOT live here, and the rule that keeps it out:
//!
//!   - **The window.** No title bar, no traffic lights, no route, no toast.
//!     The layout questions the rail and the bars share are asked of
//!     `app_chrome::controller::ShellController`, which the shell builds and
//!     hands in.
//!   - **The library.** No rows, no shelves, no covers except the map the
//!     rail's identity row is handed. The reading position this crate
//!     produces is written to the library by the shell's own effect.
//!   - **Storage.** This crate never touches localStorage. The one write it
//!     owes — the open document's gloss marks — leaves through the
//!     [`state::GlossSave`] callback the shell injects, because the blob
//!     those marks land in is the library's.
//!   - **Settings ownership.** The persisted `Settings` arrives as a signal
//!     and is read (and, for the reader's own knobs, written) but never
//!     loaded or saved here.
//!
//! The consequence is the one that matters: nothing in this crate can name
//! the shell's `AppState`. Every entry point takes the narrow thing it
//! needs — [`state::ReaderState`], a settings signal, a cover map, a
//! controller — so a reader instance can be built by whoever is hosting it,
//! which in the next phase is a document of its own.

pub mod components;
pub mod dom_contract;
pub mod effects;
pub mod epoch;
pub mod features;
pub mod state;
pub mod zoom;
