/
/
!

T
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

w
i
r
e
d

t
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

l
i
v
e

h
a
l
f

o
f

/
/
!

`
p
d
f
-
p
a
p
e
r
`
.

use std::sync::atomic::{AtomicUsize, Ordering};

use wasm_bindgen_futures::spawn_local;

use pdf_paper::{PAPER_SHARE, PagePalette, PaperArea, PaperConfig, PaperDetector, Rgb};

use crate::api;
use crate::session::PdfSession;

mod lookahead;

use lookahead::{ensure_lookahead, sample_page};

/
/

N
a
m
e
d

b
y

t
h
e

s
t
a
t
e
-
m
a
c
h
i
n
e

t
e
s
t
s

d
i
r
e
c
t
l
y
.
use lookahead::lookahead_wants;

/
/
/

L
o
o
k
-
a
h
e
a
d

s
a
m
p
l
e
s

i
n

f
l
i
g
h
t

a
c
r
o
s
s

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
,

a

g
a
u
g
e
.
static SAMPLES_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// How many look-ahead samples are in flight, realm-wide.
pub fn pending_samples() -> usize {
    SAMPLES_IN_FLIGHT.load(Ordering::Relaxed)
}

/// One session's paper state.
pub(crate) struct Paper {
    config: PaperConfig,
    blend_on: bool,
    doc_path: Option<String>,
    num_pages: u32,
    /
    /
    /

    P
    e
    r
    -
    p
    a
    g
    e

    c
    o
    l
    o
    u
    r
    s
    ,

    o
    n
    e

    l
    a
    d
    d
    e
    r

    p
    e
    r

    d
    e
    t
    e
    c
    t
    i
    o
    n

    a
    r
    e
    a
    .
    palettes: [PagePalette; 2],
    /
    /
    /

    P
    e
    r
    -
    a
    r
    e
    a

    f
    i
    r
    s
    t

    l
    i
    v
    e

    c
    o
    l
    o
    u
    r
    ,

    t
    h
    e

    f
    a
    l
    l
    b
    a
    c
    k

    a
    t

    b
    o
    o
    k

    o
    p
    e
    n
    .
    interim: [Option<Rgb>; 2],
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

    c
    o
    l
    o
    u
    r

    h
    a
    n
    d
    e
    d

    t
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
    ;

    u
    n
    k
    n
    o
    w
    n

    a
    n
    s
    w
    e
    r
    s

    h
    o
    l
    d

    i
    t
    .
    published: Option<String>,
    /// The reader's page-ladder position as of the last [`position`] call.
    position: f64,
    /
    /
    /

    P
    a
    g
    e
    s

    w
    h
    o
    s
    e

    o
    f
    f
    s
    c
    r
    e
    e
    n

    l
    o
    o
    k
    -
    a
    h
    e
    a
    d

    s
    a
    m
    p
    l
    e

    i
    s

    i
    n

    f
    l
    i
    g
    h
    t
    .
    sampling: std::collections::HashSet<u32>,
    /// Generation token: bumped on document open and on dispose.
    epoch: u64,
}

impl Default for Paper {
    fn default() -> Self {
        Self {
            config: PaperConfig::default(),
            blend_on: false,
            doc_path: None,
            num_pages: 0,
            palettes: [PagePalette::new(), PagePalette::new()],
            interim: [None, None],
            published: None,
            position: 1.0,
            sampling: std::collections::HashSet::new(),
            epoch: 0,
        }
    }
}

impl Drop for Paper {
    fn drop(&mut self) {
        self.clear_sampling();
    }
}

impl Paper {
    /// This session's look-ahead samples in flight.
    pub(crate) fn pending_samples(&self) -> usize {
        self.sampling.len()
    }

    fn start_sample(&mut self, page: u32) {
        if self.sampling.insert(page) {
            SAMPLES_IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn end_sample(&mut self, page: u32) {
        if self.sampling.remove(&page) {
            SAMPLES_IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
        }
    }

    fn clear_sampling(&mut self) {
        SAMPLES_IN_FLIGHT.fetch_sub(self.sampling.len(), Ordering::Relaxed);
        self.sampling.clear();
    }

    /
    /
    /

    T
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
    s

    b
    e
    i
    n
    g

    d
    i
    s
    p
    o
    s
    e
    d
    :

    s
    a
    m
    p
    l
    e
    s

    a
    b
    a
    n
    d
    o
    n
    e
    d
    ,

    d
    o
    c
    u
    m
    e
    n
    t

    f
    o
    r
    g
    o
    t
    t
    e
    n
    .
    pub(crate) fn invalidate(&mut self) {
        self.epoch += 1;
        self.clear_sampling();
        self.doc_path = None;
        self.num_pages = 0;
        for palette in &mut self.palettes {
            palette.clear();
        }
        self.interim = [None, None];
        self.published = None;
    }
}

/// The ladder index a detection area resolves from.
pub(super) fn slot(area: PaperArea) -> usize {
    match area {
        PaperArea::WholePage => 0,
        PaperArea::Edges => 1,
    }
}

/
/
/

S
p
a
w
n

a
n

e
n
g
i
n
e
-
t
a
l
k
i
n
g

t
a
s
k

o
n
l
y

w
h
e
n

a
n

e
n
g
i
n
e

i
s

a
t
t
a
c
h
e
d
.
pub(super) fn spawn_engine<F: std::future::Future<Output = ()> + 'static>(
    session: &PdfSession,
    f: impl FnOnce(PdfSession) -> F + 'static,
) {
    if crate::bridge::has_pdf_reader() && session.is_live() {
        let session = session.clone();
        spawn_local(async move {
            f(session).await;
        });
    }
}

/
/
/

L
a
n
d

a
n

o
f
f
s
c
r
e
e
n

s
a
m
p
l
e

t
a
k
e
n

a
t

`
e
p
o
c
h
`
,

i
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
s

l
i
v
e
.
pub(super) fn land_sample(
    session: &PdfSession,
    epoch: u64,
    page: u32,
    frame: Option<&api::PaperFrame>,
) -> bool {
    if !session.is_live() {
        return false;
    }
    session.with_paper(|s| {
        if s.epoch != epoch {
            return false; // the document changed under the sample
        }
        s.end_sample(page);
        match frame {
            Some(f) => feed_state(s, f),
            None => false, // unreadable page: nothing to learn
        }
    })
}

/
/
/

T
h
e

r
e
a
d
e
r
'
s

p
a
p
e
r

s
e
t
t
i
n
g
s

c
h
a
n
g
e
d
,

o
r

a
r
e

r
e
s
t
a
t
e
d

t
o

a

n
e
w

/
/
/

s
e
s
s
i
o
n
.
pub(crate) fn configure(session: &PdfSession, blend_on: bool, mut config: PaperConfig) {
    config.sanitize();
    session.with_paper(|s| {
        if s.config.edge_width != config.edge_width {
            let edge_slot = slot(PaperArea::Edges);
            s.palettes[edge_slot].clear();
            s.interim[edge_slot] = None;
        }
        s.blend_on = blend_on;
        s.config = config;
    });
    session.set_paper_active(blend_on);
    publish(session);
    let cold = session.with_paper(|s| s.doc_path.is_some() && s.blend_on && s.published.is_none());
    if cold {
        let (epoch, page) = session.with_paper(|s| (s.epoch, s.position.floor().max(1.0) as u32));
        sample_page(session, epoch, page);
    }
    ensure_lookahead(session);
}

/
/
/

A

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

s
t
a
r
t

i
t
s

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
.
pub(crate) fn document_open(session: &PdfSession, path: &str, num_pages: u32) {
    session.with_paper(|s| {
        s.epoch += 1; // abandon anything in flight for an earlier state
        s.clear_sampling();
        s.doc_path = Some(path.to_string());
        s.num_pages = num_pages;
        for palette in &mut s.palettes {
            palette.clear();
        }
        s.interim = [None, None];
        s.published = None;
        s.position = 1.0;
    });
    session.set_paper(None);
}

/
/
/

A

l
i
v
e

r
e
n
d
e
r

c
o
m
p
l
e
t
e
d
:

d
r
a
i
n

i
t
s

f
r
a
m
e

i
n
t
o

t
h
e

p
a
l
e
t
t
e
.
pub(crate) fn live_frame(session: &PdfSession, canvas_id: &str) {
    if !session.with_paper(|s| s.blend_on) {
        return;
    }
    if let Some(frame) = session.take_paper_frame(canvas_id) {
        feed_frame(session, &frame);
    }
}

/
/
/

T
h
e

v
i
e
w
p
o
r
t
'
s

p
o
s
i
t
i
o
n

a
l
o
n
g

t
h
e

p
a
g
e

l
a
d
d
e
r
,

f
r
a
c
t
i
o
n
a
l
.
pub(crate) fn position(session: &PdfSession, pos: f64) {
    if !pos.is_finite() || pos <= 0.0 {
        return;
    }
    let moved = session.with_paper(|s| {
        let moved = (pos - s.position).abs() > f64::EPSILON;
        s.position = pos;
        moved
    });
    if moved {
        publish(session);
        ensure_lookahead(session);
    }
}

/// Feed one raw frame (live stash or offscreen sample) into the session.
fn feed_frame(session: &PdfSession, frame: &api::PaperFrame) {
    let changed = session.with_paper(|s| feed_state(s, frame));
    if changed {
        publish(session);
        ensure_lookahead(session);
    }
}

/
/
/

T
h
e

s
t
a
t
e

h
a
l
f

o
f

a

f
e
e
d
,

f
o
r

i
n
-
b
o
r
r
o
w

u
s
e
.
fn feed_state(s: &mut Paper, frame: &api::PaperFrame) -> bool {
    if s.doc_path.is_none() || frame.width == 0 || frame.height == 0 {
        return false;
    }
    /
    /

    D
    e
    t
    e
    c
    t

    t
    h
    r
    o
    u
    g
    h

    B
    O
    T
    H

    a
    r
    e
    a
    s

    a
    t

    o
    n
    c
    e
    :

    r
    a
    w

    p
    i
    x
    e
    l
    s
    ,

    t
    w
    o

    h
    i
    s
    t
    o
    g
    r
    a
    m
    s
    .
    let (w, h) = (frame.width as usize, frame.height as usize);
    let edge = s.config.edge_width as usize;
    let mut whole = PaperDetector::new();
    whole.feed(PaperArea::WholePage, w, h, &frame.data, edge);
    let mut edges = PaperDetector::new();
    edges.feed(PaperArea::Edges, w, h, &frame.data, edge);
    let colours = [whole.dominant(PAPER_SHARE), edges.dominant(PAPER_SHARE)];

    let slot = slot(s.config.area);
    let changed = s.palettes[slot].get(frame.page) != colours[slot];
    let had_interim = s.interim[slot].is_some();
    if let Some(colour) = colours[0] {
        s.palettes[0].set(frame.page, colour);
    }
    if let Some(colour) = colours[1] {
        s.palettes[1].set(frame.page, colour);
    }
    if s.interim[0].is_none() {
        s.interim[0] = colours[0];
    }
    if s.interim[1].is_none() {
        s.interim[1] = colours[1];
    }
    changed || (!had_interim && s.interim[slot].is_some())
}

/
/
/

T
h
e

c
o
l
o
u
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

r
e
s
o
l
v
e
s

r
i
g
h
t

n
o
w
,

i
f

a
n
y
.
fn resolve(s: &Paper) -> Option<Rgb> {
    let slot = slot(s.config.area);
    s.palettes[slot].colour_at(s.position).or(s.interim[slot])
}

/
/
/

H
a
n
d

t
h
e

r
e
s
o
l
v
e
d

c
o
l
o
u
r

t
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
,

o
r

c
l
e
a
r

i
t

d
e
l
i
b
e
r
a
t
e
l
y
.
pub(super) fn publish(session: &PdfSession) {
    let outcome = session.with_paper(|s| {
        if s.doc_path.is_none() || !s.blend_on {
            return (None, s.published.take());
        }
        match resolve(s).map(|c| c.to_hex()) {
            Some(hex) => {
                if s.published.as_deref() == Some(hex.as_str()) {
                    return (None, None); // unchanged
                }
                s.published = Some(hex.clone());
                (Some(hex), None)
            }
            None => (None, None), // hold
        }
    });
    match outcome {
        (Some(hex), _) => session.set_paper(Some(hex.as_str())),
        // Deliberate blank: no book / blend off — clear the session's paper.
        (None, Some(_)) => session.set_paper(None),
        _ => {}
    }
}

/
/
/

T
e
s
t

h
o
o
k
:

o
p
e
n

a

d
o
c
u
m
e
n
t

a
n
d

f
e
e
d

o
n
e

c
r
e
a
m

p
a
g
e
.
pub(crate) fn test_feed(session: &PdfSession, path: &str) {
    session.with_paper(|s| s.blend_on = true);
    document_open(session, path, 4);
    let frame = tests::uniform(1, 16, 16, [0xfa, 0xf4, 0xe8]);
    feed_frame(session, &frame);
}

#[cfg(test)]
pub(crate) fn test_has_palette(session: &PdfSession) -> bool {
    session.with_paper(|s| s.palettes[0].contains(1))
}

/
/

T
h
e

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

r
u
n
s

o
n

t
h
e

h
o
s
t
;

o
n
l
y

i
n
-
R
u
s
t

t
r
a
n
s
i
t
i
o
n
s

r
u
n
.
mod tests {
    use super::*;
    use pdf_paper::PaperArea;
    use std::cell::RefCell;

    /
    /

    E
    a
    c
    h

    t
    e
    s
    t

    d
    r
    i
    v
    e
    s

    O
    N
    E

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

    t
    h
    r
    o
    u
    g
    h

    t
    h
    e
    s
    e

    w
    r
    a
    p
    p
    e
    r
    s
    .
    thread_local! {
        static CURRENT: RefCell<Option<PdfSession>> = const { RefCell::new(None) };
    }

    fn cur() -> PdfSession {
        CURRENT.with(|c| c.borrow().clone().expect("reset_session first"))
    }

    fn with<R>(f: impl FnOnce(&mut Paper) -> R) -> R {
        cur().with_paper(f)
    }

    fn document_open(path: &str, num_pages: u32) {
        super::document_open(&cur(), path, num_pages);
    }

    fn feed_frame(frame: &api::PaperFrame) {
        super::feed_frame(&cur(), frame);
    }

    fn position(pos: f64) {
        super::position(&cur(), pos);
    }

    fn configure(blend_on: bool, config: PaperConfig) {
        super::configure(&cur(), blend_on, config);
    }

    /// A uniform `w × h` frame of one colour.
    pub(super) fn uniform(page: u32, w: u32, h: u32, colour: [u8; 3]) -> api::PaperFrame {
        let mut data = vec![255u8; (w * h * 4) as usize];
        for i in (0..data.len()).step_by(4) {
            data[i] = colour[0];
            data[i + 1] = colour[1];
            data[i + 2] = colour[2];
        }
        api::PaperFrame {
            page,
            width: w,
            height: h,
            data,
        }
    }

    const CREAM: [u8; 3] = [0xfa, 0xf4, 0xe8];
    const INK: [u8; 3] = [0x40, 0x40, 0x40];
    const WHITE: [u8; 3] = [0xff, 0xff, 0xff];
    const MAROON: [u8; 3] = [0x80, 0x00, 0x00];

    fn reset_session(config: PaperConfig, blend_on: bool) {
        let session = PdfSession::create();
        session.with_paper(|s| {
            s.config = config;
            s.blend_on = blend_on;
        });
        CURRENT.with(|c| *c.borrow_mut() = Some(session));
    }

    fn published() -> Option<String> {
        with(|s| s.published.clone())
    }

    #[test]
    fn the_first_live_frame_publishes_its_colour() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));
    }

    #[test]
    fn a_position_straddling_pages_blends_their_shares() {
        /
        /

        T
        H
        E

        r
        e
        g
        r
        e
        s
        s
        i
        o
        n
        :

        4
        0
        %

        p
        a
        g
        e

        1

        +

        6
        0
        %

        p
        a
        g
        e

        2

        r
        e
        a
        d
        s

        a
        s

        6
        0
        %

        o
        f

        p
        a
        g
        e

        2
        .
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        feed_frame(&uniform(2, 32, 32, WHITE));
        position(1.6);
        let want = pdf_paper::lerp(
            Rgb::new(CREAM[0], CREAM[1], CREAM[2]),
            Rgb::new(WHITE[0], WHITE[1], WHITE[2]),
            0.6,
        )
        .to_hex();
        assert_eq!(published().as_deref(), Some(want.as_str()));
    }

    #[test]
    fn resting_on_a_page_publishes_exactly_its_colour() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        feed_frame(&uniform(2, 32, 32, INK));
        position(2.0);
        assert_eq!(published().as_deref(), Some("#404040"));
    }

    #[test]
    fn an_artwork_frame_contributes_nothing_and_holds_the_published() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));

        /
        /

        A
        n

        a
        r
        t
        w
        o
        r
        k

        p
        a
        g
        e
        :

        s
        i
        x
        t
        e
        e
        n

        b
        a
        n
        d
        s
        ,

        n
        o

        b
        u
        c
        k
        e
        t

        r
        e
        a
        c
        h
        i
        n
        g

        t
        h
        e

        p
        a
        p
        e
        r

        s
        h
        a
        r
        e
        .
        let mut art = uniform(2, 32, 32, CREAM);
        for y in 0..32usize {
            for band in 0..16u8 {
                for x in (band as usize * 2)..(band as usize * 2 + 2) {
                    let i = (y * 32 + x) * 4;
                    art.data[i] = band.wrapping_mul(16);
                    art.data[i + 1] = 255 - band.wrapping_mul(15);
                    art.data[i + 2] = band.wrapping_mul(7).wrapping_add(3);
                }
            }
        }
        feed_frame(&art);
        assert_eq!(published().as_deref(), Some("#faf4e8"));
        assert!(!with(
            |s| s.palettes[0].contains(2) || s.palettes[1].contains(2)
        ));
    }

    #[test]
    fn an_area_flip_on_a_uniform_page_hands_over_without_a_gap() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));

        configure(
            true,
            PaperConfig {
                area: PaperArea::Edges,
                ..PaperConfig::default()
            },
        );
        /
        /

        O
        n
        e

        f
        r
        a
        m
        e

        f
        e
        d

        b
        o
        t
        h

        l
        a
        d
        d
        e
        r
        s
        :

        t
        h
        e

        f
        l
        i
        p

        r
        e
        s
        o
        l
        v
        e
        s

        o
        n

        t
        h
        e

        s
        p
        o
        t
        .
        assert_eq!(published().as_deref(), Some("#faf4e8"));
        assert!(with(|s| s.palettes[slot(PaperArea::Edges)].contains(1)));
        // A live frame under the new area keeps agreeing.
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));
    }

    /
    /
    /

    A

    4
    0
    ×
    4
    4

    f
    r
    a
    m
    e
    :

    c
    r
    e
    a
    m

    c
    e
    n
    t
    r
    e

    u
    n
    d
    e
    r

    a

    m
    a
    r
    o
    o
    n

    m
    a
    r
    g
    i
    n
    .
    fn split(page: u32) -> api::PaperFrame {
        let (w, h) = (40usize, 44usize);
        let mut data = vec![255u8; w * h * 4];
        for i in (0..data.len()).step_by(4) {
            data[i] = CREAM[0];
            data[i + 1] = CREAM[1];
            data[i + 2] = CREAM[2];
        }
        for y in 0..h {
            for x in 0..w {
                if y < 5 || y + 5 >= h || x < 5 || x + 5 >= w {
                    let i = (y * w + x) * 4;
                    data[i] = MAROON[0];
                    data[i + 1] = MAROON[1];
                    data[i + 2] = MAROON[2];
                }
            }
        }
        api::PaperFrame {
            page,
            width: w as u32,
            height: h as u32,
            data,
        }
    }

    #[test]
    fn an_area_flip_hands_over_in_both_directions_without_a_scroll() {
        /
        /

        T
        H
        E

        r
        e
        g
        r
        e
        s
        s
        i
        o
        n
        :

        a

        f
        l
        i
        p

        u
        s
        e
        d

        t
        o

        w
        a
        i
        t

        o
        n

        a
        n

        o
        f
        f
        s
        c
        r
        e
        e
        n

        r
        o
        u
        n
        d

        t
        r
        i
        p
        .
        reset_session(
            PaperConfig {
                area: PaperArea::Edges,
                ..PaperConfig::default()
            },
            true,
        );
        document_open("/fake/book.pdf", 10);
        feed_frame(&split(1));
        assert_eq!(published().as_deref(), Some("#800000"));
        feed_frame(&split(2)); // the scroll: other pages feed, position moves
        position(2.0);

        configure(true, PaperConfig::default()); // → WholePage
        assert_eq!(published().as_deref(), Some("#faf4e8"));

        configure(
            true,
            PaperConfig {
                area: PaperArea::Edges,
                ..PaperConfig::default()
            },
        );
        assert_eq!(published().as_deref(), Some("#800000"));
    }

    #[test]
    fn changing_edge_width_invalidates_only_the_edge_cache() {
        reset_session(
            PaperConfig {
                area: PaperArea::Edges,
                ..PaperConfig::default()
            },
            true,
        );
        document_open("/fake/book.pdf", 3);
        feed_frame(&split(1));

        let mut config = with(|s| s.config);
        config.edge_width += 1;
        configure(true, config);

        with(|s| {
            assert!(s.palettes[slot(PaperArea::WholePage)].contains(1));
            assert!(s.interim[slot(PaperArea::WholePage)].is_some());
            assert!(s.palettes[slot(PaperArea::Edges)].is_empty());
            assert!(s.interim[slot(PaperArea::Edges)].is_none());
            assert!(resolve(s).is_none());

            /
            /

            `
            c
            o
            n
            f
            i
            g
            u
            r
            e
            `

            q
            u
            e
            u
            e
            d

            t
            h
            e
            s
            e

            s
            a
            m
            p
            l
            e
            s
            ;

            c
            l
            e
            a
            r

            t
            h
            e

            b
            o
            o
            k
            k
            e
            e
            p
            i
            n
            g
            .
            s.clear_sampling();
            assert_eq!(lookahead_wants(s), vec![1, 2, 3]);
        });
    }

    #[test]
    fn blend_off_never_publishes() {
        reset_session(PaperConfig::default(), false);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published(), None);
    }

    #[test]
    fn turning_blend_off_clears_a_published_colour() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));
        configure(false, PaperConfig::default());
        assert_eq!(published(), None);
    }

    #[test]
    fn disposing_the_session_forgets_the_book() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        futures::executor::block_on(cur().dispose());
        assert_eq!(published(), None);
        assert!(with(
            |s| s.palettes[0].is_empty() && s.palettes[1].is_empty()
        ));
        assert!(with(|s| s.doc_path.is_none()));
        // A disposed session ignores every later paper call.
        cur().paper_position(3.0);
        cur().paper_document_open("/fake/other.pdf", 3);
        assert!(with(|s| s.doc_path.is_none()));
    }

    #[test]
    fn a_sample_that_outlives_its_session_is_dropped() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        let session = cur();
        let epoch = session.with_paper(|s| {
            s.start_sample(2);
            s.epoch
        });
        assert_eq!(session.paper_pending_samples(), 1);
        futures::executor::block_on(session.dispose());
        /
        /

        T
        h
        e

        d
        i
        s
        p
        o
        s
        e

        a
        b
        a
        n
        d
        o
        n
        e
        d

        t
        h
        e

        i
        n
        -
        f
        l
        i
        g
        h
        t

        s
        a
        m
        p
        l
        e
        .
        assert_eq!(session.paper_pending_samples(), 0);
        let frame = uniform(2, 32, 32, CREAM);
        assert!(!land_sample(&session, epoch, 2, Some(&frame)));
        assert!(with(|s| !s.palettes[0].contains(2)));
    }

    #[test]
    fn a_sample_from_before_a_reopen_is_dropped() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        let session = cur();
        let epoch = session.with_paper(|s| {
            s.start_sample(2);
            s.epoch
        });
        document_open("/fake/book.pdf", 10); // epoch moves on
        let frame = uniform(2, 32, 32, CREAM);
        assert!(!land_sample(&session, epoch, 2, Some(&frame)));
        assert_eq!(session.paper_pending_samples(), 0);
    }

    #[test]
    fn two_sessions_keep_two_palettes() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/a.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        let a = cur();
        reset_session(PaperConfig::default(), true);
        document_open("/fake/b.pdf", 10);
        feed_frame(&uniform(1, 32, 32, INK));
        let b = cur();
        assert_eq!(
            a.with_paper(|s| s.published.clone()).as_deref(),
            Some("#faf4e8")
        );
        assert_eq!(
            b.with_paper(|s| s.published.clone()).as_deref(),
            Some("#404040")
        );
        futures::executor::block_on(a.dispose());
        assert_eq!(
            b.with_paper(|s| s.published.clone()).as_deref(),
            Some("#404040")
        );
    }

    #[test]
    fn the_lookahead_names_the_pair_and_the_page_after() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(3, 32, 32, CREAM)); // page 3 known (a live frame)
        /
        /

        S
        e
        t

        t
        h
        e

        p
        o
        s
        i
        t
        i
        o
        n

        d
        i
        r
        e
        c
        t
        l
        y
        ;

        `
        p
        o
        s
        i
        t
        i
        o
        n
        (
        )
        `

        w
        o
        u
        l
        d

        m
        a
        r
        k

        p
        a
        g
        e
        s

        i
        n

        f
        l
        i
        g
        h
        t
        .
        with(|s| s.position = 3.0);
        let wants = with(|s| lookahead_wants(s));
        assert_eq!(wants, vec![4, 5]); // 3 is known; the pair's next page +1
    }

    #[test]
    fn the_lookahead_stops_at_the_last_page() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 3);
        with(|s| s.position = 3.0);
        assert_eq!(with(|s| lookahead_wants(s)), vec![3]); // page 3, nothing after
    }

    #[test]
    fn the_lookahead_is_quiet_when_blend_is_off() {
        reset_session(PaperConfig::default(), false);
        document_open("/fake/book.pdf", 10);
        with(|s| s.position = 1.0);
        assert!(with(|s| lookahead_wants(s)).is_empty());
    }

    #[test]
    fn an_unsampled_position_falls_back_to_the_interim() {
        /
        /

        A
        n

        e
        m
        p
        t
        y

        s
        t
        r
        e
        t
        c
        h

        o
        f

        t
        h
        e

        p
        a
        l
        e
        t
        t
        e
        :

        t
        h
        e

        f
        i
        r
        s
        t

        l
        i
        v
        e

        c
        o
        l
        o
        u
        r

        h
        o
        l
        d
        s
        .
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        position(4.2); // nothing sampled that far yet
        assert_eq!(published().as_deref(), Some("#faf4e8"));
    }
}
