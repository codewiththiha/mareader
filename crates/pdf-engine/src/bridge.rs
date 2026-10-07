/
/
!

W
a
s
m
-
b
i
n
d
g
e
n

i
n
t
e
r
o
p

w
i
t
h

t
h
e

i
m
p
e
r
a
t
i
v
e

P
D
F

e
n
g
i
n
e
,

t
h
e

o
n
l
y

p
l
a
c
e

/
/
!

`
w
i
n
d
o
w
.
P
D
F
R
e
a
d
e
r
`

i
s

d
e
c
l
a
r
e
d
.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    // --- Session lifecycle ------------------------------------------------

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

    e
    n
    g
    i
    n
    e

    s
    e
    s
    s
    i
    o
    n

    `
    s
    i
    d
    `
    ;

    a

    s
    i
    d

    i
    s

    n
    e
    v
    e
    r

    r
    e
    u
    s
    e
    d
    .
    pub fn create_session(sid: u32) -> bool;

    /
    /
    /

    T
    e
    a
    r

    s
    e
    s
    s
    i
    o
    n

    `
    s
    i
    d
    `

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
    .
    pub async fn destroy_session(sid: u32) -> JsValue;

    /// Make `sid` the session whose paper the root backdrop shows.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "presentSession")]
    pub fn present_session(sid: u32);

    /// One session's gauges and counters (`Stats`), or null for an unknown
    /// sid.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "sessionStats")]
    pub fn session_stats(sid: u32) -> JsValue;

    // --- Document ---------------------------------------------------------

    /// Open `path` in session `sid` — a session holds exactly one document.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub async fn open(sid: u32, path: &str) -> JsValue;

    /
    /

    T
    h
    e

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

    i
    s

    r
    e
    s
    o
    l
    v
    e
    d

    a
    f
    t
    e
    r

    o
    p
    e
    n
    ,

    o
    n
    e

    r
    o
    u
    n
    d

    t
    r
    i
    p

    p
    e
    r

    e
    n
    t
    r
    y
    .
    pub async fn resolve_outline(sid: u32) -> JsValue;

    /
    /
    /

    R
    e
    n
    d
    e
    r

    p
    a
    g
    e

    1

    o
    f

    t
    h
    e

    b
    o
    o
    k

    t
    o

    a

    J
    P
    E
    G

    d
    a
    t
    a

    U
    R
    L
    ,

    f
    o
    r

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
    pub async fn cover_data_url(sid: u32, path: &str, max_width: f64) -> JsValue;

    // --- Pages ------------------------------------------------------------

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
    ,

    t
    y
    p
    e
    d

    a
    n
    d

    i
    d
    -
    f
    r
    e
    e
    .
    pub fn register_page(
        sid: u32,
        page: u32,
        canvas_id: &str,
        host_id: &str,
        canvas: Option<&web_sys::Element>,
        host: Option<&web_sys::Element>,
    );

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "unregisterPage")]
    pub fn unregister_page(sid: u32, canvas_id: &str);

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
    e

    s
    e
    s
    s
    i
    o
    n

    i
    n

    o
    n
    e

    c
    a
    l
    l
    .
    pub fn cancel_page_renders(sid: u32);

    /
    /
    /

    Q
    u
    e
    u
    e

    o
    n
    e

    p
    a
    g
    e
    '
    s

    r
    a
    s
    t
    e
    r
    ;

    `
    r
    a
    n
    k
    `

    o
    r
    d
    e
    r
    s

    t
    h
    e

    l
    a
    n
    e
    .
    pub async fn render_page(
        sid: u32,
        canvas_id: &str,
        scale: f64,
        render_text: bool,
        rank: u32,
    ) -> JsValue;

    /
    /
    /

    S
    t
    o
    p

    o
    n
    e

    p
    a
    g
    e
    '
    s

    q
    u
    e
    u
    e
    d

    o
    r

    i
    n
    -
    f
    l
    i
    g
    h
    t

    r
    a
    s
    t
    e
    r
    ;

    i
    t

    s
    t
    a
    y
    s

    r
    e
    g
    i
    s
    t
    e
    r
    e
    d
    .
    pub fn cancel_page(sid: u32, canvas_id: &str);

    /
    /
    /

    O
    n
    e

    p
    a
    g
    e
    '
    s

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
    ,

    w
    i
    t
    h
    o
    u
    t

    r
    a
    s
    t
    e
    r
    i
    s
    i
    n
    g

    i
    t
    .
    pub async fn probe_page_size(sid: u32, page: u32) -> JsValue;

    // --- Thumbnails -------------------------------------------------------

    /
    /

    T
    h
    u
    m
    b
    n
    a
    i
    l

    l
    a
    n
    e
    :

    a

    c
    h
    e
    a
    p

    r
    e
    n
    d
    e
    r

    p
    a
    t
    h

    w
    i
    t
    h

    a

    p
    e
    r
    -
    s
    e
    s
    s
    i
    o
    n

    b
    i
    t
    m
    a
    p

    c
    a
    c
    h
    e
    .
    pub async fn render_thumb(sid: u32, canvas_id: &str, page: u32, scale: f64) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "cancelThumb")]
    pub fn cancel_thumb(sid: u32, canvas_id: &str);

    /
    /
    /

    S
    Y
    N
    C
    H
    R
    O
    N
    O
    U
    S

    c
    a
    c
    h
    e

    p
    r
    o
    b
    e
    ,

    r
    e
    a
    d

    w
    h
    i
    l
    e

    a

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
    e
    l
    l

    b
    u
    i
    l
    d
    s
    .
    pub fn has_thumb(sid: u32, page: u32, scale: f64) -> bool;

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
    pub async fn prefetch_thumb(sid: u32, page: u32, scale: f64) -> JsValue;

    /
    /
    /

    T
    h
    e

    p
    a
    n
    e

    l
    e
    f
    t

    t
    h
    e

    s
    c
    r
    e
    e
    n
    :

    a
    b
    a
    n
    d
    o
    n

    t
    h
    e

    i
    d
    l
    e

    p
    r
    e
    f
    e
    t
    c
    h
    e
    s
    .
    pub fn suspend_prefetches(sid: u32);

    /// The pane is on screen again: the session's idle prefetch may run.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "resumePrefetches")]
    pub fn resume_prefetches(sid: u32);

    // --- Search -----------------------------------------------------------

    /// Extract one page's text runs (`{ok, page, items:[{str,x,y,w,h}]}`)
    /// for the session's Rust-owned search index.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "extractPageText")]
    pub async fn extract_page_text(sid: u32, page: u32) -> JsValue;

    /
    /
    /

    P
    u
    b
    l
    i
    s
    h

    t
    h
    e

    a
    c
    t
    i
    v
    e

    q
    u
    e
    r
    y

    t
    o

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
    '
    s

    t
    e
    x
    t

    l
    a
    y
    e
    r
    s
    .
    pub fn set_search_context(sid: u32, query: &str);

    /
    /
    /

    E
    m
    p
    h
    a
    s
    i
    s
    e

    o
    c
    c
    u
    r
    r
    e
    n
    c
    e

    `
    i
    n
    d
    e
    x
    `

    o
    f

    `
    p
    a
    g
    e
    `

    a
    s

    t
    h
    e

    c
    u
    r
    r
    e
    n
    t

    m
    a
    t
    c
    h
    .
    pub fn set_active_match(sid: u32, page: u32, index: i32);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "clearHighlights")]
    pub fn clear_highlights(sid: u32);

    // --- Paper ------------------------------------------------------------

    /
    /

    T
    h
    e

    p
    a
    p
    e
    r

    p
    i
    p
    e
    l
    i
    n
    e
    '
    s

    e
    y
    e
    s
    :

    t
    h
    e

    e
    n
    g
    i
    n
    e

    o
    w
    n
    s

    t
    h
    e

    c
    a
    n
    v
    a
    s
    e
    s
    ,

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

    /
    /

    t
    h
    e

    c
    o
    l
    o
    u
    r
    s
    .
    pub fn set_paper(sid: u32, hex: &str);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setPaperActive")]
    pub fn set_paper_active(sid: u32, on: bool);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "takePaperFrame")]
    pub fn take_paper_frame(sid: u32, canvas_id: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "samplePaperPage")]
    pub async fn sample_paper_page(sid: u32, page: u32) -> JsValue;

    // --- Memory -----------------------------------------------------------

    /
    /
    /

    R
    e
    l
    e
    a
    s
    e

    r
    a
    s
    t
    e
    r
    s

    a
    n
    d

    c
    a
    c
    h
    e
    s

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

    n
    o

    l
    o
    n
    g
    e
    r

    n
    e
    e
    d
    s
    .
    pub fn sweep(sid: u32);

    /
    /
    /

    D
    r
    o
    p

    t
    h
    e

    `
    .
    p
    a
    g
    e
    -
    s
    n
    a
    p
    s
    h
    o
    t
    `

    s
    c
    r
    u
    b

    c
    o
    v
    e
    r
    s

    o
    f

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
    '
    s

    p
    a
    g
    e

    h
    o
    s
    t
    s
    .
    pub fn sweep_snapshots(sid: u32);

    // --- Realm: appearance broadcast --------------------------------------

    /
    /

    A
    p
    p
    e
    a
    r
    a
    n
    c
    e

    i
    s

    g
    l
    o
    b
    a
    l
    ;

    t
    h
    e

    r
    a
    s
    t
    e
    r
    s

    a
    r
    e

    s
    e
    s
    s
    i
    o
    n
    -
    o
    w
    n
    e
    d
    .
    pub fn refresh_theme();

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setScrubMode")]
    pub fn set_scrub_mode(on: bool);

    /
    /

    T
    h
    e

    a
    p
    p
    e
    a
    r
    a
    n
    c
    e

    p
    o
    p
    o
    v
    e
    r

    i
    s

    o
    p
    e
    n
    :

    e
    a
    c
    h

    s
    e
    s
    s
    i
    o
    n

    r
    e
    t
    a
    i
    n
    s

    i
    t
    s

    r
    a
    w
    s
    .
    pub fn set_appearance_menu_open(on: bool);

    // --- Realm: diagnostics -----------------------------------------------

    /
    /
    /

    T
    h
    e

    r
    e
    a
    l
    m

    a
    g
    g
    r
    e
    g
    a
    t
    e
    :

    e
    v
    e
    r
    y

    s
    e
    s
    s
    i
    o
    n
    '
    s

    g
    a
    u
    g
    e
    s

    p
    l
    u
    s

    t
    h
    e

    t
    o
    t
    a
    l
    s
    .
    pub fn stats() -> JsValue;

    /// Turn the engine's lifecycle event narration on/off.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setLifecycleLog")]
    pub fn set_lifecycle_log(on: bool);
}

/
/
/

T
r
u
e

w
h
e
n

`
w
i
n
d
o
w
.
P
D
F
R
e
a
d
e
r
`

e
x
i
s
t
s
;

c
h
e
c
k

b
e
f
o
r
e

a
n
y

c
a
l
l
.
pub fn has_pdf_reader() -> bool {
    if !cfg!(target_arch = "wasm32") {
        return false;
    }
    web_sys::window()
        .map(|w| {
            let g: js_sys::Object = w.unchecked_into();
            js_sys::Reflect::get(&g, &JsValue::from_str("PDFReader"))
                .map(|v| !(v.is_undefined() || v.is_null()))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

/
/
/

R
e
l
e
a
s
e

e
n
g
i
n
e

s
e
s
s
i
o
n

`
s
i
d
`

w
i
t
h
o
u
t

a
w
a
i
t
i
n
g

i
t
.
pub fn release_session_detached(sid: u32) {
    if !cfg!(target_arch = "wasm32") {
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let global: js_sys::Object = window.unchecked_into();
    let Ok(reader) = js_sys::Reflect::get(&global, &JsValue::from_str("PDFReader")) else {
        return;
    };
    let Ok(destroy) = js_sys::Reflect::get(&reader, &JsValue::from_str("destroySession")) else {
        return;
    };
    if let Some(destroy) = destroy.dyn_ref::<js_sys::Function>() {
        let _ = destroy.call1(&reader, &JsValue::from_f64(f64::from(sid)));
    }
}
