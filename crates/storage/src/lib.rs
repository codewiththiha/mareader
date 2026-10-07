/
/
!

P
e
r
s
i
s
t
e
d

a
p
p

s
t
a
t
e

o
v
e
r

l
o
c
a
l
S
t
o
r
a
g
e
,

p
l
u
s

`
k
e
p
t
`
.

pub mod kept;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use ai_core::gloss::GlossMark;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

use runtime_contract::covers::{CoverImage, CoverMap};
/
/

T
h
e

l
i
b
r
a
r
y
'
s

k
e
y
s
,

s
h
a
p
e

a
n
d

m
i
g
r
a
t
i
o
n

l
i
v
e

i
n

`
l
i
b
r
a
r
y
_
c
o
r
e
:
:
b
l
o
b
`
.
use library_core::blob::migrate::{BlobV2, LEGACY_KEY, RecentBook, V2_KEY, migrate_v1, migrate_v2};
use library_core::blob::sanitize as sanitize_library;
use library_core::blob::{LIBRARY_KEY, LibraryBlob, RETIRED_LIBRARY_KEY};
use reader_core::settings::{RETIRED_SETTINGS_KEY, SETTINGS_KEY, Settings, sanitize};

const COVERS_KEY: &str = "mareader.covers.v1";

/
/
/

T
h
e

k
e
y

t
h
e

a
p
p

r
e
a
d

b
e
f
o
r
e

i
t

w
a
s

r
e
n
a
m
e
d
.
const RETIRED_COVERS_KEY: &str = "pdfreader.covers.v1";

/
/
/

G
l
o
s
s

h
i
g
h
l
i
g
h
t
s
,

k
e
y
e
d

b
y

t
h
e

R
O
W

I
D

t
h
e

l
i
b
r
a
r
y

h
o
l
d
s
.
const GLOSS_KEY: &str = "mareader.gloss.v2";

/// [`GLOSS_KEY`] before the rename.
const RETIRED_GLOSS_KEY: &str = "pdfreader.gloss.v2";

/
/
/

T
h
e

a
d
d
r
e
s
s
-
k
e
y
e
d

m
a
p

t
h
i
s

b
u
i
l
d

m
i
g
r
a
t
e
d

f
r
o
m
.
const GLOSS_V1_KEY: &str = "pdfreader.gloss.v1";

/
/
/

O
n
e
-
s
h
o
t

g
a
t
e

f
o
r

t
h
e

a
d
d
r
e
s
s
-
t
o
-
r
o
w

m
i
g
r
a
t
i
o
n
.
const GLOSS_V2_MIGRATED_KEY: &str = "mareader.gloss.v2.migrated";

/
/
/

T
h
e

g
a
t
e

a
s

t
h
e

p
r
e
-
r
e
b
r
a
n
d

b
u
i
l
d

s
e
t

i
t
.
const RETIRED_GLOSS_V2_MIGRATED_KEY: &str = "pdfreader.gloss.v2.migrated";

/
/
/

A

p
e
r
s
i
s
t
e
n
c
e

f
a
i
l
u
r
e
:

q
u
o
t
a
,

b
l
o
c
k
e
d

s
t
o
r
a
g
e
,

s
e
r
i
a
l
i
z
a
t
i
o
n
.
pub struct StorageError {
    op: &'static str,
    detail: String,
}

impl StorageError {
    /
    /
    /

    S
    u
    r
    f
    a
    c
    e

    t
    h
    e

    f
    a
    i
    l
    u
    r
    e

    o
    n

    t
    h
    e

    c
    o
    n
    s
    o
    l
    e

    w
    i
    t
    h
    o
    u
    t

    i
    n
    t
    e
    r
    r
    u
    p
    t
    i
    n
    g

    t
    h
    e

    U
    I
    .
    pub fn report(&self) {
        #[cfg(target_arch = "wasm32")]
        web_sys::console::warn_1(&JsValue::from_str(&format!("[storage] {self}")));
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.op, self.detail)
    }
}

fn warn(op: &'static str, detail: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::warn_1(&JsValue::from_str(&format!("[storage] {op}: {detail}")));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (op, detail);
}

/
/
/

T
h
e

b
r
o
w
s
e
r
'
s

o
w
n

k
e
y
-
v
a
l
u
e

s
t
o
r
e
,

`
N
o
n
e
`

o
f
f

w
a
s
m
.
fn local() -> Option<web_sys::Storage> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()?.local_storage().ok()?
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/
/
/

R
e
a
d

a

r
a
w

J
S
O
N

b
l
o
b
,

i
f

p
r
e
s
e
n
t

a
n
d

r
e
a
d
a
b
l
e
.
pub(crate) fn get(key: &str) -> Option<String> {
    local().and_then(|s| s.get_item(key).ok().flatten())
}

/// Write a raw JSON blob. Quota/security failures surface as an error.
pub(crate) fn set(key: &str, value: &str) -> Result<(), StorageError> {
    let storage = local().ok_or_else(|| StorageError {
        op: "set",
        detail: "localStorage unavailable".to_string(),
    })?;
    storage.set_item(key, value).map_err(|e| StorageError {
        op: "set",
        detail: e.as_string().unwrap_or_else(|| "unknown error".to_string()),
    })
}

/
/
/

S
e
r
i
a
l
i
z
e

f
o
r

s
t
o
r
a
g
e
,

n
a
m
i
n
g

t
h
e

o
p
e
r
a
t
i
o
n

o
n

f
a
i
l
u
r
e
.
fn encode<T: serde::Serialize + ?Sized>(
    op: &'static str,
    value: &T,
) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|e| StorageError {
        op,
        detail: format!("serialize failed: {e}"),
    })
}

/
/
/

R
e
a
d

a

s
t
o
r
e

u
n
d
e
r

i
t
s

c
u
r
r
e
n
t

k
e
y
,

f
a
l
l
i
n
g

b
a
c
k

t
o

t
h
e

r
e
t
i
r
e
d

o
n
e
.
fn load_keyed<T: serde::de::DeserializeOwned + Default>(
    op: &'static str,
    key: &str,
    retired: &str,
) -> T {
    get(key)
        .or_else(|| get(retired))
        .map(|raw| parse(op, &raw))
        .unwrap_or_default()
}

fn parse<T: serde::de::DeserializeOwned + Default>(op: &'static str, raw: &str) -> T {
    match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            /
            /

            C
            o
            r
            r
            u
            p
            t

            s
            t
            a
            t
            e

            m
            u
            s
            t

            n
            o
            t

            b
            r
            i
            c
            k

            t
            h
            e

            a
            p
            p
            ,

            n
            o
            r

            d
            i
            s
            a
            p
            p
            e
            a
            r

            s
            i
            l
            e
            n
            t
            l
            y
            .
            warn(op, &format!("invalid JSON, falling back to default ({e})"));
            T::default()
        }
    }
}

/// Load persisted settings; invalid values fall back to defaults + sanitize.
pub fn load_settings() -> Settings {
    let mut settings: Settings = load_keyed("settings", SETTINGS_KEY, RETIRED_SETTINGS_KEY);
    sanitize(&mut settings);
    settings
}

pub fn save_settings(settings: &Settings) -> Result<(), StorageError> {
    set(SETTINGS_KEY, &encode("save_settings", settings)?)
}

/
/
/

L
o
a
d

t
h
e

l
i
b
r
a
r
y
,

m
i
g
r
a
t
i
n
g

t
h
e

p
r
e
v
i
o
u
s

s
c
h
e
m
a
'
s

b
l
o
b
.
pub fn load_library() -> LibraryBlob {
    if let Some(raw) = get(LIBRARY_KEY) {
        let mut blob: LibraryBlob = parse("library", &raw);
        sanitize_library(&mut blob);
        return blob;
    }
    if let Some(raw) = get(RETIRED_LIBRARY_KEY) {
        let mut blob: LibraryBlob = parse("library", &raw);
        sanitize_library(&mut blob);
        return blob;
    }
    if let Some(raw) = get(V2_KEY) {
        let legacy: BlobV2 = parse("library v2", &raw);
        let mut blob = migrate_v2(legacy);
        sanitize_library(&mut blob);
        return blob;
    }
    let legacy: Vec<RecentBook> = get(LEGACY_KEY)
        .map(|raw| parse("library v1", &raw))
        .unwrap_or_default();
    if legacy.is_empty() {
        return LibraryBlob::default();
    }
    let mut blob = migrate_v1(legacy, runtime_contract::time::now_ms());
    sanitize_library(&mut blob);
    blob
}

pub fn save_library(blob: &LibraryBlob) -> Result<(), StorageError> {
    set(LIBRARY_KEY, &encode("save_library", blob)?)
}

/
/
/

A

c
h
e
a
p

i
d
e
n
t
i
t
y

f
o
r

o
n
e

s
t
o
r
e
'
s

c
u
r
r
e
n
t

c
o
n
t
e
n
t
s
:

a

h
a
s
h
.
fn stamp_of(key: &str) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let raw = get(key)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    raw.len().hash(&mut hasher);
    raw.hash(&mut hasher);
    Some(hasher.finish())
}

/// The library blob's stamp (see [`stamp_of`]).
pub fn library_stamp() -> Option<u64> {
    stamp_of(LIBRARY_KEY)
}

/// The cover map's stamp (see [`stamp_of`]).
pub fn covers_stamp() -> Option<u64> {
    stamp_of(COVERS_KEY)
}

/// The settings blob's stamp (see [`stamp_of`]).
pub fn settings_stamp() -> Option<u64> {
    stamp_of(SETTINGS_KEY)
}

/// Load the cover-art map (path -> page-1 JPEG data URL).
pub fn load_covers() -> CoverMap {
    let stored: HashMap<String, CoverImage> = load_keyed("covers", COVERS_KEY, RETIRED_COVERS_KEY);
    stored
        .into_iter()
        .map(|(path, cover)| (path, Arc::new(cover)))
        .collect()
}

/
/
/

S
a
v
e

t
h
e

c
o
v
e
r
-
a
r
t

m
a
p
,

t
h
r
o
u
g
h

a

m
a
p

o
f

B
O
R
R
O
W
E
D

c
o
v
e
r
s
.
pub fn save_covers(covers: &CoverMap) -> Result<(), StorageError> {
    let borrowed: HashMap<&str, &CoverImage> = covers
        .iter()
        .map(|(path, cover)| (path.as_str(), cover.as_ref()))
        .collect();
    set(COVERS_KEY, &encode("save_covers", &borrowed)?)
}

/
/
/

A
p
p
l
y

a

r
e
a
d

p
o
i
n
t

t
o

t
h
e

p
e
r
s
i
s
t
e
d

l
i
b
r
a
r
y

b
l
o
b
.
pub fn apply_read_point(point: &runtime_contract::boundary::ReadPoint) {
    let mut blob = load_library();
    /
    /

    T
    h
    e

    s
    a
    m
    e

    r
    e
    c
    o
    r
    d
    e
    r

    t
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

    t
    a
    i
    l

    u
    s
    e
    d
    :

    i
    t

    m
    i
    n
    t
    s

    a

    l
    i
    n
    k
    e
    d

    r
    o
    w
    .
    let lib_point = library_core::book::ReadPoint {
        page: point.page,
        num_pages: point.num_pages,
        fraction: point.fraction,
    };
    library_core::book::record_read(
        &mut blob.books,
        point.book_id.as_deref(),
        &point.path,
        point.title.clone(),
        point.author.clone(),
        lib_point,
        runtime_contract::time::now_ms(),
    );
    let _ = save_library(&blob);
}

/
/
/

C
a
r
r
y

a
d
d
r
e
s
s
-
k
e
y
e
d

h
i
g
h
l
i
g
h
t
s

o
n
t
o

t
h
e

r
o
w
s

t
h
a
t

r
e
a
d

t
h
e
m
.
pub fn migrate_gloss_keys(books: &[library_core::book::Row]) {
    if get(GLOSS_V2_MIGRATED_KEY)
        .or_else(|| get(RETIRED_GLOSS_V2_MIGRATED_KEY))
        .is_some()
    {
        return;
    }
    let Some(raw) = get(GLOSS_V1_KEY) else {
        return;
    };
    let old: HashMap<String, Vec<GlossMark>> = parse("gloss v1", &raw);
    if old.is_empty() {
        return;
    }
    let mut carried = load_gloss();
    for (key, marks) in old {
        if marks.is_empty() {
            continue;
        }
        let id = match key.split_once("::") {
            /
            /

            A

            p
            r
            i
            v
            a
            t
            e

            r
            o
            w
            '
            s

            o
            w
            n

            l
            i
            s
            t
            :

            r
            e
            -
            k
            e
            y
            e
            d

            o
            n
            t
            o

            t
            h
            e

            i
            d
            .
            Some((id, _)) => library_core::book::find_by_id(books, id)
                .map(|b| b.id.clone())
                .unwrap_or_else(|| id.to_string()),
            // The list every shared row at this address read.
            None => library_core::book::book_rows(books)
                .find(|b| b.path() == key && !b.independent)
                .map(|b| b.id.clone())
                .unwrap_or_default(),
        };
        if id.is_empty() {
            continue;
        }
        /
        /

        T
        w
        o

        o
        l
        d

        k
        e
        y
        s

        c
        a
        n

        l
        a
        n
        d

        o
        n

        o
        n
        e

        r
        o
        w
        :

        t
        h
        e

        m
        a
        r
        k
        s

        a
        r
        e

        u
        n
        i
        o
        n
        e
        d
        .
        let existing = carried.entry(id).or_default();
        for mark in marks {
            if !existing.iter().any(|kept| kept.same_spot(&mark)) {
                existing.push(mark);
            }
        }
    }
    if let Err(e) = save_gloss(&carried) {
        e.report();
        return;
    }
    if let Err(e) = set(GLOSS_V2_MIGRATED_KEY, "1") {
        e.report();
    }
}

/// Load every book's gloss highlights, keyed by row id.
pub fn load_gloss() -> HashMap<String, Vec<GlossMark>> {
    load_keyed("gloss", GLOSS_KEY, RETIRED_GLOSS_KEY)
}

fn save_gloss(all: &HashMap<String, Vec<GlossMark>>) -> Result<(), StorageError> {
    set(GLOSS_KEY, &encode("save_gloss", all)?)
}

/
/
/

D
r
o
p

o
n
e

r
o
w
'
s

m
a
r
k
s
:

t
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

d
a
t
a

g
o
e
s

w
i
t
h

t
h
e

b
o
o
k
.
pub fn remove_gloss(row_id: &str) {
    take_gloss(row_id);
}

/
/
/

T
a
k
e

o
n
e

r
o
w
'
s

m
a
r
k
s

o
u
t

o
f

t
h
e

s
t
o
r
e
.
pub fn take_gloss(row_id: &str) -> Vec<GlossMark> {
    let mut all = load_gloss();
    let Some(marks) = all.remove(row_id) else {
        return Vec::new();
    };
    if let Err(e) = save_gloss(&all) {
        e.report();
    }
    marks
}

/
/
/

R
e
p
l
a
c
e

o
n
e

r
o
w
'
s

m
a
r
k
s

a
n
d

w
r
i
t
e

t
h
e

w
h
o
l
e

m
a
p

b
a
c
k
.
pub fn persist_gloss(row_id: &str, marks: &[GlossMark]) {
    let mut all = load_gloss();
    all.insert(row_id.to_string(), marks.to_vec());
    if let Err(e) = save_gloss(&all) {
        e.report();
    }
}

/
/
/

O
n
e

r
o
w
'
s

m
a
r
k
s

c
r
o
s
s
i
n
g

t
h
e

r
e
a
d
e
r

t
o

S
h
e
l
l

b
o
u
n
d
a
r
y
,

a
s

J
S
O
N
.
pub fn encode_gloss(marks: &[GlossMark]) -> Result<String, StorageError> {
    encode("encode_gloss", marks)
}

fn decode_gloss(encoded: &str) -> Result<Vec<GlossMark>, StorageError> {
    serde_json::from_str(encoded).map_err(|e| StorageError {
        op: "decode_gloss",
        detail: format!("parse failed: {e}"),
    })
}

/
/
/

T
h
e

w
r
i
t
e
r
'
s

h
a
l
f

o
f

`
e
n
c
o
d
e
_
g
l
o
s
s
`
:

d
e
c
o
d
e
,

t
h
e
n

p
e
r
s
i
s
t
.
pub fn persist_encoded_gloss(row_id: &str, encoded: &str) {
    match decode_gloss(encoded) {
        Ok(marks) => persist_gloss(row_id, &marks),
        Err(e) => e.report(),
    }
}

/
/
/

O
n
e

r
o
w
'
s

m
a
r
k
s
,

c
o
p
i
e
d

o
n
t
o

a
n
o
t
h
e
r

r
o
w
.
pub fn copy_gloss(from_id: &str, to_id: &str) {
    let mut all = load_gloss();
    let Some(marks) = all.get(from_id) else {
        return;
    };
    if marks.is_empty() {
        return;
    }
    all.insert(
        to_id.to_string(),
        re_ided(marks, runtime_contract::time::now_ms()),
    );
    if let Err(e) = save_gloss(&all) {
        e.report();
    }
}

/
/
/

T
h
e

m
a
r
k
s

a

d
u
p
l
i
c
a
t
e

w
e
a
r
s
,

u
n
d
e
r

f
r
e
s
h
l
y

m
i
n
t
e
d

i
d
s
.
fn re_ided(marks: &[GlossMark], now_ms: u64) -> Vec<GlossMark> {
    marks
        .iter()
        .enumerate()
        .map(|(at, mark)| GlossMark {
            id: ai_core::gloss::mark_id(mark.anchor.page, now_ms + at as u64),
            ..mark.clone()
        })
        .collect()
}

/
/
/

B
u
i
l
d

a

r
e
a
d
e
r

l
a
u
n
c
h

d
e
s
c
r
i
p
t
o
r

f
o
r

a
n

i
n
-
s
e
s
s
i
o
n

o
p
e
n
.
pub fn resolve_launch(path: &str) -> Option<runtime_contract::boundary::LaunchDocument> {
    use library_core::book::resume_point;
    let blob = load_library();
    let book_id = library_core::book::book_rows(&blob.books)
        .find(|b| b.path() == path && !b.independent)
        .map(|b| b.id.clone());
    let (resume_page, saved_fraction) = resume_point(&blob.books, book_id.as_deref(), path);
    let display_name = book_id
        .as_deref()
        .and_then(|id| library_core::book::find_by_id(&blob.books, id))
        .map(|b| b.title());
    Some(runtime_contract::boundary::LaunchDocument {
        book_id,
        path: path.to_string(),
        resume_page,
        saved_fraction,
        blend_override: false,
        cover_data_url: None,
        display_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_core::gloss::{GlossBox, PageAnchor};

    fn mark(id: &str, word: &str, page: u32) -> GlossMark {
        GlossMark {
            id: id.to_string(),
            word: word.to_string(),
            context: "the sentence it stood in".to_string(),
            anchor: PageAnchor {
                page,
                rect: GlossBox {
                    x: 10.0,
                    y: 20.0,
                    w: 30.0,
                    h: 8.0,
                    r: 0.0,
                },
            },
        }
    }

    #[test]
    fn a_copied_list_keeps_its_spots_and_mints_its_own_ids() {
        let marks = vec![mark("g3-1", "palimpsest", 3), mark("g3-2", "sietch", 3)];
        let fresh = re_ided(&marks, 1_700);
        assert_eq!(fresh.len(), 2);
        for (old, new) in marks.iter().zip(&fresh) {
            assert_eq!(new.word, old.word, "the explained word travels");
            assert_eq!(new.context, old.context, "the context travels");
            assert_eq!(new.anchor, old.anchor, "the spot travels");
            assert_ne!(new.id, old.id, "the id does not");
        }
        assert_ne!(fresh[0].id, fresh[1].id, "two marks on one page differ");
        assert_eq!(
            fresh[0].id, "g3-1700",
            "the scheme the capture sites mint is the scheme the copy mints"
        );
        assert_eq!(fresh[1].id, "g3-1701", "the index keeps the stamps apart");
    }

    #[test]
    fn a_list_survives_the_boundary_encoding_and_garbage_does_not_decode() {
        let marks = vec![mark("g3-1", "palimpsest", 3), mark("g9-2", "sietch", 9)];
        let encoded = encode_gloss(&marks).expect("a mark list encodes");
        assert_eq!(decode_gloss(&encoded).expect("and decodes"), marks);
        assert!(
            decode_gloss("[]")
                .expect("an emptied list decodes")
                .is_empty()
        );
        assert!(
            decode_gloss("{\"not\":\"a list\"}").is_err(),
            "a malformed list is refused, so it can never be written"
        );
    }

    #[test]
    fn an_empty_list_never_reaches_storage() {
        /
        /

        T
        h
        e

        g
        u
        a
        r
        d

        t
        h
        e

        c
        a
        l
        l
        e
        r

        r
        i
        d
        e
        s
        :

        n
        o
        t
        h
        i
        n
        g

        t
        o

        c
        o
        p
        y

        w
        r
        i
        t
        e
        s

        n
        o
        t
        h
        i
        n
        g
        .
        assert!(re_ided(&[], 5).is_empty());
    }
}
