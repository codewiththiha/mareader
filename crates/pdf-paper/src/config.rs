/
/
!

W
h
i
c
h

p
i
x
e
l
s

c
a
r
r
y

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
'
s

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
.

use serde::{Deserialize, Serialize};

/
/
/

E
d
g
e
-
s
t
r
i
p

b
o
u
n
d
s
,

i
n

s
a
m
p
l
e
d
-
r
a
s
t
e
r

p
i
x
e
l
s
.
const MIN_EDGE_WIDTH: u32 = 2;
const MAX_EDGE_WIDTH: u32 = 32;

/
/
/

T
h
e

d
e
f
a
u
l
t

e
d
g
e
-
s
t
r
i
p

t
h
i
c
k
n
e
s
s
:

a

t
h
i
n

s
l
i
c
e

o
f

e
a
c
h

s
i
d
e
.
pub const DEFAULT_EDGE_WIDTH: u32 = 10;

/// Which pixels of a page raster carry the paper colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperArea {
    #[default]
    WholePage,
    Edges,
}

impl PaperArea {
    pub fn label(&self) -> &'static str {
        match self {
            Self::WholePage => "Whole Page",
            Self::Edges => "Edges",
        }
    }
}

/
/
/

E
v
e
r
y

k
n
o
b

t
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

e
x
p
o
s
e
s
,

i
n

o
n
e

v
a
l
u
e
.
pub struct PaperConfig {
    #[serde(default)]
    pub area: PaperArea,
    #[serde(default = "default_edge_width")]
    pub edge_width: u32,
}

fn default_edge_width() -> u32 {
    DEFAULT_EDGE_WIDTH
}

impl Default for PaperConfig {
    fn default() -> Self {
        Self {
            area: PaperArea::default(),
            edge_width: DEFAULT_EDGE_WIDTH,
        }
    }
}

impl PaperConfig {
    /
    /
    /

    C
    l
    a
    m
    p

    e
    v
    e
    r
    y

    k
    n
    o
    b

    i
    n
    t
    o

    i
    t
    s

    l
    e
    g
    a
    l

    r
    a
    n
    g
    e
    .
    pub fn sanitize(&mut self) {
        self.edge_width = self.edge_width.clamp(MIN_EDGE_WIDTH, MAX_EDGE_WIDTH);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_whole_page() {
        let c = PaperConfig::default();
        assert_eq!(c.area, PaperArea::WholePage);
        assert_eq!(c.edge_width, DEFAULT_EDGE_WIDTH);
    }

    #[test]
    fn a_config_round_trips_through_snake_case_json() {
        let c = PaperConfig {
            area: PaperArea::Edges,
            edge_width: 6,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""area":"edges""#), "{json}");
        assert_eq!(serde_json::from_str::<PaperConfig>(&json).unwrap(), c);
    }

    #[test]
    fn a_stale_blob_loads_and_fills_in_the_defaults() {
        /
        /

        A
        n

        o
        l
        d
        e
        r

        b
        l
        o
        b

        c
        a
        r
        r
        i
        e
        s

        r
        e
        t
        i
        r
        e
        d

        k
        e
        y
        s
        ;

        d
        e
        f
        a
        u
        l
        t
        s

        f
        i
        l
        l

        t
        h
        e

        r
        e
        s
        t
        .
        let c: PaperConfig = serde_json::from_str(r#"{"mode":"fixed","scan_pages":100}"#).unwrap();
        assert_eq!(c.area, PaperArea::WholePage);
        assert_eq!(c.edge_width, DEFAULT_EDGE_WIDTH);
    }

    #[test]
    fn sanitize_clamps_the_edge_width() {
        let mut c = PaperConfig {
            edge_width: 99,
            ..PaperConfig::default()
        };
        c.sanitize();
        assert_eq!(c.edge_width, MAX_EDGE_WIDTH);
        c.edge_width = 0;
        c.sanitize();
        assert_eq!(c.edge_width, MIN_EDGE_WIDTH);
    }
}
