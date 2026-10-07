/
/
!

T
h
e

a
p
p
'
s

o
n
e

c
l
o
c
k
:

m
i
l
l
i
s
e
c
o
n
d
s

s
i
n
c
e

t
h
e

e
p
o
c
h
.

/// Milliseconds since the Unix epoch; `0` off wasm (host tests).
pub fn now_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
}
