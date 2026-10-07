/
/
!

T
h
e

c
o
v
e
r

s
t
o
r
e
'
s

d
a
t
a

t
y
p
e
s
,

n
a
m
e
d

b
y

b
o
t
h

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

a
n
d

/
/
!

`
s
t
o
r
a
g
e
`
.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub data_url: String,
    pub width: f64,
    pub height: f64,
}

/
/
/

B
e
h
i
n
d

a
n

`
A
r
c
`
:

a

c
o
v
e
r

i
s

t
e
n
s

o
f

k
i
l
o
b
y
t
e
s
.
pub type CoverMap = std::collections::HashMap<String, Arc<CoverImage>>;

/
/
/

T
h
e

c
o
v
e
r

q
u
e
u
e
'
s

r
e
n
d
e
r

w
i
d
t
h
.
pub const COVER_WIDTH: f64 = 240.0;

/
/
/

T
h
e

p
a
g
e

a
s
p
e
c
t

a

t
i
l
e

a
s
s
u
m
e
s

b
e
f
o
r
e

a

r
e
a
l

c
o
v
e
r

a
r
r
i
v
e
s
.
pub const DEFAULT_PAGE_ASPECT: f64 = 0.75;

/// The persisted map's entry cap (`library-core`'s blob budget rule).
pub const COVER_CAP: usize = 400;
