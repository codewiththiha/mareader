/
/
!

F
u
l
l
-
t
e
x
t

s
e
a
r
c
h
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
'
s

i
n
-
p
r
o
c
e
s
s

i
n
d
e
x

o
v
e
r

i
t
s

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

/
/
!

e
x
t
r
a
c
t
e
d

t
e
x
t
.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use futures::stream::{self, StreamExt};
use serde::Deserialize;

use pdf_core::search::{PageText, SearchIndex, SearchItem};
use reader_core::search::SearchResponse;

use super::{PdfSession, no_session};
use crate::api::{self, EngineError};

/
/
/

P
a
g
e
s

e
x
t
r
a
c
t
e
d

c
o
n
c
u
r
r
e
n
t
l
y

p
e
r

t
u
r
n

w
h
i
l
e

t
h
e

i
n
d
e
x

i
s

b
u
i
l
t
.
pub const SEARCH_PAGE_CONCURRENCY: usize = 3;

/
/
/

T
h
e

i
d
e
n
t
i
t
y

o
f

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

a
n

i
n
d
e
x

b
e
l
o
n
g
s

t
o
.
struct IndexKey {
    identity: String,
    num_pages: u32,
}

/// One session's search state.
#[derive(Default)]
pub(crate) struct SearchState {
    index: SearchIndex,
    /// The document the session's open scoped the index to.
    scoped: Option<IndexKey>,
    /
    /
    /

    W
    h
    a
    t

    `
    i
    n
    d
    e
    x
    `

    h
    o
    l
    d
    s
    :

    t
    h
    e

    k
    e
    y

    i
    t

    w
    a
    s

    b
    u
    i
    l
    t

    f
    o
    r
    ,

    a
    n
    d

    t
    h
    e

    p
    a
    g
    e

    c
    o
    u
    n
    t
    .
    built: Option<(IndexKey, u32)>,
}

thread_local! {
    /// The last disposed session's finished index (see the module docs).
    static RETAINED: RefCell<Option<SearchState>> = const { RefCell::new(None) };
}

impl SearchState {
    /
    /
    /

    S
    c
    o
    p
    e

    t
    h
    e

    i
    n
    d
    e
    x

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

    b
    e
    i
    n
    g

    o
    p
    e
    n
    e
    d
    .
    pub(crate) fn scope(&mut self, fingerprint: Option<&str>, path: &str, num_pages: u32) {
        let identity = fingerprint.filter(|f| !f.is_empty()).unwrap_or(path);
        let scoped = (!identity.is_empty()).then(|| IndexKey {
            identity: identity.to_string(),
            num_pages,
        });
        self.scoped = scoped.clone();
        if self.adopted_count(num_pages).is_some() {
            return;
        }
        /
        /

        A

        r
        e
        t
        a
        i
        n
        e
        d

        i
        n
        d
        e
        x

        f
        o
        r

        t
        h
        i
        s

        e
        x
        a
        c
        t

        d
        o
        c
        u
        m
        e
        n
        t

        i
        s

        a
        d
        o
        p
        t
        e
        d
        ,

        o
        t
        h
        e
        r
        s

        d
        r
        o
        p
        p
        e
        d
        .
        let retained = RETAINED.with(|r| r.borrow_mut().take());
        match retained {
            Some(r) if scoped.is_some() && r.built.as_ref().map(|(k, _)| k) == scoped.as_ref() => {
                self.index = r.index;
                self.built = r.built;
            }
            _ => {
                self.index.clear();
                self.built = None;
            }
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
    d
    e
    x
    '
    s

    p
    a
    g
    e

    c
    o
    u
    n
    t

    w
    h
    e
    n

    b
    u
    i
    l
    t

    f
    o
    r

    t
    h
    i
    s

    s
    c
    o
    p
    e
    d

    d
    o
    c
    u
    m
    e
    n
    t
    .
    fn adopted_count(&self, num_pages: u32) -> Option<u32> {
        let scoped = self.scoped.as_ref()?;
        if scoped.num_pages != num_pages {
            return None;
        }
        let (built, indexed) = self.built.as_ref()?;
        (built == scoped && *indexed > 0 && !self.index.is_empty()).then_some(*indexed)
    }

    /
    /
    /

    R
    e
    m
    e
    m
    b
    e
    r

    w
    h
    a
    t

    a

    f
    i
    n
    i
    s
    h
    e
    d

    b
    u
    i
    l
    d

    p
    r
    o
    d
    u
    c
    e
    d
    .
    fn record_build(&mut self, num_pages: u32, indexed: u32) {
        self.built = self
            .scoped
            .clone()
            .filter(|key| key.num_pages == num_pages)
            .map(|key| (key, indexed));
    }

    pub(crate) fn query(&self, query: &str) -> SearchResponse {
        self.index.query(query)
    }
}

/
/
/

A

d
i
s
p
o
s
e
d

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
,

k
e
p
t

o
n
l
y

w
h
e
n

w
o
r
t
h

a
d
o
p
t
i
n
g
.
pub(crate) fn retain(state: SearchState) {
    if state.built.as_ref().is_some_and(|(_, n)| *n > 0) && !state.index.is_empty() {
        RETAINED.with(|r| *r.borrow_mut() = Some(state));
    }
}

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

r
e
a
l
m
'
s

r
e
t
a
i
n
e
d

i
n
d
e
x
:

a

t
e
x
t

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
.
pub fn drop_retained_search() {
    RETAINED.with(|r| *r.borrow_mut() = None);
}

/
/
/

`
{
o
k
:
t
r
u
e
,

p
a
g
e
,

i
t
e
m
s
}
`
:

e
n
g
i
n
e
.
e
x
t
r
a
c
t
P
a
g
e
T
e
x
t
.
struct PageTextPayload {
    page: u32,
    items: Vec<ItemPayload>,
}

#[derive(Debug, Deserialize)]
struct ItemPayload {
    #[serde(rename = "str")]
    text: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/
/
/

I
n
-
f
l
i
g
h
t

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

b
u
i
l
d
s

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
.
static BUILD_ACTIVE: AtomicU32 = AtomicU32::new(0);

pub(crate) fn search_build_active() -> u32 {
    BUILD_ACTIVE.load(Ordering::Relaxed)
}

struct BuildActiveGuard;

impl Drop for BuildActiveGuard {
    fn drop(&mut self) {
        BUILD_ACTIVE.fetch_sub(1, Ordering::Relaxed);
    }
}

/
/
/

E
x
t
r
a
c
t

e
v
e
r
y

p
a
g
e

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

d
o
c
u
m
e
n
t

i
n
t
o

I
T
S

i
n
d
e
x
.
pub(crate) async fn build(session: &PdfSession, num_pages: u32) -> Result<u32, EngineError> {
    if !session.is_live() {
        return Err(no_session());
    }
    api::require_pdf_reader()?;
    /
    /

    T
    h
    e

    b
    u
    i
    l
    d

    c
    a
    n

    b
    e

    m
    i
    d
    -
    f
    l
    i
    g
    h
    t

    w
    h
    e
    n

    a

    p
    a
    n
    e

    c
    l
    o
    s
    e
    s
    ,

    s
    o

    i
    t

    i
    s

    g
    a
    u
    g
    e
    d
    .
    BUILD_ACTIVE.fetch_add(1, Ordering::Relaxed);
    let _build_guard = BuildActiveGuard;
    if let Some(indexed) = session.with_search(|s| s.adopted_count(num_pages)) {
        return Ok(indexed);
    }
    session.with_search(|s| {
        s.index.clear();
        s.built = None;
    });
    if num_pages == 0 {
        session.with_search(|s| s.record_build(num_pages, 0));
        return Ok(0);
    }

    // One TURN = [`SEARCH_PAGE_CONCURRENCY`] pages extracted concurrently,
    // then the builder yields to the event loop.
    let mut indexed = 0u32;
    let mut cursor = 1u32;
    while cursor <= num_pages {
        let end = (cursor + SEARCH_PAGE_CONCURRENCY as u32 - 1).min(num_pages);
        let batch: Vec<u32> = (cursor..=end).collect();
        let extracted: Vec<Option<PageTextPayload>> = stream::iter(batch)
            .map(|page| async move {
                let value = session.extract_page_text(page).await?;
                api::resolve::<PageTextPayload>(value, "extractPageText").ok()
            })
            .buffer_unordered(SEARCH_PAGE_CONCURRENCY)
            .collect()
            .await;
        if !session.is_live() {
            return Err(no_session());
        }
        for p in extracted.into_iter().flatten() {
            let items = p
                .items
                .into_iter()
                .map(|it| SearchItem::new(it.text, it.x, it.y, it.w, it.h))
                .collect();
            session.with_search(|s| {
                s.index.add_page(PageText {
                    page: p.page,
                    items,
                })
            });
            indexed += 1;
        }
        cursor = end + 1;
    }
    session.with_search(|s| s.record_build(num_pages, indexed));
    Ok(indexed)
}

/
/

T
h
e

s
c
o
p
e

a
n
d

a
d
o
p
t

r
u
l
e
s

a
r
e

p
u
r
e

h
o
s
t

l
o
g
i
c
.
mod tests {
    use super::*;

    fn item(word: &str) -> SearchItem {
        SearchItem::new(word, 0.0, 0.0, 1.0, 1.0)
    }

    /
    /
    /

    A

    f
    i
    n
    i
    s
    h
    e
    d

    b
    u
    i
    l
    d

    f
    o
    r

    o
    n
    e

    p
    a
    g
    e
    ,

    s
    i
    m
    u
    l
    a
    t
    e
    d

    w
    i
    t
    h
    o
    u
    t

    t
    h
    e

    e
    n
    g
    i
    n
    e
    .
    fn built(fingerprint: Option<&str>, path: &str, num_pages: u32) -> SearchState {
        let mut s = SearchState::default();
        s.scope(fingerprint, path, num_pages);
        s.index.add_page(PageText {
            page: 1,
            items: vec![item("moby")],
        });
        s.record_build(num_pages, 1);
        s
    }

    #[test]
    fn reopening_the_same_book_adopts_the_retained_index() {
        drop_retained_search();
        retain(built(Some("fp-a"), "/shelf/book.pdf", 1));
        let mut next = SearchState::default();
        next.scope(Some("fp-a"), "/shelf/book.pdf", 1);
        assert_eq!(next.adopted_count(1), Some(1));
        assert_eq!(next.query("moby").total, 1);
        // Adopted means MOVED: the slot is empty again.
        assert!(RETAINED.with(|r| r.borrow().is_none()));
    }

    #[test]
    fn a_different_book_drops_the_retained_index() {
        drop_retained_search();
        retain(built(Some("fp-a"), "/shelf/book.pdf", 1));
        let mut next = SearchState::default();
        next.scope(Some("fp-b"), "/shelf/other.pdf", 1);
        assert_eq!(next.adopted_count(1), None);
        assert!(next.index.is_empty());
        assert!(RETAINED.with(|r| r.borrow().is_none()));
    }

    #[test]
    fn without_a_fingerprint_the_path_is_the_identity() {
        drop_retained_search();
        retain(built(None, "/shelf/book.pdf", 1));
        let mut next = SearchState::default();
        next.scope(None, "/shelf/book.pdf", 1);
        assert_eq!(next.adopted_count(1), Some(1));
        // Same address, different length: the page count guards the key.
        next.scope(None, "/shelf/book.pdf", 2);
        assert_eq!(next.adopted_count(2), None);
    }

    #[test]
    fn an_unfinished_build_is_never_retained() {
        drop_retained_search();
        let mut half = SearchState::default();
        half.scope(Some("fp-a"), "/shelf/book.pdf", 3);
        half.index.add_page(PageText {
            page: 1,
            items: vec![item("moby")],
        });
        retain(half);
        assert!(RETAINED.with(|r| r.borrow().is_none()));
    }

    #[test]
    fn two_sessions_hold_two_indexes() {
        drop_retained_search();
        let a = built(Some("fp-a"), "/a.pdf", 1);
        let mut b = SearchState::default();
        b.scope(Some("fp-b"), "/b.pdf", 1);
        b.index.add_page(PageText {
            page: 1,
            items: vec![item("ahab")],
        });
        b.record_build(1, 1);
        assert_eq!(a.query("moby").total, 1);
        assert_eq!(a.query("ahab").total, 0);
        assert_eq!(b.query("ahab").total, 1);
        assert_eq!(b.query("moby").total, 0);
    }
}
