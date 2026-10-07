/
/
!

T
h
e

l
o
o
k
-
a
h
e
a
d
:

r
e
s
o
l
v
e

t
h
e

p
a
g
e
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

i
s

a
p
p
r
o
a
c
h
i
n
g
.

use super::{Paper, land_sample, publish, slot, spawn_engine};
use crate::session::PdfSession;

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
s

w
h
o
s
e

c
o
l
o
u
r

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

w
a
n
t
s

k
n
o
w
n
.
pub(super) fn lookahead_wants(s: &Paper) -> Vec<u32> {
    if !s.blend_on || s.num_pages == 0 {
        return Vec::new();
    }
    let base = s.position.floor().max(1.0) as u32;
    let mut wants = Vec::new();
    for page in [base, base + 1, base + 2] {
        if (1..=s.num_pages).contains(&page)
            && !s.palettes[slot(s.config.area)].contains(page)
            && !s.sampling.contains(&page)
        {
            wants.push(page);
        }
    }
    wants
}

/
/
/

R
e
s
o
l
v
e

t
h
e

p
a
g
e
s

[
`
l
o
o
k
a
h
e
a
d
_
w
a
n
t
s
`
]

n
a
m
e
s
,

e
p
o
c
h
-
g
u
a
r
d
e
d
.
pub(super) fn ensure_lookahead(session: &PdfSession) {
    let (epoch, pages) = session.with_paper(|s| {
        let wants = lookahead_wants(s);
        for page in &wants {
            s.start_sample(*page);
        }
        (s.epoch, wants)
    });
    for page in pages {
        sample_page(session, epoch, page);
    }
}

pub(super) fn sample_page(session: &PdfSession, epoch: u64, page: u32) {
    spawn_engine(session, move |session| async move {
        let frame = session.sample_paper_page(page).await.ok().flatten();
        if land_sample(&session, epoch, page, frame.as_ref()) {
            publish(&session);
        }
    });
}
