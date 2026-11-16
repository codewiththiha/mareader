//! The names a script spells an enum with, one table each.
//!
//! A table is the whole vocabulary of one field: what `json::one_of` reads,
//! and what a refusal lists when the name a script sent is not in it. The
//! names are the ids' case — dotted and kebab — so a script reads the same
//! words in an id, in a param's kind and in the answer to a mistake.

use mareader_library::Pen;
use mareader_workspace::{Edge, SplitAxis};

use crate::dict::Download;
use crate::documents::Placement;
use crate::export::Format;
use crate::pane::{Arrow, Fit, Step, ViewMode, ZoomCommand};
use crate::route::Route;
use crate::settings::Tab;

/// Which way an arrow key goes.
pub(crate) const ARROW: &[(&str, Arrow)] = &[
    ("left", Arrow::Left),
    ("right", Arrow::Right),
    ("up", Arrow::Up),
    ("down", Arrow::Down),
];

/// How many pages a pane shows at once.
pub(crate) const VIEW_MODE: &[(&str, ViewMode)] = &[
    ("single", ViewMode::Single),
    ("spread", ViewMode::Spread),
    ("vertical", ViewMode::Vertical),
    ("horizontal", ViewMode::Horizontal),
];

/// A zoom, by its step or by what it fits.
pub(crate) const ZOOM: &[(&str, ZoomCommand)] = &[
    ("in", ZoomCommand::Step(Step::In)),
    ("out", ZoomCommand::Step(Step::Out)),
    ("fit-width", ZoomCommand::Fit(Fit::Width)),
    ("fit-page", ZoomCommand::Fit(Fit::Page)),
];

/// Which way a split's seam runs.
pub(crate) const AXIS: &[(&str, SplitAxis)] = &[
    ("horizontal", SplitAxis::Horizontal),
    ("vertical", SplitAxis::Vertical),
];

/// A pane's side, which is where a lifted pane lands.
pub(crate) const EDGE: &[(&str, Edge)] = &[
    ("left", Edge::Left),
    ("right", Edge::Right),
    ("top", Edge::Top),
    ("bottom", Edge::Bottom),
];

/// Where a book goes when a pane takes it.
pub(crate) const PLACEMENT: &[(&str, Placement)] = &[
    ("into", Placement::Into),
    ("beside", Placement::Beside),
    ("left", Placement::Edge(Edge::Left)),
    ("right", Placement::Edge(Edge::Right)),
    ("top", Placement::Edge(Edge::Top)),
    ("bottom", Placement::Edge(Edge::Bottom)),
];

/// A settings window's tab, minus the one a model has to be found for.
pub(crate) const TAB: &[(&str, Tab)] = &[
    ("appearance", Tab::Appearance),
    ("layout", Tab::Layout),
    ("read-aloud", Tab::ReadAloud),
    ("highlights", Tab::Highlights),
    ("vocabulary", Tab::Vocabulary),
    ("dictionary", Tab::Dictionary),
    ("ai", Tab::Ai),
    ("models", Tab::Models),
    ("panes", Tab::Panes),
    ("motion", Tab::Motion),
    ("updates", Tab::Updates),
];

/// What an export is written as.
pub(crate) const FORMAT: &[(&str, Format)] = &[
    ("markdown", Format::Markdown),
    ("text", Format::Text),
    ("json", Format::Json),
];

/// What a download is asked to do.
pub(crate) const DOWNLOAD: &[(&str, Download)] = &[
    ("start", Download::Start),
    ("pause", Download::Pause),
    ("cancel", Download::Cancel),
    ("remove", Download::Remove),
];

/// A pen a highlight is drawn with.
pub(crate) const PEN: &[(&str, Pen)] = &[
    ("full", Pen::Full),
    ("half", Pen::Half),
    ("underline", Pen::Underline),
];

/// Which page fills the window.
pub(crate) const ROUTE: &[(&str, Route)] = &[
    ("reader", Route::Reader),
    ("library", Route::Library),
    ("dictionary", Route::Dictionary),
];
