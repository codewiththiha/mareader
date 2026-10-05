//! Reader close, through the host's real cascade: `PaneManager::dispose_all`
//! reaches every pane's dispose, and every pane's dispose ends its document
//! session with the same `PaneHandle::end_document` the production
//! [`crate::pane::document::DocumentPane`] runs — so no session of any
//! format outlives the host.
//!
//! Lives on the pane side: the host may not name panes, sessions or
//! engines (tools/check-host-boundary.mjs), but a pane may use the host's
//! contract.

use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use leptos::prelude::*;
use pdf_engine::PdfSession;

use crate::host::contract::{
    PaneAppearance, PaneCommand, PaneDocStatus, PaneEnv, PaneFactory, PaneResourceCounts,
    PaneRuntime, PaneSite, PaneSurface, PaneTeardown,
};
use crate::host::manager::PaneManager;
use crate::host::model::{
    DocumentId, DocumentRef, PaneBounds, PaneDescriptor, PaneError, PaneFormat, PaneId, PaneRequest,
};
use crate::pane::handle::PaneHandle;
use crate::pane::session::{FormatSession, MdSession, TxtSession};
use crate::state::document::reflow::ReflowContent;
use runtime_contract::boundary::LaunchDocument;

/// A pane that owns nothing but a real pane handle, disposed the way the
/// production pane disposes its document.
struct SessionPane {
    id: PaneId,
    handle: PaneHandle,
}

impl PaneRuntime for SessionPane {
    fn id(&self) -> PaneId {
        self.id
    }
    fn format(&self) -> PaneFormat {
        PaneFormat::Pdf
    }
    fn document(&self) -> Option<DocumentId> {
        None
    }
    fn lifecycle_changed(&self, lifecycle: crate::host::model::PaneLifecycle) {
        self.handle.publish_lifecycle(lifecycle);
    }
    fn surface(&self) -> PaneSurface {
        PaneSurface {
            status: Signal::stored(PaneDocStatus::Ready),
            error: Signal::stored(None),
            page: Signal::stored(1),
            reflowable: Signal::stored(false),
            search_visible: Signal::stored(false),
            name: Signal::stored(String::new()),
        }
    }
    fn mount(&self, _bounds: PaneBounds, _site: PaneSite) -> AnyView {
        ().into_any()
    }
    fn resize(&self, _bounds: PaneBounds) {}
    fn focus(&self) {}
    fn blur(&self) {}
    fn appearance(&self, _appearance: PaneAppearance) {}
    fn command(&self, _command: PaneCommand) -> Result<(), PaneError> {
        Ok(())
    }
    fn resources(&self) -> PaneResourceCounts {
        PaneResourceCounts {
            virtualizers: 0,
            document_session: self.handle.holds_document_session(),
            zoom: 1.0,
        }
    }
    fn dispose(&self) -> PaneTeardown {
        let (_, teardown) = self.handle.end_document();
        let handle = self.handle;
        Box::pin(async move {
            teardown.settled().await;
            handle.release();
        })
    }
}

fn env(id: PaneId) -> PaneEnv {
    let settings = RwSignal::new(reader_core::settings::Settings::default());
    let ui = app_state::UiState {
        sidebar: RwSignal::new(app_state::SidebarMode::None),
        toast: RwSignal::new(None),
        window_maximized: RwSignal::new(false),
    };
    PaneEnv {
        runtime: crate::runtime::ReaderRuntime::new(),
        settings,
        ui,
        api: crate::context::ApiHandle::Standalone,
        session_id: 1,
        chrome: app_state::ChromeState {
            settings,
            ui,
            reader: app_state::ReaderSurface {
                reflowable: Signal::stored(false),
                search_visible: Signal::stored(false),
                sidebar_slide: RwSignal::new(app_state::Motion::default()),
            },
        },
        active: Signal::stored(id.get() == 1),
        settings_open: RwSignal::new(false),
        request_focus: Callback::new(|_| {}),
        open: Callback::new(|_| {}),
        can_split: Signal::stored(false),
        moves: Signal::stored(Default::default()),
        relocate: Callback::new(|_| {}),
        workspace: Signal::stored(Default::default()),
        lift: Callback::new(|_| {}),
    }
}

fn request(path: &str) -> PaneRequest {
    PaneRequest {
        document: DocumentId::from_launch(None, path).map(|document_id| DocumentRef {
            document_id,
            path: path.to_string(),
        }),
        ..PaneRequest::default()
    }
}

type Handles = Rc<std::cell::RefCell<Vec<PaneHandle>>>;

fn manager() -> (PaneManager, Handles) {
    let handles: Handles = Rc::default();
    let built = Rc::clone(&handles);
    let factory: PaneFactory = Rc::new(
        move |e: PaneEnv, d: PaneDescriptor, _: Option<LaunchDocument>| -> Rc<dyn PaneRuntime> {
            let handle = PaneHandle::new(d.pane_id, e.runtime);
            built.borrow_mut().push(handle);
            Rc::new(SessionPane {
                id: d.pane_id,
                handle,
            })
        },
    );
    (PaneManager::new(factory), handles)
}

/// Drive the host's teardown to completion (every tail here is ready at
/// once: no engine is attached on the host).
fn drive(mut tail: PaneTeardown) {
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(tail.as_mut().poll(&mut cx), Poll::Ready(()));
}

/// Reader close: the host's dispose cascades through EVERY live session,
/// whatever its format.
#[test]
fn dispose_all_disposes_every_pane_session() {
    let owner = Owner::new();
    owner.with(|| {
        let (manager, handles) = manager();
        manager.create(request("/a.pdf"), None, env).unwrap();
        manager.create(request("/b.md"), None, env).unwrap();
        manager.create(request("/c.txt"), None, env).unwrap();
        let pdf = PdfSession::create();
        let md = MdSession::new("/b.md", ReflowContent::default());
        let txt = TxtSession::new("/c.txt", ReflowContent::default());
        {
            let handles = handles.borrow();
            handles[0].install_session(FormatSession::Pdf(pdf.clone()));
            handles[1].install_session(FormatSession::Markdown(md.clone()));
            handles[2].install_session(FormatSession::Text(txt.clone()));
            assert!(handles.iter().all(PaneHandle::holds_document_session));
        }
        assert!(pdf.is_live() && md.is_live() && txt.is_live());

        manager.dispose_all();
        drive(manager.take_teardown());

        assert!(!pdf.is_live(), "the PDF pane's session was disposed");
        assert!(!md.is_live(), "the Markdown pane's session was disposed");
        assert!(!txt.is_live(), "the text pane's session was disposed");
        assert!(handles.borrow().iter().all(|h| !h.holds_document_session()));
    });
}

/// Pane close through the host: removing ONE pane disposes its session and
/// leaves the others' live.
#[test]
fn removing_one_pane_disposes_only_its_session() {
    let owner = Owner::new();
    owner.with(|| {
        let (manager, handles) = manager();
        let a = manager.create(request("/a.pdf"), None, env).unwrap();
        manager.create(request("/b.txt"), None, env).unwrap();
        let pdf = PdfSession::create();
        let txt = TxtSession::new("/b.txt", ReflowContent::default());
        handles.borrow()[0].install_session(FormatSession::Pdf(pdf.clone()));
        handles.borrow()[1].install_session(FormatSession::Text(txt.clone()));

        drive(manager.close_now(a, None).unwrap());

        assert!(!pdf.is_live());
        assert!(txt.is_live());
        assert!(handles.borrow()[1].holds_document_session());
    });
}
