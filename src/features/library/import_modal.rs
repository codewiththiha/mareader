//! The import sheet: what a folder import is allowed to be.
//!
//! Five answers, and each one is a `library_core::folder::FolderOpts` field — the
//! sheet writes the options and the shell's walk reads them, so there is no
//! second copy of "what does larger than 30 KB mean" anywhere in the app.
//!
//! The two switches are nested on purpose. "Watch for new books" only means
//! something for a folder the library reads in place: a store copy is the app's
//! own file from the moment it lands, and rescanning the source afterwards would
//! be a second opinion about a book that already exists. The row hides rather
//! than disables, because a switch that cannot be turned on is noise.
//!
//! The watch switch is the reader's on every ground, and what changes is which
//! rung their answer lands on. Tracking is a tree rather than one flag for the
//! whole import (`library_core::tracking`), so ground an existing read-at-place
//! tree already covers is a rung of THAT tree: the sheet opens with the tree's
//! own answer for the rung already on the switch, and the click writes back to
//! that rung, which is what lets a subfolder be tracked differently from the
//! import it stands inside. Ground nothing covers is a fresh folder's root, and
//! the switch writes the options the sheet already holds. Either way the switch
//! is the sheet's own signal — what it shows is what lands, and a covered
//! ground seeds the signal rather than owning the display, because a switch
//! that reads one place and writes another is a switch that cannot be clicked.
//!
//! This used to be a lock. Ground a watched tree was seated on got a disabled
//! switch and a sentence telling the reader to go and right-click the shelf
//! instead, because there was no unit smaller than the whole imported tree to
//! hang a decision on — so an import of a subfolder under a watched ancestor
//! could not say anything about that subfolder, and a sheet whose defaults were
//! `false` would have quietly un-tracked the tree it was importing. The lock was
//! the only honest answer a single flag could give. With a rung to write, the
//! import is the reader's ask again: turning the switch off on covered ground is
//! an explicit `Off` at that rung, and the tree above keeps watching its own.
//!
//! The sheet still hides the row entirely for a folder imported as copies,
//! because a store copy is the app's own file from the moment it lands and
//! rescanning the source afterwards would be a second opinion about a book that
//! already exists. Hiding is the honest answer there — the mode has no tracking
//! question — where disabling never was. A copies run therefore lands no
//! tracking answer at all: an `Off` written to a standing tree's rung by a mode
//! that never showed the switch would be a decision the reader was never asked.
//!
//! Turning tracking off by hand from the shelf's own right-click is
//! `crate::services::library::set_shelf_watch`, and it answers for the SEAT the
//! shelf stands on: a rung's shelf turns that rung, the root shelf turns the
//! whole tree, and a shelf a hand made inside a tree turns the closest rung the
//! disk named for it.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use library_core::folder::{FolderOpts, MIN_SIZE_CEIL, MIN_SIZE_FLOOR};
use library_core::scan::selectable_formats;
use reader_core::format::Format;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::controls::option_button::OptionButton;
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter};
use crate::components::primitives::form::row::Row;
use crate::services::library::{ground_tracking, import_folder, pick_folder};
use crate::state::AppState;

/// The sheet's two handles, provided by the library page: whether it is open and
/// the folder it opens onto.
///
/// A context rather than props because three surfaces can open it — the shelf's
/// `+` card, the empty state's button and a folder dropped on the window — and
/// threading two signals through all of them would put the sheet's plumbing in
/// every component between.
#[derive(Clone, Copy)]
pub(crate) struct ImportSheet {
    pub open: RwSignal<bool>,
    pub root: RwSignal<Option<String>>,
    toasts: RwSignal<Option<String>>,
}

impl ImportSheet {
    /// Create and provide the handles. Called once, by the page.
    pub fn provide() -> Self {
        let sheet = Self {
            open: RwSignal::new(false),
            root: RwSignal::new(None),
            toasts: RwSignal::new(None),
        };
        provide_context(sheet);
        sheet
    }

    /// Open the sheet, onto `root` when the caller already has one.
    pub fn open_on(&self, root: Option<String>) {
        if let Some(root) = root {
            self.root.set(Some(root));
        }
        self.open.set(true);
    }

    /// Report a failure to open the sheet — a picker that could not run, on a
    /// surface that has no toast host of its own.
    pub fn toast(&self, message: String) {
        self.toasts.set(Some(message));
    }
}

/// Raise anything the sheet collected while it was closed. Called by the page,
/// which owns the app's one toast slot.
pub(crate) fn drain_sheet_toasts(state: AppState, sheet: ImportSheet) {
    if let Some(message) = sheet.toasts.get_untracked() {
        sheet.toasts.set(None);
        crate::services::library::toast(state, message);
    }
}

#[component]
pub(crate) fn ImportModal(state: AppState, sheet: ImportSheet) -> impl IntoView {
    // The options outlive the sheet being open: a reader who imports a second
    // folder usually wants it imported the same way as the first. The lane
    // arbitration and the Escape rule are the modal shell's (see
    // `crate::components::primitives::overlay::modal_shell`).
    let opts = RwSignal::new(FolderOpts::default());

    let in_place = Signal::derive(move || opts.with(|o| o.in_place));
    let watching = Signal::derive(move || opts.with(|o| o.watch));
    // The tree and rung a watch answer belongs to, when an existing read-at-place
    // tree already covers this ground. Read reactively, so a folder whose tracking
    // the reader turned off from its own menu in between — or whose shelf another
    // surface took apart while this sheet was open — answers on the frame it
    // happens rather than at the next open onto it.
    let ground = Signal::derive(move || {
        sheet
            .open
            .get()
            .then(|| sheet.root.get())
            .flatten()
            .and_then(|root| ground_tracking(state, &root))
    });
    // The switch is the sheet's own on EVERY ground, and what it shows —
    // `opts.watch` — is the value that lands. Ground an existing tree covers
    // SEEDS the option rather than owning the display: an effect reads the
    // tree's own answer for the rung into the options when the sheet opens
    // onto that ground, so the switch starts at the state the tree is in and a
    // click moves it. The display used to read the tree and the click to write
    // the options, which is a switch that shows one value and stores another:
    // on covered ground it could be clicked all day without the knob moving.
    Effect::new(move |_| {
        if !sheet.open.get() {
            return;
        }
        let Some(root) = sheet.root.get() else {
            return;
        };
        // The lists under `ground_tracking` are read untracked, so the effect
        // re-runs on an open and on a changed folder and on nothing else: a
        // seed that re-fired on every folder write would undo the toggle the
        // reader just made with the state it is toggling.
        if let Some((_, _, on)) = ground_tracking(state, &root) {
            opts.update(|o| o.watch = on);
        }
    });
    let include = Signal::derive(move || opts.with(|o| o.include_selected));
    let grouped = Signal::derive(move || opts.with(|o| o.groups));
    let min_size = Signal::derive(move || opts.with(|o| o.min_size));
    let label = Signal::derive(move || opts.with(|o| o.min_size_label()));
    // The floor is zero, so "at the floor" and "is zero" are one question.
    let at_floor = Signal::derive(move || min_size.get() == MIN_SIZE_FLOOR);
    let at_ceil = Signal::derive(move || min_size.get() >= MIN_SIZE_CEIL);
    let chosen = Signal::derive(move || sheet.root.with(|r| r.is_some()));
    // The path row truncates to one line, and a deep folder is exactly the
    // path a reader needs to READ before trusting the import with it — so
    // the truncation has an adjuster: one control that unfolds the row to the
    // whole path, wrapped, and folds it back.
    let path_open = RwSignal::new(false);

    view! {
        <ModalShell
            open=sheet.open
            aria_label="Import a folder"
            width="min(92vw, 480px)"
        >
                    <header class="flex shrink-0 items-center gap-2 px-4 pb-2 pt-4">
                        <h2 class="text-sm font-semibold text-ink">"Import books"</h2>
                        <div class="ml-auto">
                            <IconButton
                                icon=IconName::Close
                                title="Close"
                                class="rounded-full bg-line/60 hover:bg-line".to_string()
                                on_click=move || sheet.open.set(false)
                            />
                        </div>
                    </header>

                    // The heading is this sheet's own — a sentence with no
                    // subtitle and a different rhythm from the six that ask a
                    // question. The body and the button row are everybody's.
                    <SheetBody>
                        // --- the folder ------------------------------------
                        <SectionLabel text="Folder" />
                        <div class="mb-4 flex items-start gap-2 rounded-xl border border-line px-3 py-2.5">
                            <Icon name=IconName::Open size=16 class="mt-0.5 shrink-0 text-muted" />
                            <span
                                class=move || {
                                    if path_open.get() {
                                        "min-w-0 flex-1 break-all text-sm text-ink"
                                    } else {
                                        "min-w-0 flex-1 truncate text-sm text-ink"
                                    }
                                }
                                title=move || sheet.root.get().unwrap_or_default()
                            >
                                {move || {
                                    sheet
                                        .root
                                        .get()
                                        .unwrap_or_else(|| "Choose a folder…".to_string())
                                }}
                            </span>
                            {move || {
                                chosen.get().then(|| {
                                    view! {
                                        <IconButton
                                            icon=if path_open.get() {
                                                IconName::ChevronUp
                                            } else {
                                                IconName::ChevronDown
                                            }
                                            title=if path_open.get() {
                                                "Show one line"
                                            } else {
                                                "Show the full path"
                                            }
                                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                                            on_click=move || path_open.set(!path_open.get())
                                        />
                                    }
                                })
                            }}
                            <Button
                                on_click=move |_| {
                                    wasm_bindgen_futures::spawn_local(async move {
                                        match pick_folder().await {
                                            Ok(Some(root)) => sheet.root.set(Some(root)),
                                            Ok(None) => {}
                                            Err(message) => sheet.toast(message),
                                        }
                                    });
                                }
                                variant=ButtonVariant::Ghost
                                compact=true
                                title="Choose another folder"
                            >
                                <span>"Change…"</span>
                            </Button>
                        </div>

                        // --- formats ---------------------------------------
                        <SectionLabel text="Formats" />
                        <div class="mb-2 flex gap-1.5">
                            <OptionButton
                                selected=Signal::derive(move || include.get())
                                on_click=move || opts.update(|o| o.include_selected = true)
                                variant_class="flex-1 px-2 py-1.5 text-xs"
                            >
                                <span>"Include selected"</span>
                            </OptionButton>
                            <OptionButton
                                selected=Signal::derive(move || !include.get())
                                on_click=move || opts.update(|o| o.include_selected = false)
                                variant_class="flex-1 px-2 py-1.5 text-xs"
                            >
                                <span>"Exclude selected"</span>
                            </OptionButton>
                        </div>
                        <div class="mb-4 grid grid-cols-2 gap-1.5">
                            {selectable_formats()
                                .into_iter()
                                .map(|format| {
                                    view! {
                                        <FormatRow opts=opts format=format />
                                    }
                                })
                                .collect_view()}
                        </div>

                        // --- size ------------------------------------------
                        <SectionLabel text="File size larger than" />
                        <div class="mb-4 flex items-center justify-between gap-3 rounded-xl border border-line px-3 py-2">
                            <span class="rounded-md bg-line/60 px-2 py-0.5 text-xs tabular-nums text-ink">
                                {move || label.get()}
                            </span>
                            <div class="flex items-center gap-1">
                                <IconButton
                                    icon=IconName::Minus
                                    size=14
                                    title="Smaller files count too"
                                    class="rounded-full bg-line/60 hover:bg-line".to_string()
                                    disabled=at_floor
                                    on_click=move || opts.update(|o| o.step_min_size(-1))
                                />
                                <IconButton
                                    icon=IconName::Plus
                                    size=14
                                    title="Only larger files"
                                    class="rounded-full bg-line/60 hover:bg-line".to_string()
                                    disabled=at_ceil
                                    on_click=move || opts.update(|o| o.step_min_size(1))
                                />
                            </div>
                        </div>

                        // --- how the books are held -------------------------
                        <SectionLabel text="Books" />
                        <div class="divide-y divide-line rounded-xl border border-line">
                            <Row label="Read at place">
                                <Switch
                                    checked=in_place
                                    on_change=Callback::new(move |on| {
                                        // The ground's own answer, read before the write: a
                                        // mode turned back on restores the state the tree is
                                        // in rather than the `false` the mode-off took with it.
                                        let ground_on =
                                            ground.get_untracked().map(|(_, _, gon)| gon);
                                        opts.update(|o| {
                                            o.in_place = on;
                                            // Watching a copy is a question the
                                            // sheet does not ask, so turning the
                                            // mode off takes the answer with it —
                                            // the row hides, and a hidden switch
                                            // that remembered an answer would
                                            // land one nobody can see.
                                            if !on {
                                                o.watch = false;
                                            } else if let Some(gon) = ground_on {
                                                o.watch = gon;
                                            }
                                        });
                                    })
                                    title="Read the books from their own folders, without copying".to_string()
                                />
                            </Row>
                            <Show when=move || in_place.get() fallback=|| ()>
                                <div class="px-4 py-3.5 pl-8">
                                    <div class="flex items-center justify-between gap-3">
                                        <div class="min-w-0">
                                            <span class="block text-sm text-ink">
                                                "Watch for new books"
                                            </span>
                                            <span class="mt-0.5 block text-xs text-muted">
                                                {move || {
                                                    // A subfolder of a tree the
                                                    // library already reads says
                                                    // which rung the answer is
                                                    // about, because the switch is
                                                    // the reader's here rather
                                                    // than a lock: turning it off
                                                    // is a decision at that rung
                                                    // and the tree above keeps
                                                    // watching its own.
                                                    if ground.get().is_some() {
                                                        "This folder is part of a tree the library \
                                                         already reads. The switch answers for this \
                                                         subfolder alone."
                                                            .to_string()
                                                    } else {
                                                        "Checks for new books when the app opens or \
                                                         you come back to it."
                                                            .to_string()
                                                    }
                                                }}
                                            </span>
                                        </div>
                                        // Rebuilt rather than reactive inside:
                                        // the row's title is a `String` prop, and
                                        // a ground that changed is a switch with a
                                        // different sentence on it.
                                        {move || {
                                            let title = if ground.get().is_some() {
                                                "Watch this subfolder for new books"
                                            } else {
                                                "Watch for new books"
                                            };
                                            view! {
                                                <Switch
                                                    checked=watching
                                                    on_change=Callback::new(move |on| {
                                                        opts.update(|o| o.watch = on);
                                                    })
                                                    title=title.to_string()
                                                />
                                            }
                                        }}
                                    </div>
                                </div>
                            </Show>
                            <div class="px-4 py-3.5">
                                <span class="mb-2 block text-sm text-ink">"Folder structure"</span>
                                <div class="flex flex-col gap-1.5">
                                    <OptionButton
                                        selected=Signal::derive(move || grouped.get())
                                        on_click=move || opts.update(|o| o.groups = true)
                                        variant_class="flex items-center gap-2 px-2.5 py-1.5 text-xs"
                                    >
                                        <Dot on=Signal::derive(move || grouped.get()) />
                                        <span>"A shelf for each folder"</span>
                                    </OptionButton>
                                    <OptionButton
                                        selected=Signal::derive(move || !grouped.get())
                                        on_click=move || opts.update(|o| o.groups = false)
                                        variant_class="flex items-center gap-2 px-2.5 py-1.5 text-xs"
                                    >
                                        <Dot on=Signal::derive(move || !grouped.get()) />
                                        <span>"One shelf for everything"</span>
                                    </OptionButton>
                                </div>
                            </div>
                        </div>

                        <p class="mt-3 text-xs text-muted">
                            {move || {
                                if in_place.get() {
                                    "Books stay where they are — the library just remembers \
                                     where they live."
                                        .to_string()
                                } else {
                                    "Books are copied into the app's own files, so they keep \
                                     working even if the folder moves or is deleted."
                                        .to_string()
                                }
                            }}
                        </p>
                    </SheetBody>

                    <SheetFooter>
                        <Button
                            on_click=move |_| sheet.open.set(false)
                            variant=ButtonVariant::Ghost
                            title="Close without importing"
                        >
                            <span>"Cancel"</span>
                        </Button>
                        <Button
                            on_click=move |_| {
                                let (Some(root), mut options) = (
                                    sheet.root.get_untracked(),
                                    opts.get_untracked(),
                                ) else {
                                    return;
                                };
                                // The value that lands is the value the switch
                                // showed, and the switch is the options signal on
                                // every ground — so one read carries both halves of
                                // the answer. Ground an existing tree covers sends
                                // the decision to that tree at the rung the pick
                                // names; ground nothing covers sends it to a fresh
                                // folder's root through `options.watch` itself.
                                let track = options.in_place.then(|| {
                                    ground.get_untracked().map(|(tree, rung, _)| {
                                        (tree, rung, options.watch)
                                    })
                                }).flatten();
                                sheet.open.set(false);
                                import_folder(state, root, options, track);
                            }
                            variant=ButtonVariant::Primary
                            disabled=Signal::derive(move || !chosen.get())
                            title="Import this folder"
                        >
                            <Icon name=IconName::Drop size=16 />
                            <span>"Import"</span>
                        </Button>
                    </SheetFooter>
        </ModalShell>
    }
}

/// One format's checkbox row. Its own component because the rows are built from
/// the registry rather than typed out, and a closure over a loop variable would
/// have to clone the signal handle per row anyway.
#[component]
fn FormatRow(opts: RwSignal<FolderOpts>, format: Format) -> impl IntoView {
    let on = Signal::derive(move || opts.with(|o| o.formats.contains(&format)));
    view! {
        <OptionButton
            selected=on
            on_click=move || {
                opts.update(|o| {
                    if o.formats.contains(&format) {
                        o.formats.remove(&format);
                    } else {
                        o.formats.insert(format);
                    }
                });
            }
            variant_class="flex items-center gap-2 px-2.5 py-1.5 text-xs"
            title=format.label().to_string()
        >
            <Dot on=on />
            <span>{format.label()}</span>
        </OptionButton>
    }
}

/// The radio dot inside an option row: filled when the row is the chosen one.
#[component]
fn Dot(on: Signal<bool>) -> impl IntoView {
    view! {
        <span
            class=move || {
                let base = "flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-full border";
                if on.get() {
                    format!("{base} border-accent")
                } else {
                    format!("{base} border-line")
                }
            }
        >
            {move || {
                on.get().then(|| {
                    view! { <span class="h-1.5 w-1.5 rounded-full bg-accent"></span> }
                })
            }}
        </span>
    }
}
