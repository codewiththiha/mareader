/
/
!

P
a
g
e
-
p
a
p
e
r

c
o
l
o
u
r

l
o
g
i
c

f
o
r

t
h
e

b
l
e
n
d

b
a
c
k
d
r
o
p
.

mod color;
mod config;
mod detect;
mod palette;

pub use color::{Rgb, lerp};
pub use config::{DEFAULT_EDGE_WIDTH, PaperArea, PaperConfig};
pub use detect::{PAPER_SHARE, PaperDetector};
pub use palette::PagePalette;
