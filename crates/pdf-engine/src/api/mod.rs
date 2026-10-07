/
/
!

T
h
e

r
e
a
l
m
-
l
e
v
e
l

h
a
l
f

o
f

t
h
e

e
n
g
i
n
e

s
u
r
f
a
c
e
,

p
l
u
s

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
n
v
e
l
o
p
e

/
/
!

p
a
r
s
e
r
.

use serde::de::DeserializeOwned;
use std::thread::LocalKey;
use wasm_bindgen::JsValue;

pub mod diagnostics;
pub mod paper;
pub mod theme;

pub use diagnostics::{EngineStats, engine_stats, set_lifecycle_log};
pub use paper::PaperFrame;
pub use theme::{refresh_theme, set_appearance_menu_open, set_scrub_mode};

/
/
/

E
r
r
o
r

f
r
o
m

a
n
y

e
n
g
i
n
e

c
a
l
l
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
'
s

n
a
m
e

a
n
d

m
e
s
s
a
g
e
,

o
r

a

/
/
/

l
o
c
a
l

f
a
i
l
u
r
e

t
o

p
a
r
s
e
.
pub struct EngineError {
    pub name: String,
    pub message: String,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name, self.message)
    }
}

/
/
/

E
n
g
i
n
e

e
r
r
o
r
s

a
r
e

t
o
a
s
t

t
e
x
t
;

t
h
e

c
o
n
v
e
r
s
i
o
n

a
v
o
i
d
s

c
l
o
n
i
n
g
.
impl From<EngineError> for String {
    fn from(e: EngineError) -> Self {
        e.to_string()
    }
}

/
/
/

A

n
o
n
-
s
t
r
i
n
g

e
r
r
o
r

f
i
e
l
d

s
h
o
w
s

i
t
s

d
e
b
u
g

f
o
r
m
.
fn js_str(v: JsValue) -> String {
    v.as_string().unwrap_or_else(|| format!("{v:?}"))
}

/
/
/

H
o
i
s
t
e
d

p
r
o
p
e
r
t
y

k
e
y
s
,

c
r
e
a
t
e
d

o
n
c
e

f
o
r

t
h
e

h
o
t
t
e
s
t

p
a
t
h
.
macro_rules! js_keys {
    ($($name:ident => $lit:literal),* $(,)?) => {
        /
        /

        `
        &
        N
        A
        M
        E
        `

        o
        n

        a

        `
        t
        h
        r
        e
        a
        d
        _
        l
        o
        c
        a
        l
        !
        `

        i
        s

        a

        p
        r
        o
        m
        o
        t
        e
        d

        `
        '
        s
        t
        a
        t
        i
        c
        `

        r
        e
        f
        e
        r
        e
        n
        c
        e
        .
        $(thread_local! {
            pub(crate) static $name: JsValue = JsValue::from_str($lit);
        })*
    };
}

js_keys! {
    KEY_OK => "ok",
    KEY_ERROR => "error",
    KEY_NAME => "name",
    KEY_MESSAGE => "message",
    KEY_PAGE => "page",
    KEY_WIDTH => "width",
    KEY_HEIGHT => "height",
    KEY_DATA => "data",
}

/// `obj[key]` using one of the hoisted keys.
pub(crate) fn reflect_get(
    obj: &JsValue,
    key: &'static LocalKey<JsValue>,
) -> Result<JsValue, JsValue> {
    key.with(|k| js_sys::Reflect::get(obj, k))
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
pub(crate) fn require_pdf_reader() -> Result<(), EngineError> {
    if crate::bridge::has_pdf_reader() {
        Ok(())
    } else {
        Err(EngineError {
            name: "no_engine".to_string(),
            message: "PDF engine is not loaded yet. Restart the app and try again.".to_string(),
        })
    }
}

/
/
/

[
`
r
e
q
u
i
r
e
_
p
d
f
_
r
e
a
d
e
r
`
]

a
s

a

b
o
o
l
e
a
n
,

f
o
r

t
h
e

s
i
l
e
n
t

c
a
l
l
s
.
pub(crate) fn guard_pdf_reader() -> bool {
    crate::bridge::has_pdf_reader()
}

/
/
/

P
a
r
s
e
s

a

`
{
o
k
,

e
r
r
o
r
?
,

.
.
.
}
`

v
a
l
u
e

i
n
t
o

`
T
`
.
pub(crate) fn resolve<T: DeserializeOwned>(value: JsValue, what: &str) -> Result<T, EngineError> {
    let is_ok = reflect_get(&value, &KEY_OK)
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if is_ok {
        serde_wasm_bindgen::from_value(value).map_err(|e| EngineError {
            name: "parse".to_string(),
            message: format!("{what}: bad engine payload ({e})"),
        })
    } else {
        let err = reflect_get(&value, &KEY_ERROR).unwrap_or(JsValue::UNDEFINED);
        let name = reflect_get(&err, &KEY_NAME).map(js_str).unwrap_or_default();
        let message = reflect_get(&err, &KEY_MESSAGE)
            .map(js_str)
            .unwrap_or_else(|_| "unknown engine error".to_string());
        Err(EngineError { name, message })
    }
}
