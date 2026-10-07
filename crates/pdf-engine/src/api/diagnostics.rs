/
/
!

E
n
g
i
n
e

l
i
f
e
c
y
c
l
e

a
n
d

r
e
s
o
u
r
c
e

c
o
u
n
t
e
r
s
:

t
h
e

d
i
a
g
n
o
s
t
i
c
s

s
n
a
p
s
h
o
t
'
s

/
/
!

e
n
g
i
n
e

h
a
l
f
.

pub use pdf_core::diagnostics::EngineStats;

/
/
/

R
e
a
d

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

c
o
u
n
t
e
r
s
;

`
N
o
n
e
`

o
f
f
-
w
a
s
m

o
r

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
pub fn engine_stats() -> Option<EngineStats> {
    if !crate::bridge::has_pdf_reader() {
        return None;
    }
    let mut stats: EngineStats = serde_wasm_bindgen::from_value(crate::bridge::stats()).ok()?;
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

    g
    a
    u
    g
    e

    l
    i
    v
    e
    s

    o
    n

    t
    h
    e

    R
    u
    s
    t

    s
    i
    d
    e
    ,

    s
    o

    i
    t

    i
    s

    f
    o
    l
    d
    e
    d

    i
    n

    h
    e
    r
    e
    .
    stats.search_active = crate::session::search::search_build_active();
    Some(stats)
}

/
/
/

T
u
r
n

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

l
i
f
e
c
y
c
l
e

n
a
r
r
a
t
i
o
n

o
n

o
r

o
f
f
.
pub fn set_lifecycle_log(on: bool) {
    if crate::bridge::has_pdf_reader() {
        crate::bridge::set_lifecycle_log(on);
    }
}
