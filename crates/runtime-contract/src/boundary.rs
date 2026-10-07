/
/
!

T
h
e

s
e
r
i
a
l
i
z
e
d

b
o
u
n
d
a
r
y

b
e
t
w
e
e
n

t
h
e

S
h
e
l
l

a
n
d

t
h
e

r
u
n
t
i
m
e
s
.

use reader_core::settings::Settings;
use serde::{Deserialize, Serialize};

/
/
/

T
h
e

m
i
n
i
m
a
l

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

t
h
e

S
h
e
l
l

h
a
n
d
s

a

s
t
a
r
t
i
n
g

r
e
a
d
e
r
.
pub struct LaunchDocument {
    /
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

    r
    o
    w

    t
    h
    e

    o
    p
    e
    n

    w
    a
    s

    n
    a
    m
    e
    d

    b
    y
    ,

    w
    h
    e
    n

    i
    t

    w
    a
    s
    .
    pub book_id: Option<String>,
    pub path: String,
    /
    /
    /

    W
    h
    e
    r
    e

    t
    o

    r
    e
    s
    u
    m
    e
    :

    t
    h
    e

    p
    a
    g
    e

    a
    n
    d

    t
    h
    e

    s
    t
    r
    e
    a
    m
    '
    s

    f
    r
    a
    c
    t
    i
    o
    n
    .
    pub resume_page: u32,
    #[serde(default)]
    pub saved_fraction: Option<f64>,
    /
    /
    /

    T
    h
    e

    t
    e
    s
    t

    h
    o
    o
    k
    '
    s

    `
    ?
    b
    l
    e
    n
    d
    =
    1
    `

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

    w
    r
    i
    t
    e
    .
    pub blend_override: bool,
    /
    /
    /

    T
    h
    e

    b
    o
    o
    k
    '
    s

    c
    o
    v
    e
    r
    ,

    r
    e
    s
    o
    l
    v
    e
    d

    b
    y

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
    .
    pub cover_data_url: Option<String>,
    /
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

    d
    i
    s
    p
    l
    a
    y

    n
    a
    m
    e

    f
    o
    r

    t
    h
    i
    s

    o
    p
    e
    n
    '
    s

    r
    o
    w
    .
    pub display_name: Option<String>,
}

/
/
/

A

d
u
r
a
b
l
e

w
r
i
t
e
-
t
h
r
o
u
g
h
:

w
h
e
r
e

t
h
e

r
e
a
d
e
r

g
o
t

t
o
.
pub struct ReadPoint {
    #[serde(default)]
    pub book_id: Option<String>,
    pub path: String,
    pub page: u32,
    #[serde(default)]
    pub num_pages: u32,
    #[serde(default)]
    pub fraction: Option<f64>,
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

    t
    i
    t
    l
    e

    a
    n
    d

    a
    u
    t
    h
    o
    r
    ,

    w
    h
    e
    n

    a

    r
    o
    w

    m
    a
    y

    b
    e

    m
    i
    n
    t
    e
    d
    .
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
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

s
t
a
t
u
s

a
s

t
h
e

r
e
a
d
e
r

s
e
s
s
i
o
n

r
e
p
o
r
t
s

i
t
.
pub struct DocStatusReport {
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
}

/
/
/

T
h
e

t
y
p
e
d

c
o
m
m
a
n
d

s
u
r
f
a
c
e

a

r
u
n
t
i
m
e

c
a
l
l
s

t
h
e

S
h
e
l
l

t
h
r
o
u
g
h
.
pub trait ShellApi {
    /
    /
    /

    L
    i
    b
    r
    a
    r
    y

    t
    o

    S
    h
    e
    l
    l
    :

    o
    p
    e
    n

    t
    h
    i
    s

    d
    o
    c
    u
    m
    e
    n
    t
    .
    fn open_document(&self, launch: &LaunchDocument);
    /
    /
    /

    R
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
    :

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

    h
    a
    n
    d
    i
    n
    g

    c
    o
    n
    t
    r
    o
    l

    b
    a
    c
    k
    .
    fn navigate_library(&self);
    /// Reader → Shell: a durable read-point write (debounce or flush).
    fn read_point(&self, point: &ReadPoint);
    /// Runtime → Shell: persist the settings blob (the Shell owns the key).
    fn save_settings(&self, settings: &Settings);
    /
    /
    /

    R
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
    :

    o
    n
    e

    g
    e
    n
    e
    r
    a
    t
    e
    d

    c
    o
    v
    e
    r
    .
    fn save_cover(&self, path: &str, image: &crate::covers::CoverImage);
    /
    /
    /

    R
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
    :

    p
    e
    r
    s
    i
    s
    t

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
    '
    s

    g
    l
    o
    s
    s

    m
    a
    r
    k
    s
    .
    fn save_gloss(&self, key: &str, marks: String);
    /
    /
    /

    L
    i
    b
    r
    a
    r
    y

    t
    o

    S
    h
    e
    l
    l
    :

    b
    a
    k
    e

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

    i
    n
    t
    o

    c
    o
    v
    e
    r

    a
    r
    t
    .
    fn bake_cover(&self, path: &str);
    /// Reader → Shell: the document status changed (URL policy + probe).
    fn doc_status(&self, report: &DocStatusReport);
    /// Reader → Shell: the diagnostics digest (the Shell's probe merges it).
    fn publish_digest(&self, json: String);
}
