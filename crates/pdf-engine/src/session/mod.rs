/
/
!

`
P
d
f
S
e
s
s
i
o
n
`
:

t
h
e

o
n
e

o
w
n
e
r

o
f

a
n

o
p
e
n

P
D
F

d
o
c
u
m
e
n
t
,

k
e
y
e
d

b
y

i
t
s

/
/
!

s
i
d
.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::api::{self, EngineError, EngineStats};
use crate::backdrop::{self, Paper};
use crate::bridge;
use crate::types::{
    CoverResult, OpenResult, OutlineEntry, PageSizeResult, RenderResult, ThumbResult,
};

use pdf_paper::PaperConfig;
use reader_core::search::SearchResponse;

pub(crate) mod search;

pub use search::{SEARCH_PAGE_CONCURRENCY, drop_retained_search};

use search::SearchState;

/// The session's lifecycle. Only `Live` admits work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Live,
    Disposing,
    Disposed,
}

/
/
/

T
h
e

l
a
s
t

s
i
d

m
i
n
t
e
d

i
n

t
h
i
s

r
e
a
l
m
;

m
o
n
o
t
o
n
i
c
.
static NEXT_SID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn mint_sid() -> u32 {
    NEXT_SID.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

struct Inner {
    sid: u32,
    state: Cell<State>,
    /
    /
    /

    T
    h
    e

    e
    n
    g
    i
    n
    e

    a
    c
    c
    e
    p
    t
    e
    d

    t
    h
    e

    s
    i
    d
    ,

    f
    a
    l
    s
    e

    o
    n

    t
    h
    e

    h
    o
    s
    t
    .
    registered: bool,
    paper: RefCell<Paper>,
    search: RefCell<SearchState>,
}

impl Drop for Inner {
    /
    /
    /

    T
    h
    e

    s
    a
    f
    e
    t
    y

    n
    e
    t

    f
    o
    r

    a

    s
    e
    s
    s
    i
    o
    n

    d
    r
    o
    p
    p
    e
    d

    w
    i
    t
    h
    o
    u
    t

    `
    d
    i
    s
    p
    o
    s
    e
    `
    .
    fn drop(&mut self) {
        if self.state.get() != State::Disposed && self.registered && bridge::has_pdf_reader() {
            bridge::release_session_detached(self.sid);
        }
    }
}

/
/
/

A

p
a
g
e
'
s

o
w
n

e
l
e
m
e
n
t
s
,

s
o

t
h
e

e
n
g
i
n
e

p
a
i
n
t
s

i
n
t
o

T
H
E
S
E
.
pub struct PageElements<'a> {
    pub canvas: &'a web_sys::Element,
    pub host: Option<&'a web_sys::Element>,
}

/// An open (or opening) PDF document. See the module docs.
#[derive(Clone)]
pub struct PdfSession {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for PdfSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfSession")
            .field("sid", &self.inner.sid)
            .field("state", &self.inner.state.get())
            .finish()
    }
}

fn no_session() -> EngineError {
    EngineError {
        name: "no_session".to_string(),
        message: "The PDF session was disposed".to_string(),
    }
}

impl PdfSession {
    /
    /
    /

    A

    f
    r
    e
    s
    h

    s
    e
    s
    s
    i
    o
    n

    w
    i
    t
    h

    a

    n
    e
    v
    e
    r
    -
    u
    s
    e
    d

    s
    i
    d
    .
    pub fn create() -> Self {
        let sid = mint_sid();
        let registered = bridge::has_pdf_reader() && bridge::create_session(sid);
        Self {
            inner: Rc::new(Inner {
                sid,
                state: Cell::new(State::Live),
                registered,
                paper: RefCell::new(Paper::default()),
                search: RefCell::new(SearchState::default()),
            }),
        }
    }

    pub fn sid(&self) -> u32 {
        self.inner.sid
    }

    /// Whether the session still admits work. False from the first line of
    /// [`Self::dispose`] on, forever.
    pub fn is_live(&self) -> bool {
        self.inner.state.get() == State::Live
    }

    /// Whether `other` is this very session (not merely one with the same
    /// document).
    pub fn same(&self, other: &PdfSession) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// The engine may be called for this session: live, registered, and the
    /// engine still attached.
    fn engine(&self) -> bool {
        self.is_live() && self.inner.registered && bridge::has_pdf_reader()
    }

    /// As [`Self::engine`], as a `Result` for the async calls that report.
    fn require(&self) -> Result<u32, EngineError> {
        if !self.is_live() {
            return Err(no_session());
        }
        api::require_pdf_reader()?;
        if !self.inner.registered {
            return Err(no_session());
        }
        Ok(self.inner.sid)
    }

    pub(crate) fn with_paper<R>(&self, f: impl FnOnce(&mut Paper) -> R) -> R {
        f(&mut self.inner.paper.borrow_mut())
    }

    pub(crate) fn with_search<R>(&self, f: impl FnOnce(&mut SearchState) -> R) -> R {
        f(&mut self.inner.search.borrow_mut())
    }

    // --- Document -------------------------------------------------------

    /
    /
    /

    O
    p
    e
    n

    `
    p
    a
    t
    h
    `

    i
    n

    t
    h
    i
    s

    s
    e
    s
    s
    i
    o
    n
    ;

    o
    n
    e

    d
    o
    c
    u
    m
    e
    n
    t

    p
    e
    r

    s
    e
    s
    s
    i
    o
    n
    .
    pub async fn open(&self, path: &str) -> Result<OpenResult, EngineError> {
        let sid = self.require()?;
        let value = bridge::open(sid, path).await;
        let open: OpenResult = api::resolve(value, "open")?;
        /
        /

        D
        i
        s
        p
        o
        s
        e
        d

        w
        h
        i
        l
        e

        t
        h
        e

        e
        n
        g
        i
        n
        e

        w
        o
        r
        k
        e
        d
        :

        n
        o
        t
        h
        i
        n
        g

        l
        a
        n
        d
        s

        h
        e
        r
        e
        .
        if !self.is_live() {
            return Err(no_session());
        }
        /
        /

        T
        h
        e

        i
        n
        d
        e
        x

        i
        s

        s
        c
        o
        p
        e
        d

        t
        o

        t
        h
        e

        d
        o
        c
        u
        m
        e
        n
        t
        '
        s

        c
        o
        n
        t
        e
        n
        t

        i
        d
        e
        n
        t
        i
        t
        y

        f
        i
        r
        s
        t
        .
        self.with_search(|s| s.scope(open.fingerprint.as_deref(), path, open.num_pages));
        Ok(open)
    }

    /
    /
    /

    T
    h
    e

    d
    o
    c
    u
    m
    e
    n
    t
    '
    s

    c
    h
    a
    p
    t
    e
    r

    t
    r
    e
    e
    ,

    f
    l
    a
    t
    t
    e
    n
    e
    d

    i
    n
    t
    o

    w
    i
    r
    e

    e
    n
    t
    r
    i
    e
    s
    .
    pub async fn outline(&self) -> Result<Vec<OutlineEntry>, EngineError> {
        if !self.engine() {
            return Ok(Vec::new());
        }
        let value = bridge::resolve_outline(self.inner.sid).await;
        let payload: OutlinePayload = api::resolve(value, "resolveOutline")?;
        Ok(payload.outline)
    }

    /
    /
    /

    P
    a
    g
    e

    1

    o
    f

    `
    p
    a
    t
    h
    `

    a
    s

    a

    s
    m
    a
    l
    l

    J
    P
    E
    G
    ,

    t
    h
    e

    s
    h
    e
    l
    f

    c
    o
    v
    e
    r
    .
    pub async fn cover_data_url(
        &self,
        path: &str,
        max_width: f64,
    ) -> Result<CoverResult, EngineError> {
        let sid = self.require()?;
        let value = bridge::cover_data_url(sid, path, max_width).await;
        api::resolve::<CoverResult>(value, "cover")
    }

    /// Make this session the one whose paper the root backdrop shows.
    pub fn present(&self) {
        if self.engine() {
            bridge::present_session(self.inner.sid);
        }
    }

    // --- Pages ----------------------------------------------------------

    /
    /
    /

    R
    e
    g
    i
    s
    t
    e
    r

    a

    p
    a
    g
    e
    '
    s

    c
    a
    n
    v
    a
    s

    w
    i
    t
    h

    T
    H
    I
    S

    s
    e
    s
    s
    i
    o
    n
    '
    s

    r
    e
    g
    i
    s
    t
    r
    y
    .
    pub fn register_page(
        &self,
        page: u32,
        canvas_id: &str,
        host_id: Option<&str>,
        elements: Option<PageElements<'_>>,
    ) {
        if self.engine() {
            bridge::register_page(
                self.inner.sid,
                page,
                canvas_id,
                host_id.unwrap_or(""),
                elements.map(|e| e.canvas),
                elements.and_then(|e| e.host),
            );
        }
    }

    pub fn unregister_page(&self, canvas_id: &str) {
        // Teardown-shaped: admitted until the engine forgets the sid (the
        // engine ignores a retired one anyway).
        if self.inner.registered && bridge::has_pdf_reader() {
            bridge::unregister_page(self.inner.sid, canvas_id);
        }
    }

    /
    /
    /

    C
    a
    n
    c
    e
    l

    e
    v
    e
    r
    y

    i
    n
    -
    f
    l
    i
    g
    h
    t

    p
    a
    g
    e

    r
    e
    n
    d
    e
    r

    o
    f

    t
    h
    i
    s

    s
    e
    s
    s
    i
    o
    n
    .
    pub fn cancel_page_renders(&self) {
        if self.engine() {
            bridge::cancel_page_renders(self.inner.sid);
        }
    }

    /// Queue one page's raster at `rank` in this session's page lane: lower
    /// runs first.
    pub async fn render_page(
        &self,
        canvas_id: &str,
        scale: f64,
        render_text: bool,
        rank: u32,
    ) -> Result<RenderResult, EngineError> {
        let sid = self.require()?;
        let value = bridge::render_page(sid, canvas_id, scale, render_text, rank).await;
        let result = api::resolve::<RenderResult>(value, "render")?;
        if !self.is_live() {
            return Err(no_session());
        }
        Ok(result)
    }

    /// Stand down one page's queued or in-flight raster, leaving its
    /// registration alone.
    pub fn cancel_page(&self, canvas_id: &str) {
        if let Ok(sid) = self.require() {
            bridge::cancel_page(sid, canvas_id);
        }
    }

    /
    /
    /

    T
    h
    e

    i
    n
    t
    r
    i
    n
    s
    i
    c

    b
    o
    x

    o
    f

    o
    n
    e

    p
    a
    g
    e
    ,

    r
    e
    a
    d

    f
    r
    o
    m

    t
    h
    e

    d
    o
    c
    u
    m
    e
    n
    t
    .
    pub async fn probe_page_size(&self, page: u32) -> Result<PageSizeResult, EngineError> {
        let sid = self.require()?;
        let value = bridge::probe_page_size(sid, page).await;
        let result = api::resolve::<PageSizeResult>(value, "probe")?;
        if !self.is_live() {
            return Err(no_session());
        }
        Ok(result)
    }

    // --- Thumbnails -----------------------------------------------------

    /// Render one thumbnail through this session's cached thumbnail lane.
    pub async fn render_thumb(
        &self,
        canvas_id: &str,
        page: u32,
        scale: f64,
    ) -> Result<ThumbResult, EngineError> {
        let sid = self.require()?;
        let value = bridge::render_thumb(sid, canvas_id, page, scale).await;
        let result = api::resolve::<ThumbResult>(value, "thumb")?;
        if !self.is_live() {
            return Err(no_session());
        }
        Ok(result)
    }

    /// Cancel an in-flight thumbnail render (cell unmounted). Does NOT evict
    /// the cached bitmap.
    pub fn cancel_thumb(&self, canvas_id: &str) {
        if self.inner.registered && bridge::has_pdf_reader() {
            bridge::cancel_thumb(self.inner.sid, canvas_id);
        }
    }

    /// Synchronous probe: is this page's thumbnail cached at `scale`?
    pub fn has_thumb(&self, page: u32, scale: f64) -> bool {
        self.engine() && bridge::has_thumb(self.inner.sid, page, scale)
    }

    /
    /
    /

    R
    e
    n
    d
    e
    r

    a

    p
    a
    g
    e

    i
    n
    t
    o

    t
    h
    e

    t
    h
    u
    m
    b
    n
    a
    i
    l

    c
    a
    c
    h
    e

    w
    i
    t
    h

    n
    o

    D
    O
    M

    c
    a
    n
    v
    a
    s
    .
    pub async fn prefetch_thumb(&self, page: u32, scale: f64) {
        if self.engine() {
            let _ = bridge::prefetch_thumb(self.inner.sid, page, scale).await;
        }
    }

    /// Park this session's idle prefetch (the pane left the screen).
    pub fn suspend_prefetches(&self) {
        if self.engine() {
            bridge::suspend_prefetches(self.inner.sid);
        }
    }

    pub fn resume_prefetches(&self) {
        if self.engine() {
            bridge::resume_prefetches(self.inner.sid);
        }
    }

    // --- Memory ---------------------------------------------------------

    /// Advisory cleanup of this session's no-longer-needed rasters.
    pub fn sweep(&self) {
        if self.engine() {
            bridge::sweep(self.inner.sid);
        }
    }

    /// Drop the scrub covers this session's page hosts still carry.
    pub fn sweep_snapshots(&self) {
        if self.engine() {
            bridge::sweep_snapshots(self.inner.sid);
        }
    }

    // --- Search ---------------------------------------------------------

    /
    /
    /

    B
    u
    i
    l
    d

    o
    r

    a
    d
    o
    p
    t

    t
    h
    i
    s

    s
    e
    s
    s
    i
    o
    n
    '
    s

    s
    e
    a
    r
    c
    h

    i
    n
    d
    e
    x
    .
    pub async fn build_search_index(&self, num_pages: u32) -> Result<u32, EngineError> {
        search::build(self, num_pages).await
    }

    /
    /
    /

    Q
    u
    e
    r
    y

    t
    h
    i
    s

    s
    e
    s
    s
    i
    o
    n
    '
    s

    i
    n
    d
    e
    x

    a
    n
    d

    p
    u
    b
    l
    i
    s
    h

    t
    h
    e

    q
    u
    e
    r
    y
    .
    pub fn search(&self, query: &str) -> SearchResponse {
        let response = self.with_search(|s| s.query(query));
        if self.engine() {
            bridge::set_search_context(self.inner.sid, query);
        }
        response
    }

    pub fn set_active_match(&self, page: u32, index: i32) {
        if self.engine() {
            bridge::set_active_match(self.inner.sid, page, index);
        }
    }

    pub fn clear_highlights(&self) {
        if self.engine() {
            bridge::clear_highlights(self.inner.sid);
        }
    }

    pub(crate) async fn extract_page_text(&self, page: u32) -> Option<wasm_bindgen::JsValue> {
        if !self.engine() {
            return None;
        }
        Some(bridge::extract_page_text(self.inner.sid, page).await)
    }

    // --- Paper ----------------------------------------------------------

    /// The reader's paper settings, stated to THIS session (on creation and
    /// on every change).
    pub fn paper_configure(&self, blend_on: bool, config: PaperConfig) {
        if self.is_live() {
            backdrop::configure(self, blend_on, config);
        }
    }

    /
    /
    /

    T
    h
    e

    d
    o
    c
    u
    m
    e
    n
    t

    o
    p
    e
    n
    e
    d
    :

    t
    h
    e

    p
    a
    p
    e
    r

    s
    t
    a
    t
    e

    m
    a
    c
    h
    i
    n
    e

    s
    t
    a
    r
    t
    s
    .
    pub fn paper_document_open(&self, path: &str, num_pages: u32) {
        if self.is_live() {
            backdrop::document_open(self, path, num_pages);
        }
    }

    /// A live render of `canvas_id` completed: drain its stashed frame.
    pub fn paper_live_frame(&self, canvas_id: &str) {
        if self.is_live() {
            backdrop::live_frame(self, canvas_id);
        }
    }

    /// The viewport's position along the page ladder.
    pub fn paper_position(&self, pos: f64) {
        if self.is_live() {
            backdrop::position(self, pos);
        }
    }

    /// This session's look-ahead samples in flight.
    pub fn paper_pending_samples(&self) -> usize {
        self.with_paper(|p| p.pending_samples())
    }

    pub(crate) fn set_paper(&self, hex: Option<&str>) {
        if self.engine() {
            bridge::set_paper(self.inner.sid, hex.unwrap_or(""));
        }
    }

    pub(crate) fn set_paper_active(&self, on: bool) {
        if self.engine() {
            bridge::set_paper_active(self.inner.sid, on);
        }
    }

    pub(crate) fn take_paper_frame(&self, canvas_id: &str) -> Option<api::PaperFrame> {
        if !self.engine() {
            return None;
        }
        api::paper::parse_frame(&bridge::take_paper_frame(self.inner.sid, canvas_id))
    }

    pub(crate) async fn sample_paper_page(
        &self,
        page: u32,
    ) -> Result<Option<api::PaperFrame>, EngineError> {
        if !self.engine() {
            return Ok(None);
        }
        let value = bridge::sample_paper_page(self.inner.sid, page).await;
        api::paper::resolve_frame(value, &format!("samplePaperPage({page})"))
    }

    // --- Diagnostics ----------------------------------------------------

    /
    /
    /

    T
    h
    i
    s

    s
    e
    s
    s
    i
    o
    n
    '
    s

    o
    w
    n

    g
    a
    u
    g
    e
    s

    a
    n
    d

    c
    o
    u
    n
    t
    e
    r
    s
    .
    pub fn stats(&self) -> Option<EngineStats> {
        if !(self.inner.registered && bridge::has_pdf_reader()) {
            return None;
        }
        serde_wasm_bindgen::from_value(bridge::session_stats(self.inner.sid)).ok()
    }

    // --- Teardown -------------------------------------------------------

    /
    /
    /

    T
    e
    a
    r

    t
    h
    e

    s
    e
    s
    s
    i
    o
    n

    d
    o
    w
    n
    ;

    i
    d
    e
    m
    p
    o
    t
    e
    n
    t
    ,

    r
    e
    s
    o
    l
    v
    i
    n
    g

    o
    n
    c
    e

    t
    h
    e

    e
    n
    g
    i
    n
    e

    i
    s

    /
    /
    !

    g
    o
    n
    e
    .
    pub fn dispose(&self) -> impl std::future::Future<Output = ()> + use<> {
        let begun = self.inner.state.get() == State::Live;
        if begun {
            self.inner.state.set(State::Disposing);
            self.with_paper(|p| p.invalidate());
            self.with_search(|s| search::retain(std::mem::take(s)));
        }
        let inner = self.inner.clone();
        async move {
            if !begun {
                return;
            }
            if inner.registered && bridge::has_pdf_reader() {
                let _ = bridge::destroy_session(inner.sid).await;
            }
            inner.state.set(State::Disposed);
        }
    }
}

/// `{ok:true, outline}` — engine.resolveOutline.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutlinePayload {
    outline: Vec<OutlineEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        futures::executor::block_on(f)
    }

    #[test]
    fn sids_are_unique_and_monotonic() {
        let a = PdfSession::create();
        let b = PdfSession::create();
        assert!(b.sid() > a.sid());
        assert!(!a.same(&b));
        assert!(a.same(&a.clone()));
    }

    #[test]
    fn a_disposed_session_refuses_every_call() {
        let s = PdfSession::create();
        assert!(s.is_live());
        block_on(s.dispose());
        assert!(!s.is_live());
        let opened = block_on(s.open("/shelf/book.pdf"));
        assert_eq!(opened.err().map(|e| e.name).as_deref(), Some("no_session"));
        let rendered = block_on(s.render_page("cv", 1.0, false, 0));
        assert_eq!(
            rendered.err().map(|e| e.name).as_deref(),
            Some("no_session")
        );
        assert!(!s.has_thumb(1, 0.25));
        assert!(block_on(s.outline()).unwrap().is_empty());
        // A second dispose is a no-op, not a second teardown.
        block_on(s.dispose());
        assert!(!s.is_live());
    }

    #[test]
    fn a_session_stops_accepting_at_the_dispose_call() {
        let s = PdfSession::create();
        let teardown = s.dispose();
        // Not polled yet: the session already refuses.
        assert!(!s.is_live());
        let opened = block_on(s.open("/shelf/book.pdf"));
        assert_eq!(opened.err().map(|e| e.name).as_deref(), Some("no_session"));
        block_on(teardown);
        assert!(!s.is_live());
    }

    #[test]
    fn disposing_one_session_leaves_another_untouched() {
        let a = PdfSession::create();
        let b = PdfSession::create();
        backdrop::test_feed(&a, "/a.pdf");
        backdrop::test_feed(&b, "/b.pdf");
        block_on(a.dispose());
        assert!(b.is_live());
        assert!(backdrop::test_has_palette(&b));
        assert!(!backdrop::test_has_palette(&a));
    }
}
