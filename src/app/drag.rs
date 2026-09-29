//! A document drag the Shell carries from the shelf to the reader.
//!
//! The shelf hands a held book over (`BeginDocumentDrag`) the moment the
//! pointer enters its "Open in Reader" zone. From there the drag is the
//! Shell's: it owns the descriptor (data only — a row id, an address, a
//! label), the pointer and the drag's state. It reveals the reader (the
//! shelf goes off screen, its session kept intact for as long as the drag
//! lives) and relays the pointer, in the reader frame's coordinates, to the
//! reader's own drag session — which measures its own workspace, offers its
//! own targets and runs its own workspace command. Neither runtime touches
//! the other, and neither holds anything of the other.
//!
//! The pointer reaches the Shell two ways, both feeding this one session:
//! the shelf frame forwards what it still receives (the press began there,
//! so the browser keeps routing the pointer to it — the usual case), and a
//! shield over the page catches it when the browser does not. The first
//! release or cancellation ends the drag; anything after it is ignored.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use runtime_contract::boundary::{DocumentDragDescriptor, LaunchDocument};
use runtime_contract::protocol::{DocumentDragEvent, DragPointerPhase, ShellFrame};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::app::frame::Driver;
use crate::app::manager::RuntimeManager;

/// What a frame said about a carried drag.
pub enum DragReport {
    /// The shelf handed a held book over, at `(x, y)` in its own client
    /// coordinates.
    Begin {
        source: Box<DocumentDragDescriptor>,
        x: f64,
        y: f64,
    },
    /// The pointer of the drag, as the shelf frame still receives it.
    Pointer {
        x: f64,
        y: f64,
        phase: DragPointerPhase,
    },
}

/// How a drag ended.
#[derive(Clone, Copy, Debug)]
enum Ending {
    /// Let go at a page position: a drop, if the reader shows a target there.
    Drop((f64, f64)),
    Cancel,
}

/// The drag in flight. Nothing of either runtime: the descriptor, two frame
/// generations, the last pointer position and the Shell's own shield.
struct Carried {
    /// The shelf frame the drag came from: its forwarded pointer is the
    /// drag's, and its recycle is held while the drag lives.
    library: u64,
    source: DocumentDragDescriptor,
    /// The reader frame the drag is relayed to, once it is on screen and has
    /// been told the drag began.
    reader: Option<u64>,
    /// The pointer, in the page's client coordinates.
    last: (f64, f64),
    /// A release or cancellation that came before the reader was on screen:
    /// carried out the moment it is.
    ending: Option<Ending>,
    shield: Option<Shield>,
}

thread_local! {
    /// The one carried drag. The page thread is the only thread.
    static CARRIED: RefCell<Option<Carried>> = const { RefCell::new(None) };
}

/// A frame's report, generation-checked at the port already.
pub fn on_frame(manager: &Arc<RuntimeManager>, generation: u64, report: DragReport) {
    match report {
        DragReport::Begin { source, x, y } => begin(manager, generation, *source, (x, y)),
        DragReport::Pointer { x, y, phase } => {
            let from_the_shelf = CARRIED.with(|c| {
                c.borrow()
                    .as_ref()
                    .is_some_and(|carried| carried.library == generation)
            });
            if from_the_shelf {
                pointer(manager, to_page(generation, (x, y)), phase);
            }
        }
    }
}

/// The shelf on screen handed a book over: take the drag, and reveal the
/// reader it is carried to.
fn begin(
    manager: &Arc<RuntimeManager>,
    generation: u64,
    source: DocumentDragDescriptor,
    at: (f64, f64),
) {
    if manager.active_library() != Some(generation) {
        return;
    }
    if CARRIED.with(|c| c.borrow().is_some()) {
        return;
    }
    let last = to_page(generation, at);
    let shield = Shield::install(manager);
    CARRIED.with(|c| {
        *c.borrow_mut() = Some(Carried {
            library: generation,
            source,
            reader: None,
            last,
            ending: None,
            shield,
        });
    });
    let manager = manager.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let reader = manager.reveal_reader_for_drag().await;
        revealed(&manager, reader);
    });
}

/// The reader is on screen (or could not be put there): tell it the drag
/// began, where the pointer is now, and carry out an ending that came first.
fn revealed(manager: &Arc<RuntimeManager>, reader: Option<Rc<Driver>>) {
    let Some(reader) = reader else {
        end(manager, Ending::Cancel);
        return;
    };
    let step = CARRIED.with(|c| {
        let mut c = c.borrow_mut();
        let carried = c.as_mut()?;
        carried.reader = Some(reader.generation());
        Some((
            carried.library,
            carried.source.clone(),
            carried.last,
            carried.ending.take(),
        ))
    });
    let Some((library, source, last, ending)) = step else {
        return;
    };
    manager.hold_recycle(library);
    let (x, y) = to_frame(&reader, last);
    reader.send(&ShellFrame::DocumentDrag {
        event: DocumentDragEvent::Begin {
            source: Box::new(source),
            x,
            y,
        },
    });
    if let Some(ending) = ending {
        end(manager, ending);
    }
}

/// One pointer step, in page coordinates.
fn pointer(manager: &Arc<RuntimeManager>, at: (f64, f64), phase: DragPointerPhase) {
    enum Step {
        Over(u64),
        End(Ending),
    }
    let step = CARRIED.with(|c| {
        let mut c = c.borrow_mut();
        let carried = c.as_mut()?;
        if carried.ending.is_some() {
            // Already let go; the reader has not come on screen yet.
            return None;
        }
        let ending = match phase {
            DragPointerPhase::Move => {
                carried.last = at;
                return carried.reader.map(Step::Over);
            }
            DragPointerPhase::Release => {
                carried.last = at;
                Ending::Drop(at)
            }
            DragPointerPhase::Cancel => Ending::Cancel,
        };
        if carried.reader.is_some() {
            Some(Step::End(ending))
        } else {
            carried.ending = Some(ending);
            None
        }
    });
    match step {
        Some(Step::Over(generation)) => {
            if let Some(reader) = crate::app::frame::lookup(generation) {
                let (x, y) = to_frame(&reader, at);
                reader.send(&ShellFrame::DocumentDrag {
                    event: DocumentDragEvent::Over { x, y },
                });
            }
        }
        Some(Step::End(ending)) => end(manager, ending),
        None => {}
    }
}

/// The drag is over. A drop hands the reader the launch the store resolves
/// for the book (the same resolution an open from the shelf gets), and the
/// reader decides what it means — a pane at its target, or nothing. The
/// shelf's held recycle runs its course again either way.
fn end(manager: &Arc<RuntimeManager>, ending: Ending) {
    let Some(carried) = CARRIED.with(|c| c.borrow_mut().take()) else {
        return;
    };
    if let Some(shield) = carried.shield {
        shield.remove();
    }
    if let Some(reader) = carried.reader.and_then(crate::app::frame::lookup) {
        match ending {
            Ending::Drop(at) => {
                let launch = crate::services::resolve_launch(&carried.source.path)
                    .unwrap_or_else(|| launch_of(&carried.source));
                let (x, y) = to_frame(&reader, at);
                reader.send(&ShellFrame::DocumentDrag {
                    event: DocumentDragEvent::Drop {
                        x,
                        y,
                        launch: Box::new(launch),
                    },
                });
                manager.note_drag_launch(reader.generation());
            }
            Ending::Cancel => reader.send(&ShellFrame::DocumentDrag {
                event: DocumentDragEvent::Cancel,
            }),
        }
    }
    manager.resume_recycle(carried.library);
}

/// A launch from the descriptor alone, for a book the store has no row for
/// any more: from its first page.
fn launch_of(source: &DocumentDragDescriptor) -> LaunchDocument {
    LaunchDocument {
        book_id: source.book_id.clone(),
        path: source.path.clone(),
        resume_page: 1,
        saved_fraction: None,
        blend_override: false,
        cover_data_url: None,
        display_name: Some(source.label.clone()),
    }
}

/// A position in frame `generation`'s document, in the page's coordinates.
fn to_page(generation: u64, at: (f64, f64)) -> (f64, f64) {
    let (left, top) = crate::app::frame::lookup(generation)
        .map(|driver| driver.client_origin())
        .unwrap_or_default();
    (at.0 + left, at.1 + top)
}

/// A page position in `reader`'s document coordinates.
fn to_frame(reader: &Driver, at: (f64, f64)) -> (f64, f64) {
    let (left, top) = reader.client_origin();
    (at.0 - left, at.1 - top)
}

/// The page-wide layer that catches the drag's pointer when the browser does
/// not keep routing it to the shelf frame, and keeps the revealed reader's
/// own controls from seeing a pointer that belongs to the drag. Its pointer
/// listeners live on the element (handed over, gone with it); the one window
/// listener (Escape, while the Shell's page has focus) is removed with it.
struct Shield {
    element: web_sys::Element,
    on_key: Closure<dyn Fn(web_sys::KeyboardEvent)>,
}

impl Shield {
    fn install(manager: &Arc<RuntimeManager>) -> Option<Self> {
        let window = web_sys::window()?;
        let document = window.document()?;
        let element = document.create_element("div").ok()?;
        element.set_class_name("drag-shield");
        let _ = element.set_attribute("data-drag-shield", "");
        let _ = element.set_attribute("aria-hidden", "true");
        for (name, phase) in [
            ("pointermove", DragPointerPhase::Move),
            ("pointerup", DragPointerPhase::Release),
            ("pointercancel", DragPointerPhase::Cancel),
        ] {
            let manager = manager.clone();
            let listener = Closure::<dyn Fn(web_sys::Event)>::new(move |event: web_sys::Event| {
                let at = event
                    .dyn_ref::<web_sys::MouseEvent>()
                    .map(|m| (f64::from(m.client_x()), f64::from(m.client_y())))
                    .unwrap_or_default();
                pointer(&manager, at, phase);
            })
            .into_js_value();
            let _ = element.add_event_listener_with_callback(name, listener.unchecked_ref());
        }
        document.body()?.append_child(&element).ok()?;
        let keys = manager.clone();
        let on_key =
            Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |event: web_sys::KeyboardEvent| {
                if event.key() == "Escape" {
                    pointer(&keys, (0.0, 0.0), DragPointerPhase::Cancel);
                }
            });
        let _ = window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        Some(Self { element, on_key })
    }

    fn remove(self) {
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback(
                "keydown",
                self.on_key.as_ref().unchecked_ref(),
            );
        }
        self.element.remove();
    }
}
