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

f
r
a
m
e

p
a
r
s
e
r
;

t
h
e

c
o
l
o
u
r

d
e
c
i
s
i
o
n
s

l
i
v
e

i
n

/
/
!

`
c
r
a
t
e
:
:
b
a
c
k
d
r
o
p
`
.

use wasm_bindgen::JsValue;

use super::{EngineError, KEY_DATA, KEY_HEIGHT, KEY_OK, KEY_PAGE, KEY_WIDTH, reflect_get, resolve};

/
/
/

A

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
:

t
h
e

r
a
s
t
e
r

d
o
w
n
s
c
a
l
e
d

t
o

a

≤
9
6
p
x

l
o
n
g

e
d
g
e
.
pub use crate::types::PaperFrame;

/
/
/

T
h
e

s
h
a
p
e

a

f
r
a
m
e
l
e
s
s

e
r
r
o
r

e
n
v
e
l
o
p
e

d
e
s
e
r
i
a
l
i
s
e
s

i
n
t
o
.
struct Empty {}

/
/
/

P
a
r
s
e

a

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

f
r
a
m
e

p
a
y
l
o
a
d
.
pub(crate) fn parse_frame(value: &JsValue) -> Option<PaperFrame> {
    let ok = reflect_get(value, &KEY_OK)
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    let number = |name: &'static std::thread::LocalKey<JsValue>| -> Option<f64> {
        reflect_get(value, name).ok().and_then(|v| v.as_f64())
    };
    let page = number(&KEY_PAGE)? as u32;
    let width = number(&KEY_WIDTH)? as u32;
    let height = number(&KEY_HEIGHT)? as u32;
    let data = reflect_get(value, &KEY_DATA).ok()?;
    let data = js_sys::Uint8ClampedArray::from(data).to_vec();
    Some(PaperFrame {
        page,
        width,
        height,
        data,
    })
}

pub(crate) fn resolve_frame(value: JsValue, what: &str) -> Result<Option<PaperFrame>, EngineError> {
    if let Some(frame) = parse_frame(&value) {
        return Ok(Some(frame));
    }
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
    }
    `

    w
    i
    t
    h

    n
    o

    f
    r
    a
    m
    e

    i
    s

    "
    n
    o

    a
    n
    s
    w
    e
    r

    f
    o
    r

    t
    h
    i
    s

    p
    a
    g
    e
    "
    .
    let ok = reflect_get(&value, &KEY_OK)
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if ok {
        return Ok(None);
    }
    /
    /

    `
    {
    o
    k
    :
    f
    a
    l
    s
    e
    ,

    e
    r
    r
    o
    r
    }
    `
    :

    s
    u
    r
    f
    a
    c
    e

    i
    t

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
    h
    a
    r
    e
    d

    e
    r
    r
    o
    r

    p
    a
    t
    h
    .
    resolve::<Empty>(value, what)?;
    Ok(None)
}
