//! Build-time declarations shared by geometry generation and the SVG oracle.

#[allow(dead_code)]
pub(crate) struct Part {
    pub(crate) name: &'static str,
    pub(crate) ox: f32,
    pub(crate) oy: f32,
    pub(crate) len: f32,
    pub(crate) w: f32,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prop {
    X,
    Y,
    Scale,
    ScaleX,
    ScaleY,
    Rotate,
    SkewX,
    SkewY,
    Opacity,
    PathLen,
    PathOff,
    StrokeW,
}

pub(crate) struct Seg {
    pub(crate) part: u8,
    pub(crate) prop: Prop,
    pub(crate) t0: f32,
    pub(crate) dur: f32,
    pub(crate) keys: &'static [f32],
    pub(crate) times: &'static [f32],
    pub(crate) eases: &'static [u8],
}

pub(crate) struct Hole {
    pub(crate) part: u8,
    pub(crate) width: bool,
}

pub(crate) struct Pack {
    pub(crate) id: &'static str,
    pub(crate) dur: f32,
    pub(crate) parts: &'static [Part],
    pub(crate) segs: &'static [Seg],
    pub(crate) holes: &'static [Hole],
    pub(crate) svg: &'static str,
}
