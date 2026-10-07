/
/
!

S
e
r
d
e

t
y
p
e
s

m
i
r
r
o
r
i
n
g

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

r
e
t
u
r
n

s
h
a
p
e
s
.

use serde::{Deserialize, Serialize};

pub use reader_core::document::{DocStatus, PageSize};

/
/
/

`
{
o
k
,

w
i
d
t
h
,

h
e
i
g
h
t
}
`
:

o
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
.
pub struct PageSizeResult {
    pub width: f64,
    pub height: f64,
}

/
/
/

O
n
e

f
l
a
t
t
e
n
e
d

c
h
a
p
t
e
r
,

e
x
a
c
t
l
y

a
s

t
h
e

e
n
g
i
n
e

r
e
s
o
l
v
e
s

i
t
.
pub use pdf_core::outline::OutlineEntry;

/
/
/

`
{
o
k
,

n
u
m
P
a
g
e
s
,

t
i
t
l
e
,

a
u
t
h
o
r
,

f
i
n
g
e
r
p
r
i
n
t
,

o
u
t
l
i
n
e
,

.
.
.
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
o
p
e
n
(
)
.
pub struct OpenResult {
    pub num_pages: u32,
    pub title: Option<String>,
    pub author: Option<String>,
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

    p
    e
    r
    m
    a
    n
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

    f
    i
    n
    g
    e
    r
    p
    r
    i
    n
    t
    ,

    t
    h
    e

    i
    n
    d
    e
    x
    '
    s

    k
    e
    y
    .
    pub fingerprint: Option<String>,
    pub outline: Vec<OutlineEntry>,
    pub page1_size: PageSize,
    /
    /
    /

    I
    n
    t
    r
    i
    n
    s
    i
    c

    h
    e
    i
    g
    h
    t

    o
    f

    e
    v
    e
    r
    y

    p
    a
    g
    e
    ,

    i
    n

    d
    o
    c
    u
    m
    e
    n
    t

    o
    r
    d
    e
    r
    .
    pub page_heights: Vec<f64>,
    /
    /
    /

    I
    n
    t
    r
    i
    n
    s
    i
    c

    w
    i
    d
    t
    h

    o
    f

    e
    v
    e
    r
    y

    p
    a
    g
    e
    ,

    i
    n

    d
    o
    c
    u
    m
    e
    n
    t

    o
    r
    d
    e
    r
    .
    pub page_widths: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/
/
/

T
h
e

o
l
d

`
c
a
c
h
e
d
`

f
l
a
g
,

l
e
f
t

u
n
r
e
a
d
;

`
h
a
s
_
t
h
u
m
b
`

r
e
p
l
a
c
e
d

i
t
.
pub struct ThumbResult {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/
/
/

`
{
o
k
,

d
a
t
a
U
r
l
,

w
i
d
t
h
,

h
e
i
g
h
t
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
c
o
v
e
r
D
a
t
a
U
r
l
.
pub struct CoverResult {
    pub data_url: String,
    pub width: f64,
    pub height: f64,
}

/
/
/

`
{
o
k
,

p
a
g
e
,

w
i
d
t
h
,

h
e
i
g
h
t
,

d
a
t
a
}
`
:

t
h
e

r
a
w

p
a
g
e

f
r
a
m
e
.
pub struct PaperFrame {
    pub page: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}
