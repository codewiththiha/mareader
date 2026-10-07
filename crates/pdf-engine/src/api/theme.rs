/
/
!

T
h
e

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

b
r
o
a
d
c
a
s
t
:

r
e
-
b
a
k
e
,

s
c
r
u
b

m
o
d
e
,

m
e
n
u

r
e
t
e
n
t
i
o
n
.

use super::guard_pdf_reader;
use crate::bridge;

/
/
/

R
e
-
b
a
k
e

t
h
e

t
h
e
m
e

i
n
t
o

e
v
e
r
y

r
a
s
t
e
r

e
v
e
r
y

l
i
v
e

s
e
s
s
i
o
n

h
o
l
d
s
.
pub fn refresh_theme() {
    if !guard_pdf_reader() {
        return;
    }
    bridge::refresh_theme();
}

/
/
/

E
n
t
e
r

o
r

l
e
a
v
e

t
h
e

s
c
r
u
b

w
i
n
d
o
w
'
s

r
e
a
l
-
t
i
m
e

c
o
m
p
o
s
i
t
i
n
g
.
pub fn set_scrub_mode(on: bool) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::set_scrub_mode(on);
}

/
/
/

W
h
e
t
h
e
r

t
h
e

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

p
o
p
o
v
e
r

i
s

o
p
e
n
;

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
t
a
i
n
s

r
a
w
s
.
pub fn set_appearance_menu_open(on: bool) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::set_appearance_menu_open(on);
}
