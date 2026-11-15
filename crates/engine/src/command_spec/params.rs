//! The keys each command reads, as the registry shows them.
//!
//! One list per command, so a row reads as a row and a script's contract is in
//! one place. A kind says what the key holds: `int` and `real` are numbers,
//! `bool` a flag, `str` text, `path` a file, `object` a shape of its own, `any`
//! any JSON value, and `ints`, `strs`, `paths` and `bools` are lists of them. A
//! kind ending in `?` may be left out. Names an enum spells are listed with `|`
//! between them, and `x,y,w,h` is an object of those four numbers. The key
//! `pane` means the pane in focus when a script leaves it out.

use crate::command_spec::acts;

/// A command that reads nothing: the params may be an empty object.
pub(crate) const NONE: &[(&str, &str)] = &[];

/// The pane in focus, or the one a script names.
pub(crate) const PANE: &[(&str, &str)] = &[("pane", "int?")];

/// Which way a split's seam runs.
pub(crate) const AXIS: &[(&str, &str)] = &[("axis", "horizontal|vertical")];

/// Two panes: the one in focus and the one it trades with.
pub(crate) const PAIR: &[(&str, &str)] = &[("pane", "int?"), ("other", "int?")];

/// A lifted pane's landing: the pane, its edge and the room it lands in.
pub(crate) const MOVE: &[(&str, &str)] = &[
    ("pane", "int?"),
    ("other", "int?"),
    ("edge", "left|right|top|bottom"),
    ("area", "x,y,w,h"),
];

/// A divider drag: the seam's path and how far along it the drag is.
pub(crate) const RATIO: &[(&str, &str)] = &[("path", "bools"), ("ratio", "real")];

/// How many pages a pane shows.
pub(crate) const MODE: &[(&str, &str)] = &[("mode", "single|spread|vertical|horizontal")];

/// A zoom, by its step or by what it fits.
pub(crate) const ZOOM: &[(&str, &str)] = &[("zoom", "in|out|fit-width|fit-page")];

/// Which way an arrow key goes.
pub(crate) const ARROW: &[(&str, &str)] = &[("arrow", "left|right|up|down")];

/// Which way a page turns, or a focus moves.
pub(crate) const FORWARD: &[(&str, &str)] = &[("forward", "bool")];

/// A pane and the page number it shows.
pub(crate) const PANE_PAGE: &[(&str, &str)] = &[("pane", "int?"), ("page", "int")];

/// A pane and which way its page turns.
pub(crate) const PANE_FORWARD: &[(&str, &str)] = &[("pane", "int?"), ("forward", "bool")];

/// A pane and the pages a list is made from.
pub(crate) const PANE_PAGES: &[(&str, &str)] = &[("pane", "int?"), ("pages", "ints?")];

/// A pane and a text query to search its document for.
pub(crate) const PANE_QUERY: &[(&str, &str)] = &[("pane", "int?"), ("query", "str")];

/// A read-aloud player control and its optional argument.
pub(crate) const READ_ACT: &[(&str, &str)] = &[
    ("action", acts::READ_ACT),
    ("read", "bool?"),
    ("share", "real?"),
    ("card", "bool?"),
];

/// A file to show: by path, or as bytes beside the name they carry.
pub(crate) const OPEN: &[(&str, &str)] = &[
    ("path", "str?"),
    ("bytes", "ints?"),
    ("name", "str?"),
    ("pane", "int?"),
    ("placement", "into|beside|left|right|top|bottom?"),
    ("page", "int?"),
    ("pages", "int?"),
    ("place", "block,offset?"),
    ("spine", "href,item,block,offset?"),
    ("book", "int?"),
];

/// A library book, by the number the library gives it.
pub(crate) const BOOK: &[(&str, &str)] = &[("book", "int")];

/// A book placed from the rail: where it goes and how it lands.
pub(crate) const PLACE_BOOK: &[(&str, &str)] = &[
    ("book", "int"),
    ("pane", "int?"),
    ("placement", "into|beside|left|right|top|bottom?"),
];

/// The books and paths whose saved place goes.
pub(crate) const FORGET: &[(&str, &str)] = &[("books", "ints?"), ("paths", "paths?")];

/// A pen over the pane's selection, in a colour if one is named.
pub(crate) const HIGHLIGHT: &[(&str, &str)] = &[
    ("pane", "int?"),
    ("pen", "full|half|underline?"),
    ("color", "r,g,b?"),
];

/// The first of some text on a page, as a drag selects it.
pub(crate) const SELECT: &[(&str, &str)] = &[("pane", "int?"), ("page", "int"), ("text", "str")];

/// A word a card says.
pub(crate) const WORD: &[(&str, &str)] = &[("word", "str")];

/// A word, and a voice other than the one picked.
pub(crate) const WORD_VOICE: &[(&str, &str)] = &[("word", "str"), ("voice", "str")];

/// Where a reading starts, where it is and where it stops.
pub(crate) const READ_FROM: &[(&str, &str)] = &[
    ("pane", "int?"),
    ("from", "block,offset?"),
    ("at", "page,char?"),
    ("until", "page,char?"),
];

/// The spot a reading is pinned to.
pub(crate) const SPOT: &[(&str, &str)] = &[("pane", "int?"), ("at", "page,char")];

/// A voice of the tab's engine.
pub(crate) const VOICE: &[(&str, &str)] = &[("voice", "str")];

/// A model, one of its builds or one of its voices, and the download's ask.
pub(crate) const ITEM: &[(&str, &str)] = &[
    ("model", "str"),
    ("build", "str?"),
    ("voice", "str?"),
    ("act", "start|pause|cancel|remove"),
];

/// A model, and a language one of its voices speaks.
pub(crate) const MODEL_LANGUAGE: &[(&str, &str)] = &[("model", "str"), ("language", "str")];

/// What a download is asked to do.
pub(crate) const DOWNLOAD: &[(&str, &str)] = &[("act", "start|pause|cancel|remove")];

/// The folder a dataset is in.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const FOLDER: &[(&str, &str)] = &[("folder", "path")];

/// A settings tab, or a model's voices, which is a tab of its own.
pub(crate) const TAB: &[(&str, &str)] = &[
    (
        "tab",
        "appearance|layout|read-aloud|highlights|vocabulary|dictionary|ai|models|panes|motion?",
    ),
    ("model", "str?"),
];

/// A whole set of choices.
pub(crate) const SETTINGS: &[(&str, &str)] = &[("settings", "object")];

/// One setting by its dotted path, to any JSON value.
pub(crate) const PATH_VALUE: &[(&str, &str)] = &[("path", "str"), ("value", "any")];

/// A look, in the shape the popover sends.
pub(crate) const LOOK: &[(&str, &str)] = &[("look", "object")];

/// The popover's text dials, each left as it is when absent.
pub(crate) const QUICK_TEXT: &[(&str, &str)] = &[
    ("size_pct", "real?"),
    ("line_height", "real?"),
    ("paragraph_margin", "real?"),
    ("override_font", "bool?"),
    ("override_layout", "bool?"),
];

/// Whether a split takes its own colour and its own texture.
pub(crate) const INDEPENDENCE: &[(&str, &str)] = &[("themes", "bool?"), ("textures", "bool?")];

/// The ink dial, 10 to 100.
pub(crate) const INK: &[(&str, &str)] = &[("ink", "real")];

/// A preset's name, its section when it has one, and the look it keeps.
pub(crate) const PRESET_SAVE: &[(&str, &str)] =
    &[("name", "str"), ("group", "str?"), ("look", "object")];

/// A preset's name.
pub(crate) const PRESET_NAME: &[(&str, &str)] = &[("name", "str")];

/// Whether the sidebar floats over the panes.
pub(crate) const OVERLAY: &[(&str, &str)] = &[("overlay", "bool")];

/// Whether the title bar stays up.
pub(crate) const PINNED: &[(&str, &str)] = &[("pinned", "bool")];

/// Which page fills the window.
pub(crate) const ROUTE: &[(&str, &str)] = &[("route", "reader|library|dictionary")];

/// The library page's layout and order.
pub(crate) const VIEW: &[(&str, &str)] = &[("view", "object")];

/// The Dictionary panel's choices.
pub(crate) const DICTIONARY: &[(&str, &str)] = &[("dictionary", "object")];

/// An export's name, its format and the text that goes in it.
pub(crate) const EXPORT: &[(&str, &str)] = &[
    ("pane", "int?"),
    ("name", "str?"),
    ("format", "markdown|text|json"),
    ("contents", "str?"),
];

/// The page AI's asks, and the pane they are about.
pub(crate) const PAGE_AI: &[(&str, &str)] = &[("action", acts::PAGE_AI), ("pane", "int?")];

/// The study tools' asks.
pub(crate) const STUDY: &[(&str, &str)] = &[
    ("action", acts::STUDY),
    ("pane", "int?"),
    ("mark", "int?"),
    ("text", "str?"),
];

/// The dictionary's asks, and the word one of them goes after.
pub(crate) const DICT: &[(&str, &str)] = &[("action", acts::DICT), ("word", "str?")];

/// The library's asks.
pub(crate) const LIBRARY: &[(&str, &str)] = &[
    ("action", acts::LIBRARY),
    ("root", "path?"),
    ("min_size", "int?"),
    ("in_place", "bool?"),
    ("watch", "bool?"),
    ("groups", "bool?"),
];

/// The AI settings' jobs.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const JOBS: &[(&str, &str)] = &[("action", acts::JOBS)];
