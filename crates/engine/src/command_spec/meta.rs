//! Static UI metadata, enablement checks, and journal flags for `CommandSpec`.

use crate::route::Route;
use crate::session::Session;

/// Static metadata attached to each [`crate::command_spec::CommandSpec`] row.
#[derive(Clone, Copy)]
pub(crate) struct Meta {
    pub title: &'static str,
    pub menu: Option<&'static str>,
    pub shortcut: Option<&'static str>,
    pub can_run: fn(&Session) -> Result<(), &'static str>,
    pub journal: bool,
}

const fn m(
    title: &'static str,
    menu: Option<&'static str>,
    shortcut: Option<&'static str>,
    can_run: fn(&Session) -> Result<(), &'static str>,
    journal: bool,
) -> Meta {
    Meta {
        title,
        menu,
        shortcut,
        can_run,
        journal,
    }
}

fn always(_: &Session) -> Result<(), &'static str> {
    Ok(())
}

fn in_reader(session: &Session) -> Result<(), &'static str> {
    if session.route() == Route::Reader {
        Ok(())
    } else {
        Err("Available only in the reader")
    }
}

fn has_doc(session: &Session) -> Result<(), &'static str> {
    if session.route() != Route::Reader {
        Err("Available only in the reader")
    } else if session.documents().slot(session.active()).is_some() {
        Ok(())
    } else {
        Err("Open a document first")
    }
}

fn has_split(session: &Session) -> Result<(), &'static str> {
    if session.route() != Route::Reader {
        Err("Available only in the reader")
    } else if session.tree().panes().len() > 1 {
        Ok(())
    } else {
        Err("Split the view first")
    }
}

fn can_split(session: &Session) -> Result<(), &'static str> {
    if session.route() != Route::Reader {
        Err("Available only in the reader")
    } else if session.tree().panes().len() < mareader_workspace::MAX_PANES {
        Ok(())
    } else {
        Err("Workspace is full")
    }
}

const FALLBACK: Meta = m("", None, None, always, true);

#[rustfmt::skip]
const TABLE: &[(&str, Meta)] = &[
    ("ai.job", m("Run AI Job", Some("AI"), None, always, false)),
    ("ai.page", m("Page AI Action", Some("AI"), None, has_doc, false)),
    ("command.batch", m("Run Command Batch", None, None, always, false)),
    ("dict.act", m("Dictionary Action", Some("Study"), None, always, true)),
    ("dict.pronunciation", m("Pronunciation Pack", Some("Study"), None, always, true)),
    ("dict.settings", m("Dictionary Settings", Some("Study"), None, always, true)),
    ("export.write", m("Export Notes", Some("File"), None, in_reader, false)),
    ("library.act", m("Library Action", Some("Library"), None, always, true)),
    ("library.forget", m("Forget Reading Positions", Some("Library"), None, always, true)),
    ("library.view", m("Library View Options", Some("Library"), None, always, true)),
    ("mark.bookmark", m("Bookmark Page", Some("Reading"), Some("Mod+D"), has_doc, true)),
    ("mark.clear", m("Clear Selection", Some("Reading"), None, has_doc, true)),
    ("mark.highlight", m("Highlight Selection", Some("Reading"), None, has_doc, true)),
    ("mark.note", m("Add Note", Some("Reading"), None, has_doc, true)),
    ("mark.select", m("Select Text", Some("Reading"), None, has_doc, true)),
    ("page.arrow", m("Navigate Arrow", Some("Page"), None, has_doc, true)),
    ("page.autoscroll", m("Toggle Auto-Scroll", Some("View"), Some("Shift+A"), has_doc, true)),
    ("page.copy", m("Copy Pages Text", Some("Page"), None, has_doc, false)),
    ("page.first", m("First Page", Some("Page"), Some("Home"), has_doc, true)),
    ("page.go", m("Go to Page", Some("Page"), None, has_doc, true)),
    ("page.last", m("Last Page", Some("Page"), Some("End"), has_doc, true)),
    ("page.search", m("Search Document", Some("Search"), None, has_doc, true)),
    ("page.step", m("Step Screen", Some("Page"), Some("Space"), has_doc, true)),
    ("page.turn", m("Turn Page", Some("Page"), None, has_doc, true)),
    ("pane.close", m("Close Pane", Some("Pane"), None, has_split, true)),
    ("pane.cycle", m("Cycle Pane Focus", Some("Pane"), Some("F6"), has_split, true)),
    ("pane.focus", m("Focus Pane", Some("Pane"), None, in_reader, true)),
    ("pane.move", m("Move Pane", Some("Pane"), None, has_split, true)),
    ("pane.ratio", m("Resize Split Ratio", Some("Pane"), None, has_split, true)),
    ("pane.split", m("Split Pane", Some("Pane"), None, can_split, true)),
    ("pane.swap", m("Swap Panes", Some("Pane"), None, has_split, true)),
    ("pane.view", m("Set View Mode", Some("View"), None, in_reader, true)),
    ("pane.zoom", m("Zoom", Some("View"), None, in_reader, true)),
    ("read.act", m("Read Aloud Action", Some("Reading"), None, has_doc, true)),
    ("read.marker", m("Place Read Marker", Some("Reading"), None, has_doc, true)),
    ("read.start", m("Start Read Aloud", Some("Reading"), None, has_doc, true)),
    ("read.stop", m("Stop Read Aloud", Some("Reading"), None, in_reader, false)),
    ("read.toggle", m("Read Aloud", Some("Reading"), Some("Mod+Shift+U"), has_doc, true)),
    ("reader.open", m("Open File", Some("File"), None, always, true)),
    ("reader.open-book", m("Open Library Book", Some("Library"), None, always, true)),
    ("reader.place-book", m("Place Book in Pane", Some("Library"), None, always, true)),
    ("route.show", m("Switch Route", Some("App"), None, always, true)),
    ("settings.appearance", m("Set Appearance", Some("View"), None, in_reader, true)),
    ("settings.independence", m("Set Split Independence", Some("View"), None, in_reader, true)),
    ("settings.ink", m("Set Ink Contrast", Some("View"), None, in_reader, true)),
    ("settings.open", m("Open Settings Tab", Some("App"), None, always, false)),
    ("settings.preset-delete", m("Delete Appearance Preset", Some("View"), None, always, true)),
    ("settings.preset-save", m("Save Appearance Preset", Some("View"), None, always, true)),
    ("settings.quick-text", m("Quick Text Settings", Some("View"), None, in_reader, true)),
    ("settings.set", m("Set Setting Field", Some("App"), None, always, true)),
    ("settings.sidebar-overlay", m("Set Sidebar Overlay", Some("View"), None, always, true)),
    ("settings.titlebar-pin", m("Pin Title Bar", Some("View"), None, always, true)),
    ("settings.write", m("Replace Settings", Some("App"), None, always, true)),
    ("space.word", m("Space Word Action", Some("Study"), None, always, true)),
    ("study.act", m("Study Action", Some("Study"), None, in_reader, true)),
    ("study.mode", m("Study Mode", Some("Study"), None, in_reader, true)),
    ("update.act", m("Check for Updates", Some("App"), None, always, false)),
    ("vocab.attach", m("Attach Vocabulary Dataset", Some("Study"), None, always, true)),
    ("vocab.download", m("Vocabulary Dataset Action", Some("Study"), None, always, true)),
    ("voice.model", m("Voice Model Action", Some("Reading"), None, always, false)),
    ("voice.reveal", m("Reveal Voice Folder", Some("Reading"), None, always, false)),
    ("voice.stop-try", m("Stop Voice Preview", Some("Reading"), None, always, false)),
    ("voice.try", m("Preview Voice", Some("Reading"), None, always, false)),
    ("voice.try-of", m("Preview Specific Voice", Some("Reading"), None, always, false)),
    ("word.explain", m("Explain Selection", Some("Study"), None, has_doc, false)),
    ("word.list", m("Build Word List", Some("Study"), None, has_doc, false)),
    ("word.list-window", m("Open Word List Window", Some("Study"), None, has_doc, false)),
    ("word.lookup", m("Look Up Word", Some("Study"), None, has_doc, false)),
    ("word.say", m("Pronounce Word", Some("Study"), None, always, false)),
    ("word.say-with", m("Pronounce Word with Voice", Some("Study"), None, always, false)),
    ("word.stop", m("Stop Word Pronunciation", Some("Study"), None, always, false)),
];

const fn eq_bytes(mut a: &[u8], mut b: &[u8]) -> bool {
    while let (Some((x, xs)), Some((y, ys))) = (a.split_first(), b.split_first()) {
        if *x != *y {
            return false;
        }
        a = xs;
        b = ys;
    }
    a.is_empty() && b.is_empty()
}

/// Looks up static metadata for `id` at compile time.
pub(crate) const fn for_id(id: &'static str) -> Meta {
    let mut rest = TABLE;
    while let Some(((key, meta), tail)) = rest.split_first() {
        if eq_bytes(key.as_bytes(), id.as_bytes()) {
            return *meta;
        }
        rest = tail;
    }
    FALLBACK
}
