//! Build-only animated icon declarations: the Quill set.

use super::{Hole, Pack, Part, Prop, Seg};

/// Segment eases index this table.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum Ease {
    Linear,
    Bezier([f32; 4]),
    /// A damped spring; the value is its damping ratio.
    Spring(f32),
}

pub(crate) const EASINGS: [Ease; 8] = [
    Ease::Linear,                         // 0 lin
    Ease::Bezier([0.16, 1.0, 0.3, 1.0]),  // 1 flick
    Ease::Bezier([0.33, 0.0, 0.5, 1.0]),  // 2 cue
    Ease::Bezier([0.65, 0.0, 0.35, 1.0]), // 3 glide
    Ease::Spring(0.5),                    // 4 spring
    Ease::Spring(0.78),                   // 5 heavy
    Ease::Bezier([0.55, 0.0, 1.0, 0.45]), // 6 drop
    Ease::Bezier([0.25, 0.1, 0.25, 1.0]), // 7 soft
];

/// The rail flicks to the edge and springs home; the frame takes the tug.
static PANEL_LEFT: Pack = Pack {
    id: "panel-left",
    dur: 0.4833,
    parts: &[
        Part {
            name: "frame",
            ox: 12.0,
            oy: 12.0,
            len: 60.5783,
            w: 2.0,
        },
        Part {
            name: "rail",
            ox: 9.0,
            oy: 12.0,
            len: 9.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.512, -3.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.0448, 0.72, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.5, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.05,
            dur: 0.42,
            keys: &[0.0, -0.7, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 5],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M16.37 3L16.5 3A4.5 4.5 0 0 1 21 7.5L21 16.5A4.5 4.5 0 0 1 16.5 21L7.5 21A4.5 4.5 0 0 1 3 16.5L3 7.5A4.5 4.5 0 0 1 7.5 3L12.67 3\"/></g><g\u{1}><g\u{2}><path d=\"M9 7.5v9\"/></g></g>",
};

/// The chevron jabs; the rail is shoved a beat later and bounces back.
static PANEL_LEFT_CLOSE: Pack = Pack {
    id: "panel-left-close",
    dur: 0.5,
    parts: &[
        Part {
            name: "frame",
            ox: 12.0,
            oy: 12.0,
            len: 60.5783,
            w: 2.0,
        },
        Part {
            name: "rail",
            ox: 9.0,
            oy: 12.0,
            len: 9.0,
            w: 2.0,
        },
        Part {
            name: "chev",
            ox: 15.0,
            oy: 12.0,
            len: 8.4853,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 2,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.384, -2.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.5, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.07,
            dur: 0.42,
            keys: &[0.0, -1.7, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.07,
            dur: 0.42,
            keys: &[1.0, 0.82, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 2,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M16.37 3L16.5 3A4.5 4.5 0 0 1 21 7.5L21 16.5A4.5 4.5 0 0 1 16.5 21L7.5 21A4.5 4.5 0 0 1 3 16.5L3 7.5A4.5 4.5 0 0 1 7.5 3L12.67 3\"/></g><g\u{1}><path d=\"M9 7.5v9\"/></g><g\u{1}><g\u{2}><path d=\"M16.5 9l-3 3 3 3\"/></g></g>",
};

/// The rail is dragged inward like a resize handle, held, and let go.
static PANEL_RIGHT: Pack = Pack {
    id: "panel-right",
    dur: 0.5167,
    parts: &[
        Part {
            name: "frame",
            ox: 12.0,
            oy: 12.0,
            len: 60.5783,
            w: 2.0,
        },
        Part {
            name: "rail",
            ox: 15.0,
            oy: 12.0,
            len: 9.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.515,
            keys: &[0.0, 0.48, -3.0, -3.0, 0.0],
            times: &[0.0, 0.0874, 0.3204, 0.4175, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.515,
            keys: &[1.0, 1.0352, 0.78, 0.78, 1.0],
            times: &[0.0, 0.0874, 0.3204, 0.4175, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.515,
            keys: &[2.0, 2.0, 2.5, 2.5, 2.0],
            times: &[0.0, 0.0874, 0.3204, 0.4175, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleX,
            t0: 0.04,
            dur: 0.42,
            keys: &[1.0, 1.035, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 5],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M11.33 3L16.5 3A4.5 4.5 0 0 1 21 7.5L21 16.5A4.5 4.5 0 0 1 16.5 21L7.5 21A4.5 4.5 0 0 1 3 16.5L3 7.5A4.5 4.5 0 0 1 7.5 3L7.63 3\"/></g><g\u{1}><g\u{2}><path d=\"M15 7.5v9\"/></g></g>",
};

/// The two halves part and the rail is inked from its middle outward.
static COLUMNS_2: Pack = Pack {
    id: "columns-2",
    dur: 0.4833,
    parts: &[
        Part {
            name: "left",
            ox: 6.0,
            oy: 12.0,
            len: 28.1392,
            w: 2.0,
        },
        Part {
            name: "right",
            ox: 18.0,
            oy: 12.0,
            len: 28.1392,
            w: 2.0,
        },
        Part {
            name: "rail",
            ox: 12.0,
            oy: 12.0,
            len: 9.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.24, -1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.24, 1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::PathLen,
            t0: 0.02,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::PathOff,
            t0: 0.02,
            dur: 0.31,
            keys: &[0.0, 0.47, 0.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::StrokeW,
            t0: 0.1,
            dur: 0.37,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 2,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M10 3H7.5A4.5 4.5 0 0 0 3 7.5v9A4.5 4.5 0 0 0 7.5 21H10\"/></g><g\u{1}><path d=\"M14 3h2.5A4.5 4.5 0 0 1 21 7.5v9a4.5 4.5 0 0 1-4.5 4.5H14\"/></g><g\u{1}><g\u{2}><path d=\"M12 7.5v9\"/></g></g>",
};

/// Tiles are dealt in reading order, each one tipping as it lands.
static LAYOUT_GRID: Pack = Pack {
    id: "layout-grid",
    dur: 0.55,
    parts: &[
        Part {
            name: "t1",
            ox: 6.5,
            oy: 6.5,
            len: 24.225,
            w: 2.0,
        },
        Part {
            name: "t2",
            ox: 17.5,
            oy: 6.5,
            len: 24.225,
            w: 2.0,
        },
        Part {
            name: "t3",
            ox: 6.5,
            oy: 17.5,
            len: 24.225,
            w: 2.0,
        },
        Part {
            name: "t4",
            ox: 17.5,
            oy: 17.5,
            len: 24.225,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.415,
            keys: &[1.0, 1.0544, 0.66, 1.0],
            times: &[0.0, 0.1084, 0.3253, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.415,
            keys: &[0.0, 1.6, -10.0, 0.0],
            times: &[0.0, 0.1084, 0.3253, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.045,
            dur: 0.415,
            keys: &[1.0, 1.0544, 0.66, 1.0],
            times: &[0.0, 0.1084, 0.3253, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Rotate,
            t0: 0.045,
            dur: 0.415,
            keys: &[0.0, 1.6, -10.0, 0.0],
            times: &[0.0, 0.1084, 0.3253, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Scale,
            t0: 0.09,
            dur: 0.415,
            keys: &[1.0, 1.0544, 0.66, 1.0],
            times: &[0.0, 0.1084, 0.3253, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Rotate,
            t0: 0.09,
            dur: 0.415,
            keys: &[0.0, 1.6, -10.0, 0.0],
            times: &[0.0, 0.1084, 0.3253, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Scale,
            t0: 0.135,
            dur: 0.415,
            keys: &[1.0, 1.0544, 0.66, 1.0],
            times: &[0.0, 0.1084, 0.3253, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Rotate,
            t0: 0.135,
            dur: 0.415,
            keys: &[0.0, 1.6, -10.0, 0.0],
            times: &[0.0, 0.1084, 0.3253, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M5.2 3L7.8 3A2.2 2.2 0 0 1 10 5.2L10 7.8A2.2 2.2 0 0 1 7.8 10L5.2 10A2.2 2.2 0 0 1 3 7.8L3 5.2A2.2 2.2 0 0 1 5.2 3Z\"/></g><g\u{1}><path d=\"M16.2 3L18.8 3A2.2 2.2 0 0 1 21 5.2L21 7.8A2.2 2.2 0 0 1 18.8 10L16.2 10A2.2 2.2 0 0 1 14 7.8L14 5.2A2.2 2.2 0 0 1 16.2 3Z\"/></g><g\u{1}><path d=\"M5.2 14L7.8 14A2.2 2.2 0 0 1 10 16.2L10 18.8A2.2 2.2 0 0 1 7.8 21L5.2 21A2.2 2.2 0 0 1 3 18.8L3 16.2A2.2 2.2 0 0 1 5.2 14Z\"/></g><g\u{1}><path d=\"M16.2 14L18.8 14A2.2 2.2 0 0 1 21 16.2L21 18.8A2.2 2.2 0 0 1 18.8 21L16.2 21A2.2 2.2 0 0 1 14 18.8L14 16.2A2.2 2.2 0 0 1 16.2 14Z\"/></g>",
};

/// Header, main and side breathe apart in turn and click back together.
static LAYOUT_TEMPLATE: Pack = Pack {
    id: "layout-template",
    dur: 0.5667,
    parts: &[
        Part {
            name: "head",
            ox: 12.0,
            oy: 6.5,
            len: 45.7102,
            w: 2.0,
        },
        Part {
            name: "main",
            ox: 7.5,
            oy: 17.5,
            len: 27.7102,
            w: 2.0,
        },
        Part {
            name: "side",
            ox: 18.5,
            oy: 17.5,
            len: 19.7102,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.224, -1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, 0.224, -1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::X,
            t0: 0.09,
            dur: 0.465,
            keys: &[0.0, -0.224, 1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::ScaleY,
            t0: 0.09,
            dur: 0.465,
            keys: &[1.0, 1.0256, 0.84, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M5.5 3L18.5 3A2.5 2.5 0 0 1 21 5.5L21 7.5A2.5 2.5 0 0 1 18.5 10L5.5 10A2.5 2.5 0 0 1 3 7.5L3 5.5A2.5 2.5 0 0 1 5.5 3Z\"/></g><g\u{1}><path d=\"M5.5 14L9.5 14A2.5 2.5 0 0 1 12 16.5L12 18.5A2.5 2.5 0 0 1 9.5 21L5.5 21A2.5 2.5 0 0 1 3 18.5L3 16.5A2.5 2.5 0 0 1 5.5 14Z\"/></g><g\u{1}><path d=\"M18.5 14L18.5 14A2.5 2.5 0 0 1 21 16.5L21 18.5A2.5 2.5 0 0 1 18.5 21L18.5 21A2.5 2.5 0 0 1 16 18.5L16 16.5A2.5 2.5 0 0 1 18.5 14Z\"/></g>",
};

/// A wheel flick: far edge leads, page follows, near edge fades.
static GALLERY_VERTICAL: Pack = Pack {
    id: "gallery-vertical",
    dur: 0.55,
    parts: &[
        Part {
            name: "top",
            ox: 12.0,
            oy: 3.0,
            len: 9.0,
            w: 2.0,
        },
        Part {
            name: "page",
            ox: 12.0,
            oy: 12.0,
            len: 46.509,
            w: 2.0,
        },
        Part {
            name: "bottom",
            ox: 12.0,
            oy: 21.0,
            len: 9.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.32, -2.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.035,
            dur: 0.465,
            keys: &[0.0, 0.32, -2.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.035,
            dur: 0.465,
            keys: &[1.0, 1.016, 0.9, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.07,
            dur: 0.465,
            keys: &[0.0, 0.224, -1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.07,
            dur: 0.465,
            keys: &[1.0, 1.0, 0.2, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M7.5 3h9\"/></g><g\u{1}><path d=\"M7.2 7L16.8 7A3.2 3.2 0 0 1 20 10.2L20 13.8A3.2 3.2 0 0 1 16.8 17L7.2 17A3.2 3.2 0 0 1 4 13.8L4 10.2A3.2 3.2 0 0 1 7.2 7Z\"/></g><g\u{1}><path d=\"M7.5 21h9\"/></g>",
};

/// A swipe: the page squashes with its own momentum and the neighbours trail.
static GALLERY_HORIZONTAL: Pack = Pack {
    id: "gallery-horizontal",
    dur: 0.55,
    parts: &[
        Part {
            name: "left",
            ox: 3.0,
            oy: 12.0,
            len: 9.0,
            w: 2.0,
        },
        Part {
            name: "page",
            ox: 12.0,
            oy: 12.0,
            len: 46.509,
            w: 2.0,
        },
        Part {
            name: "right",
            ox: 21.0,
            oy: 12.0,
            len: 9.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 2,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.32, -2.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.035,
            dur: 0.465,
            keys: &[0.0, 0.32, -2.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleX,
            t0: 0.035,
            dur: 0.465,
            keys: &[1.0, 1.0192, 0.88, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.07,
            dur: 0.465,
            keys: &[0.0, 0.224, -1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.07,
            dur: 0.465,
            keys: &[1.0, 1.0, 0.2, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M3 7.5v9\"/></g><g\u{1}><path d=\"M10.2 4L13.8 4A3.2 3.2 0 0 1 17 7.2L17 16.8A3.2 3.2 0 0 1 13.8 20L10.2 20A3.2 3.2 0 0 1 7 16.8L7 7.2A3.2 3.2 0 0 1 10.2 4Z\"/></g><g\u{1}><path d=\"M21 7.5v9\"/></g>",
};

/// Three lines shimmy in alternate directions, a drawer being tugged.
static MENU: Pack = Pack {
    id: "menu",
    dur: 0.5667,
    parts: &[
        Part {
            name: "l1",
            ox: 12.0,
            oy: 6.0,
            len: 16.0,
            w: 2.0,
        },
        Part {
            name: "l2",
            ox: 12.0,
            oy: 12.0,
            len: 16.0,
            w: 2.0,
        },
        Part {
            name: "l3",
            ox: 8.5,
            oy: 18.0,
            len: 9.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.352, 2.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleX,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.0256, 0.84, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, 0.352, -2.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleX,
            t0: 0.045,
            dur: 0.465,
            keys: &[1.0, 1.0256, 0.84, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::X,
            t0: 0.09,
            dur: 0.465,
            keys: &[0.0, -0.352, 2.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::ScaleX,
            t0: 0.09,
            dur: 0.465,
            keys: &[1.0, 1.0256, 0.84, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M4 6h16\"/></g><g\u{1}><path d=\"M4 12h16\"/></g><g\u{1}><path d=\"M4 18h9\"/></g>",
};

/// Winds back, snips a quarter turn and lands exactly on itself.
static X: Pack = Pack {
    id: "x",
    dur: 0.45,
    parts: &[Part {
        name: "x",
        ox: 12.0,
        oy: 12.0,
        len: 14.1421,
        w: 2.0,
    }],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.44,
            keys: &[0.0, -6.3, 90.0],
            times: &[0.0, 0.1023, 1.0],
            eases: &[2, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.42,
            keys: &[1.0, 0.7, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::StrokeW,
            t0: 0.04,
            dur: 0.37,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 0,
            width: true,
        },
    ],
    svg: "<g\u{1}><g\u{2}><path d=\"M7 7l10 10\"/><path d=\"M17 7 7 17\"/></g></g>",
};

/// The bar is squeezed and tucked down, then springs back to length.
static MINUS: Pack = Pack {
    id: "minus",
    dur: 0.4667,
    parts: &[Part {
        name: "bar",
        ox: 12.0,
        oy: 12.0,
        len: 12.0,
        w: 2.0,
    }],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::ScaleX,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.0928, 0.42, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.288, 1.8, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.6, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 0,
            width: true,
        },
    ],
    svg: "<g\u{1}><g\u{2}><path d=\"M6 12h12\"/></g></g>",
};

/// Each bar is re-inked from the centre, then the whole mark pops.
static PLUS: Pack = Pack {
    id: "plus",
    dur: 0.55,
    parts: &[
        Part {
            name: "plus",
            ox: 12.0,
            oy: 12.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "v",
            ox: 12.0,
            oy: 12.0,
            len: 12.0,
            w: 2.0,
        },
        Part {
            name: "h",
            ox: 12.0,
            oy: 12.0,
            len: 12.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::PathOff,
            t0: 0.0,
            dur: 0.31,
            keys: &[0.0, 0.47, 0.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::PathLen,
            t0: 0.055,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::PathOff,
            t0: 0.055,
            dur: 0.31,
            keys: &[0.0, 0.47, 0.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.13,
            dur: 0.42,
            keys: &[1.0, 1.16, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><g\u{1}><path d=\"M12 6v12\"/></g><g\u{1}><path d=\"M6 12h12\"/></g></g>",
};

/// The frame flexes inward and an echo of it swells out to fill the screen.
static SQUARE: Pack = Pack {
    id: "square",
    dur: 0.4667,
    parts: &[
        Part {
            name: "echo",
            ox: 12.0,
            oy: 12.0,
            len: 56.2783,
            w: 2.0,
        },
        Part {
            name: "frame",
            ox: 12.0,
            oy: 12.0,
            len: 52.5783,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.0224, 0.86, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.07,
            dur: 0.38,
            keys: &[0.9, 1.36],
            times: &[],
            eases: &[1],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.07,
            dur: 0.38,
            keys: &[0.0, 0.45, 0.0],
            times: &[0.0, 0.25, 1.0],
            eases: &[7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M8.5 4L15.5 4A4.5 4.5 0 0 1 20 8.5L20 15.5A4.5 4.5 0 0 1 15.5 20L8.5 20A4.5 4.5 0 0 1 4 15.5L4 8.5A4.5 4.5 0 0 1 8.5 4Z\"/></g><g\u{1}><path d=\"M15.5 4L15.5 4A4.5 4.5 0 0 1 20 8.5L20 15.5A4.5 4.5 0 0 1 15.5 20L8.5 20A4.5 4.5 0 0 1 4 15.5L4 8.5A4.5 4.5 0 0 1 8.5 4L11.8 4\"/></g>",
};

/// Two sheets slide apart along the diagonal and restack.
static COPY: Pack = Pack {
    id: "copy",
    dur: 0.5167,
    parts: &[
        Part {
            name: "back",
            ox: 9.0,
            oy: 9.0,
            len: 25.3537,
            w: 2.0,
        },
        Part {
            name: "front",
            ox: 15.0,
            oy: 15.0,
            len: 41.9943,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.224, 1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.224, 1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.04,
            dur: 0.465,
            keys: &[0.0, 0.224, -1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.04,
            dur: 0.465,
            keys: &[0.0, 0.224, -1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.04,
            dur: 0.465,
            keys: &[1.0, 1.0, 0.5, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M5.5 15A2.5 2.5 0 0 1 3 12.5v-6A3.5 3.5 0 0 1 6.5 3h6A2.5 2.5 0 0 1 15 5.5\"/></g><g\u{1}><path d=\"M12.5 9L17.5 9A3.5 3.5 0 0 1 21 12.5L21 17.5A3.5 3.5 0 0 1 17.5 21L12.5 21A3.5 3.5 0 0 1 9 17.5L9 12.5A3.5 3.5 0 0 1 12.5 9Z\"/></g>",
};

/// Corners fly outward in diagonal pairs, pressing harder as they go.
static MAXIMIZE: Pack = Pack {
    id: "maximize",
    dur: 0.5167,
    parts: &[
        Part {
            name: "tl",
            ox: 6.5,
            oy: 6.5,
            len: 8.9275,
            w: 2.0,
        },
        Part {
            name: "tr",
            ox: 17.5,
            oy: 6.5,
            len: 8.9275,
            w: 2.0,
        },
        Part {
            name: "br",
            ox: 17.5,
            oy: 17.5,
            len: 8.9275,
            w: 2.0,
        },
        Part {
            name: "bl",
            ox: 6.5,
            oy: 17.5,
            len: 8.9275,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.272, -1.7, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.272, -1.7, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.4, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.272, 1.7, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.272, 1.7, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.4, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, -0.272, 1.7, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, 0.272, -1.7, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.045,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.4, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::X,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, 0.272, -1.7, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Y,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, -0.272, 1.7, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::StrokeW,
            t0: 0.045,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.4, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 0,
            width: true,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 2,
            width: true,
        },
        Hole {
            part: 3,
            width: false,
        },
        Hole {
            part: 3,
            width: true,
        },
    ],
    svg: "<g\u{1}><g\u{2}><path d=\"M4 9V6.5A2.5 2.5 0 0 1 6.5 4H9\"/></g></g><g\u{1}><g\u{2}><path d=\"M15 4h2.5A2.5 2.5 0 0 1 20 6.5V9\"/></g></g><g\u{1}><g\u{2}><path d=\"M20 15v2.5a2.5 2.5 0 0 1-2.5 2.5H15\"/></g></g><g\u{1}><g\u{2}><path d=\"M9 20H6.5A2.5 2.5 0 0 1 4 17.5V15\"/></g></g>",
};

/// Corners pinch toward the centre in diagonal pairs and release.
static MINIMIZE: Pack = Pack {
    id: "minimize",
    dur: 0.5167,
    parts: &[
        Part {
            name: "tl",
            ox: 6.5,
            oy: 6.5,
            len: 8.9275,
            w: 2.0,
        },
        Part {
            name: "tr",
            ox: 17.5,
            oy: 6.5,
            len: 8.9275,
            w: 2.0,
        },
        Part {
            name: "br",
            ox: 17.5,
            oy: 17.5,
            len: 8.9275,
            w: 2.0,
        },
        Part {
            name: "bl",
            ox: 6.5,
            oy: 17.5,
            len: 8.9275,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.256, 1.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.256, 1.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.4, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.256, -1.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.256, -1.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.4, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, 0.256, -1.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, -0.256, 1.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.045,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.4, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::X,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, -0.256, 1.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Y,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, 0.256, -1.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::StrokeW,
            t0: 0.045,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.4, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 0,
            width: true,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 2,
            width: true,
        },
        Hole {
            part: 3,
            width: false,
        },
        Hole {
            part: 3,
            width: true,
        },
    ],
    svg: "<g\u{1}><g\u{2}><path d=\"M9 4v2.5A2.5 2.5 0 0 1 6.5 9H4\"/></g></g><g\u{1}><g\u{2}><path d=\"M15 4v2.5A2.5 2.5 0 0 0 17.5 9H20\"/></g></g><g\u{1}><g\u{2}><path d=\"M20 15h-2.5a2.5 2.5 0 0 0-2.5 2.5V20\"/></g></g><g\u{1}><g\u{2}><path d=\"M4 15h2.5A2.5 2.5 0 0 1 9 17.5V20\"/></g></g>",
};

/// The eye squints and dims while the strike is re-inked across it.
static EYE_OFF: Pack = Pack {
    id: "eye-off",
    dur: 0.4833,
    parts: &[
        Part {
            name: "eye",
            ox: 12.0,
            oy: 12.0,
            len: 47.3674,
            w: 2.0,
        },
        Part {
            name: "slash",
            ox: 12.0,
            oy: 12.0,
            len: 21.2132,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.48,
            keys: &[1.0, 0.55, 0.55, 1.0],
            times: &[0.0, 0.25, 0.375, 1.0],
            eases: &[1, 0, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.0,
            dur: 0.48,
            keys: &[1.0, 0.4, 0.4, 1.0],
            times: &[0.0, 0.25, 0.375, 1.0],
            eases: &[1, 0, 4],
        },
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.03,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.11,
            dur: 0.37,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M2.5 12c2.2-4 5.5-6 9.5-6s7.3 2 9.5 6c-2.2 4-5.5 6-9.5 6s-7.3-2-9.5-6z\"/><path d=\"M9.7 12a2.3 2.3 0 1 0 4.6 0a2.3 2.3 0 1 0 -4.6 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><g\u{2}><path d=\"M4.5 4.5l15 15\"/></g></g>",
};

/// A quick blink, then the pupil glances aside and settles.
static EYE: Pack = Pack {
    id: "eye",
    dur: 0.5,
    parts: &[
        Part {
            name: "lid",
            ox: 12.0,
            oy: 12.0,
            len: 47.3674,
            w: 2.0,
        },
        Part {
            name: "pupil",
            ox: 12.0,
            oy: 12.0,
            len: 0.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.3,
            keys: &[1.0, 0.1, 1.0],
            times: &[0.0, 0.2333, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.3,
            keys: &[1.0, 0.1, 1.0],
            times: &[0.0, 0.2333, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.15,
            dur: 0.34,
            keys: &[0.0, 1.9, 0.0],
            times: &[0.0, 0.34, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M2.5 12c2.2-4 5.5-6 9.5-6s7.3 2 9.5 6c-2.2 4-5.5 6-9.5 6s-7.3-2-9.5-6z\"/></g><g\u{1}><path d=\"M9.7 12a2.3 2.3 0 1 0 4.6 0a2.3 2.3 0 1 0 -4.6 0Z\" fill=\"white\" stroke=\"none\"/></g>",
};

/// Keys go down in a typed order, not a scanned one; the space bar finishes.
static KEYBOARD: Pack = Pack {
    id: "keyboard",
    dur: 0.55,
    parts: &[
        Part {
            name: "frame",
            ox: 12.0,
            oy: 12.0,
            len: 54.1227,
            w: 2.0,
        },
        Part {
            name: "k1",
            ox: 7.2,
            oy: 10.2,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "k2",
            ox: 10.4,
            oy: 10.2,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "k3",
            ox: 13.6,
            oy: 10.2,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "k4",
            ox: 16.8,
            oy: 10.2,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "space",
            ox: 12.0,
            oy: 14.6,
            len: 6.4,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.26,
            keys: &[0.0, 0.8, 0.0],
            times: &[0.0, 0.2308, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.26,
            keys: &[1.0, 0.55, 1.0],
            times: &[0.0, 0.2308, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 4,
            prop: Prop::Y,
            t0: 0.055,
            dur: 0.26,
            keys: &[0.0, 0.8, 0.0],
            times: &[0.0, 0.2308, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 4,
            prop: Prop::Scale,
            t0: 0.055,
            dur: 0.26,
            keys: &[1.0, 0.55, 1.0],
            times: &[0.0, 0.2308, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.11,
            dur: 0.26,
            keys: &[0.0, 0.8, 0.0],
            times: &[0.0, 0.2308, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.11,
            dur: 0.26,
            keys: &[1.0, 0.55, 1.0],
            times: &[0.0, 0.2308, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Y,
            t0: 0.165,
            dur: 0.26,
            keys: &[0.0, 0.8, 0.0],
            times: &[0.0, 0.2308, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Scale,
            t0: 0.165,
            dur: 0.26,
            keys: &[1.0, 0.55, 1.0],
            times: &[0.0, 0.2308, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 5,
            prop: Prop::Y,
            t0: 0.235,
            dur: 0.31,
            keys: &[0.0, 0.8, 0.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 5,
            prop: Prop::ScaleX,
            t0: 0.235,
            dur: 0.31,
            keys: &[1.0, 0.86, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
        Hole {
            part: 4,
            width: false,
        },
        Hole {
            part: 5,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M17.39 5.5L17.9 5.5A3.6 3.6 0 0 1 21.5 9.1L21.5 14.9A3.6 3.6 0 0 1 17.9 18.5L6.1 18.5A3.6 3.6 0 0 1 2.5 14.9L2.5 9.1A3.6 3.6 0 0 1 6.1 5.5L13.69 5.5\"/></g><g\u{1}><path d=\"M6.05 10.2a1.15 1.15 0 1 0 2.3 0a1.15 1.15 0 1 0 -2.3 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M9.25 10.2a1.15 1.15 0 1 0 2.3 0a1.15 1.15 0 1 0 -2.3 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M12.45 10.2a1.15 1.15 0 1 0 2.3 0a1.15 1.15 0 1 0 -2.3 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M15.65 10.2a1.15 1.15 0 1 0 2.3 0a1.15 1.15 0 1 0 -2.3 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M8.8 14.6h6.4\"/></g>",
};

/// Two ink knobs slide past each other along their rails and coast home.
static SLIDERS_HORIZONTAL: Pack = Pack {
    id: "sliders-horizontal",
    dur: 0.5333,
    parts: &[
        Part {
            name: "r1",
            ox: 12.0,
            oy: 8.0,
            len: 16.0,
            w: 2.0,
        },
        Part {
            name: "r2",
            ox: 12.0,
            oy: 16.0,
            len: 16.0,
            w: 2.0,
        },
        Part {
            name: "k1",
            ox: 9.0,
            oy: 8.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "k2",
            ox: 15.0,
            oy: 16.0,
            len: 0.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 2,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.832, 5.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::ScaleX,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.9648, 1.22, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::X,
            t0: 0.06,
            dur: 0.465,
            keys: &[0.0, 0.832, -5.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::ScaleX,
            t0: 0.06,
            dur: 0.465,
            keys: &[1.0, 0.9648, 1.22, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M4 8h16\"/></g><g\u{1}><path d=\"M4 16h16\"/></g><g\u{1}><path d=\"M6.5 8a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0 -5 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M12.5 16a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0 -5 0Z\" fill=\"white\" stroke=\"none\"/></g>",
};

/// Lifted, tilted and jabbed in: the needle sinks on contact.
static PIN: Pack = Pack {
    id: "pin",
    dur: 0.4667,
    parts: &[
        Part {
            name: "pin",
            ox: 12.0,
            oy: 20.0,
            len: 7.0,
            w: 2.0,
        },
        Part {
            name: "needle",
            ox: 12.0,
            oy: 13.5,
            len: 6.5,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.416, -2.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 1.76, -11.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.165,
            dur: 0.3,
            keys: &[1.0, 0.62, 1.0],
            times: &[0.0, 0.3, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M8.5 4h7\"/><path d=\"M10 4v5l-2.5 4.5h9L14 9V4\"/><g\u{1}><path d=\"M12 13.5V20\"/></g></g>",
};

/// The cog winds back and clicks forward exactly one tooth; the hub pulses.
static SETTINGS: Pack = Pack {
    id: "settings",
    dur: 0.4833,
    parts: &[
        Part {
            name: "gear",
            ox: 12.0,
            oy: 12.0,
            len: 67.9587,
            w: 2.0,
        },
        Part {
            name: "hub",
            ox: 12.0,
            oy: 12.0,
            len: 16.3386,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.46,
            keys: &[0.0, -4.2, 60.0],
            times: &[0.0, 0.0978, 1.0],
            eases: &[2, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.06,
            dur: 0.42,
            keys: &[1.0, 1.3, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M10.449 2.83L13.551 2.83L14.335 5.72L16.271 6.838L19.166 6.072L20.717 8.758L18.606 10.883L18.606 13.117L20.717 15.242L19.166 17.928L16.271 17.162L14.335 18.28L13.551 21.17L10.449 21.17L9.665 18.28L7.729 17.162L4.834 17.928L3.283 15.242L5.394 13.117L5.394 10.883L3.283 8.758L4.834 6.072L7.729 6.838L9.665 5.72Z\"/></g><g\u{1}><path d=\"M9.4 12A2.6 2.6 0 1 1 14.6 12A2.6 2.6 0 1 1 9.4 12Z\"/></g>",
};

/// A leaf turns over the gutter as the book lifts a hair off the desk.
static BOOK_OPEN: Pack = Pack {
    id: "book-open",
    dur: 0.4667,
    parts: &[
        Part {
            name: "book",
            ox: 12.0,
            oy: 20.0,
            len: 61.9738,
            w: 2.0,
        },
        Part {
            name: "leaf",
            ox: 12.0,
            oy: 12.0,
            len: 29.2739,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.144, -0.9, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 5],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.9936, 1.04, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 5],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleX,
            t0: 0.03,
            dur: 0.4,
            keys: &[1.0, -1.0],
            times: &[],
            eases: &[3],
        },
        Seg {
            part: 1,
            prop: Prop::Opacity,
            t0: 0.03,
            dur: 0.4,
            keys: &[0.0, 0.8, 0.8, 0.0],
            times: &[0.0, 0.15, 0.8, 1.0],
            eases: &[7, 7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M12 6.5c2-1.4 4.5-2 7.5-2A1.5 1.5 0 0 1 21 6v10.5a1.5 1.5 0 0 1-1.5 1.5c-3 0-5.5.6-7.5 2-2-1.4-4.5-2-7.5-2A1.5 1.5 0 0 1 3 16.5V6a1.5 1.5 0 0 1 1.5-1.5c3 0 5.5.6 7.5 2z\"/><path d=\"M12 6.5V20\"/></g><g\u{1}><path d=\"M12 6.5c2-1.4 4.5-2 7.5-2V18c-3 0-5.5.6-7.5 2\"/></g>",
};

/// Lines are inked in reading order, the way a voice moves through the page.
static BOOK_OPEN_TEXT: Pack = Pack {
    id: "book-open-text",
    dur: 0.45,
    parts: &[
        Part {
            name: "book",
            ox: 12.0,
            oy: 12.0,
            len: 61.9738,
            w: 2.0,
        },
        Part {
            name: "t1",
            ox: 12.0,
            oy: 12.0,
            len: 2.5,
            w: 2.0,
        },
        Part {
            name: "t2",
            ox: 12.0,
            oy: 12.0,
            len: 2.5,
            w: 2.0,
        },
        Part {
            name: "t3",
            ox: 12.0,
            oy: 12.0,
            len: 2.5,
            w: 2.0,
        },
        Part {
            name: "t4",
            ox: 12.0,
            oy: 12.0,
            len: 2.5,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.25,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::PathLen,
            t0: 0.065,
            dur: 0.25,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 3,
            prop: Prop::PathLen,
            t0: 0.13,
            dur: 0.25,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 4,
            prop: Prop::PathLen,
            t0: 0.195,
            dur: 0.25,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2, 1.0],
            eases: &[3, 1],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
        Hole {
            part: 4,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M12 6.5c2-1.4 4.5-2 7.5-2A1.5 1.5 0 0 1 21 6v10.5a1.5 1.5 0 0 1-1.5 1.5c-3 0-5.5.6-7.5 2-2-1.4-4.5-2-7.5-2A1.5 1.5 0 0 1 3 16.5V6a1.5 1.5 0 0 1 1.5-1.5c3 0 5.5.6 7.5 2z\"/><path d=\"M12 6.5V20\"/></g><g\u{1}><path d=\"M6.5 9.5H9\"/></g><g\u{1}><path d=\"M6.5 13H9\"/></g><g\u{1}><path d=\"M15 9.5h2.5\"/></g><g\u{1}><path d=\"M15 13h2.5\"/></g>",
};

/// The letter springs off the cover; its crossbar is inked a beat behind.
static BOOK_A: Pack = Pack {
    id: "book-a",
    dur: 0.5,
    parts: &[
        Part {
            name: "book",
            ox: 12.0,
            oy: 22.0,
            len: 30.8551,
            w: 2.0,
        },
        Part {
            name: "a",
            ox: 12.0,
            oy: 14.0,
            len: 15.0785,
            w: 2.0,
        },
        Part {
            name: "bar",
            ox: 12.0,
            oy: 11.6,
            len: 3.4,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.24, -1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.9776, 1.14, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::PathLen,
            t0: 0.09,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::PathOff,
            t0: 0.09,
            dur: 0.31,
            keys: &[0.0, 0.47, 0.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.07,
            dur: 0.42,
            keys: &[0.0, 0.6, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 5],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M19 17H7.5a2.5 2.5 0 0 0 0 5H19\"/><path d=\"M19 17V5a2 2 0 0 0-2-2H7.5A2.5 2.5 0 0 0 5 5.5v14\"/></g><g\u{1}><path d=\"M9.2 14l2.8-7 2.8 7\"/><g\u{1}><path d=\"M10.3 11.6h3.4\"/></g></g>",
};

/// The lens arcs up over the cover, magnifies, and drops back to its corner.
static BOOK_SEARCH: Pack = Pack {
    id: "book-search",
    dur: 0.5167,
    parts: &[
        Part {
            name: "book",
            ox: 12.0,
            oy: 12.0,
            len: 30.8551,
            w: 2.0,
        },
        Part {
            name: "glass",
            ox: 11.4,
            oy: 9.4,
            len: 16.967,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.505,
            keys: &[0.0, -0.256, 1.6, 1.6, 0.0],
            times: &[0.0, 0.0891, 0.3267, 0.4059, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.505,
            keys: &[0.0, 0.208, -1.3, -1.3, 0.0],
            times: &[0.0, 0.0891, 0.3267, 0.4059, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.505,
            keys: &[1.0, 0.9744, 1.16, 1.16, 1.0],
            times: &[0.0, 0.0891, 0.3267, 0.4059, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.505,
            keys: &[0.0, 2.24, -14.0, -14.0, 0.0],
            times: &[0.0, 0.0891, 0.3267, 0.4059, 1.0],
            eases: &[2, 1, 0, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M19 17H7.5a2.5 2.5 0 0 0 0 5H19\"/><path d=\"M19 17V5a2 2 0 0 0-2-2H7.5A2.5 2.5 0 0 0 5 5.5v14\"/></g><g\u{1}><path d=\"M8.7 9.4A2.7 2.7 0 1 1 14.1 9.4A2.7 2.7 0 1 1 8.7 9.4Z\"/><path d=\"M13.6 11.6l2 2\"/></g>",
};

/// The plus is stamped in with a quarter twist; the book takes the hit.
static BOOK_PLUS: Pack = Pack {
    id: "book-plus",
    dur: 0.5167,
    parts: &[
        Part {
            name: "book",
            ox: 12.0,
            oy: 22.0,
            len: 30.8551,
            w: 2.0,
        },
        Part {
            name: "plus",
            ox: 12.0,
            oy: 10.0,
            len: 6.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.42,
            keys: &[0.0, -6.3, 90.0],
            times: &[0.0, 0.1071, 1.0],
            eases: &[2, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.42,
            keys: &[1.0, 1.3, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.09,
            dur: 0.42,
            keys: &[0.0, 0.8, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 5],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M19 17H7.5a2.5 2.5 0 0 0 0 5H19\"/><path d=\"M19 17V5a2 2 0 0 0-2-2H7.5A2.5 2.5 0 0 0 5 5.5v14\"/></g><g\u{1}><path d=\"M12 7v6\"/><path d=\"M9 10h6\"/></g>",
};

/// Lifted and dropped: it lands with a stretch and a flash of ink.
static BOOKMARK: Pack = Pack {
    id: "bookmark",
    dur: 0.4667,
    parts: &[
        Part {
            name: "mark",
            ox: 12.0,
            oy: 20.5,
            len: 56.4566,
            w: 2.0,
        },
        Part {
            name: "flash",
            ox: 12.0,
            oy: 20.5,
            len: 0.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.48, -3.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.9888, 1.07, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Opacity,
            t0: 0.165,
            dur: 0.3,
            keys: &[0.0, 0.24, 0.0],
            times: &[0.0, 0.2, 1.0],
            eases: &[7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M6.5 6A2.5 2.5 0 0 1 9 3.5h6A2.5 2.5 0 0 1 17.5 6v14.5l-5.5-4-5.5 4z\"/><g\u{1}><path d=\"M6.5 6A2.5 2.5 0 0 1 9 3.5h6A2.5 2.5 0 0 1 17.5 6v14.5l-5.5-4-5.5 4z\" fill=\"white\" stroke=\"none\"/></g></g>",
};

/// The plus twists and pops; the lens swells after it and the handle kicks out.
static ZOOM_IN: Pack = Pack {
    id: "zoom-in",
    dur: 0.5,
    parts: &[
        Part {
            name: "lens",
            ox: 10.5,
            oy: 10.5,
            len: 40.8465,
            w: 2.0,
        },
        Part {
            name: "handle",
            ox: 19.0,
            oy: 19.0,
            len: 4.1012,
            w: 2.0,
        },
        Part {
            name: "plus",
            ox: 10.5,
            oy: 10.5,
            len: 5.4,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 2,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.4,
            keys: &[0.0, -6.3, 90.0],
            times: &[0.0, 0.1125, 1.0],
            eases: &[2, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.42,
            keys: &[1.0, 1.35, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.07,
            dur: 0.42,
            keys: &[1.0, 1.1, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.07,
            dur: 0.42,
            keys: &[0.0, 0.9, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.07,
            dur: 0.42,
            keys: &[0.0, 0.9, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M4 10.5A6.5 6.5 0 1 1 17 10.5A6.5 6.5 0 1 1 4 10.5Z\"/></g><g\u{1}><path d=\"M17.6 17.6l2.9 2.9\"/></g><g\u{1}><path d=\"M10.5 7.8v5.4\"/><path d=\"M7.8 10.5h5.4\"/></g>",
};

/// The minus tucks in; the lens shrinks after it and the handle draws close.
static ZOOM_OUT: Pack = Pack {
    id: "zoom-out",
    dur: 0.5,
    parts: &[
        Part {
            name: "lens",
            ox: 10.5,
            oy: 10.5,
            len: 40.8465,
            w: 2.0,
        },
        Part {
            name: "handle",
            ox: 19.0,
            oy: 19.0,
            len: 4.1012,
            w: 2.0,
        },
        Part {
            name: "minus",
            ox: 10.5,
            oy: 10.5,
            len: 5.4,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 2,
            prop: Prop::ScaleX,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.096, 0.4, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.5, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.07,
            dur: 0.42,
            keys: &[1.0, 0.9, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.07,
            dur: 0.42,
            keys: &[0.0, -0.9, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.07,
            dur: 0.42,
            keys: &[0.0, -0.9, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 2,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M4 10.5A6.5 6.5 0 1 1 17 10.5A6.5 6.5 0 1 1 4 10.5Z\"/></g><g\u{1}><path d=\"M17.6 17.6l2.9 2.9\"/></g><g\u{1}><g\u{2}><path d=\"M7.8 10.5h5.4\"/></g></g>",
};

/// Held by the handle, the glass sweeps a small arc and catches the light.
static SEARCH: Pack = Pack {
    id: "search",
    dur: 0.4667,
    parts: &[
        Part {
            name: "glass",
            ox: 20.5,
            oy: 20.5,
            len: 40.8465,
            w: 2.0,
        },
        Part {
            name: "glint",
            ox: 10.5,
            oy: 10.5,
            len: 4.3974,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 2.4, -15.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.08, -0.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.08, -0.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Opacity,
            t0: 0.11,
            dur: 0.3,
            keys: &[0.0, 0.9, 0.0],
            times: &[0.0, 0.3, 1.0],
            eases: &[7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M4 10.5A6.5 6.5 0 1 1 17 10.5A6.5 6.5 0 1 1 4 10.5Z\"/><path d=\"M17.6 17.6l2.9 2.9\"/><g\u{1}><path d=\"M7.023 9.568A3.6 3.6 0 0 1 10.186 6.914\"/></g></g>",
};

/// Heads push to the edges while the bar stretches to fill the width.
static MOVE_HORIZONTAL: Pack = Pack {
    id: "move-horizontal",
    dur: 0.4667,
    parts: &[
        Part {
            name: "l",
            ox: 4.5,
            oy: 12.0,
            len: 9.8995,
            w: 2.0,
        },
        Part {
            name: "r",
            ox: 19.5,
            oy: 12.0,
            len: 9.8995,
            w: 2.0,
        },
        Part {
            name: "bar",
            ox: 12.0,
            oy: 12.0,
            len: 8.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.272, -1.7, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.4, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.272, 1.7, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.4, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::ScaleX,
            t0: 0.04,
            dur: 0.42,
            keys: &[1.0, 1.28, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 0,
            width: true,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><g\u{2}><path d=\"M6.5 8.5L3 12l3.5 3.5\"/></g></g><g\u{1}><g\u{2}><path d=\"M17.5 8.5L21 12l-3.5 3.5\"/></g></g><g\u{1}><path d=\"M8 12h8\"/></g>",
};

/// Four ticks dive at the page, which swells to meet them: fit.
static SHRINK: Pack = Pack {
    id: "shrink",
    dur: 0.5167,
    parts: &[
        Part {
            name: "page",
            ox: 12.0,
            oy: 12.0,
            len: 30.5682,
            w: 2.0,
        },
        Part {
            name: "tl",
            ox: 4.75,
            oy: 4.75,
            len: 3.5355,
            w: 2.0,
        },
        Part {
            name: "tr",
            ox: 19.25,
            oy: 4.75,
            len: 3.5355,
            w: 2.0,
        },
        Part {
            name: "br",
            ox: 19.25,
            oy: 19.25,
            len: 3.5355,
            w: 2.0,
        },
        Part {
            name: "bl",
            ox: 4.75,
            oy: 19.25,
            len: 3.5355,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.224, 1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.224, 1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.224, -1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.224, -1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::X,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, 0.224, -1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, -0.224, 1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 4,
            prop: Prop::X,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, -0.224, 1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 4,
            prop: Prop::Y,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, 0.224, -1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.08,
            dur: 0.42,
            keys: &[1.0, 1.13, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
        Hole {
            part: 4,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M10.5 7L13.5 7A2 2 0 0 1 15.5 9L15.5 15A2 2 0 0 1 13.5 17L10.5 17A2 2 0 0 1 8.5 15L8.5 9A2 2 0 0 1 10.5 7Z\"/></g><g\u{1}><path d=\"M3.5 3.5L6 6\"/></g><g\u{1}><path d=\"M20.5 3.5L18 6\"/></g><g\u{1}><path d=\"M20.5 20.5L18 18\"/></g><g\u{1}><path d=\"M3.5 20.5L6 18\"/></g>",
};

/// The page lifts away and the next one rises from beneath it.
static RECTANGLE_VERTICAL: Pack = Pack {
    id: "rectangle-vertical",
    dur: 0.4667,
    parts: &[
        Part {
            name: "next",
            ox: 12.0,
            oy: 12.0,
            len: 53.4795,
            w: 2.0,
        },
        Part {
            name: "page",
            ox: 12.0,
            oy: 12.0,
            len: 49.7795,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.24, -1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.0048, 0.97, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.04,
            dur: 0.38,
            keys: &[4.0, 0.0],
            times: &[],
            eases: &[1],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.04,
            dur: 0.38,
            keys: &[0.0, 0.5, 0.0],
            times: &[0.0, 0.3, 1.0],
            eases: &[7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M9.8 3L14.2 3A3.8 3.8 0 0 1 18 6.8L18 17.2A3.8 3.8 0 0 1 14.2 21L9.8 21A3.8 3.8 0 0 1 6 17.2L6 6.8A3.8 3.8 0 0 1 9.8 3Z\"/></g><g\u{1}><path d=\"M18 11.77L18 17.2A3.8 3.8 0 0 1 14.2 21L9.8 21A3.8 3.8 0 0 1 6 17.2L6 6.8A3.8 3.8 0 0 1 9.8 3L14.2 3A3.8 3.8 0 0 1 18 6.8L18 8.07\"/></g>",
};

/// The letter ducks, then grows from its baseline like a size change.
static TYPE: Pack = Pack {
    id: "type",
    dur: 0.4667,
    parts: &[Part {
        name: "t",
        ox: 12.0,
        oy: 18.0,
        len: 13.0,
        w: 2.0,
    }],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.9712, 1.18, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::StrokeW,
            t0: 0.06,
            dur: 0.37,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 0,
            width: true,
        },
    ],
    svg: "<g\u{1}><g\u{2}><path d=\"M5.5 6.5h13\"/><path d=\"M12 6.5V18\"/><path d=\"M9.5 18h5\"/></g></g>",
};

/// The sun lifts and the ridge is redrawn beneath it.
static IMAGE: Pack = Pack {
    id: "image",
    dur: 0.4667,
    parts: &[
        Part {
            name: "frame",
            ox: 12.0,
            oy: 12.0,
            len: 60.5783,
            w: 2.0,
        },
        Part {
            name: "sun",
            ox: 8.7,
            oy: 8.7,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "hill",
            ox: 12.0,
            oy: 12.0,
            len: 15.3512,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.24, -1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.952, 1.3, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::PathLen,
            t0: 0.05,
            dur: 0.35,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2, 1.0],
            eases: &[3, 1],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M16.37 3L16.5 3A4.5 4.5 0 0 1 21 7.5L21 16.5A4.5 4.5 0 0 1 16.5 21L7.5 21A4.5 4.5 0 0 1 3 16.5L3 7.5A4.5 4.5 0 0 1 7.5 3L12.67 3\"/></g><g\u{1}><path d=\"M7 8.7a1.7 1.7 0 1 0 3.4 0a1.7 1.7 0 1 0 -3.4 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M6.8 16.2l3.7-4 2.6 2.6 1.9-1.9 2.4 2.6\"/></g>",
};

/// The grid is ruled in: the column first, then each row.
static TABLE: Pack = Pack {
    id: "table",
    dur: 0.4333,
    parts: &[
        Part {
            name: "frame",
            ox: 12.0,
            oy: 12.0,
            len: 60.5783,
            w: 2.0,
        },
        Part {
            name: "col",
            ox: 12.0,
            oy: 12.0,
            len: 10.0,
            w: 2.0,
        },
        Part {
            name: "r1",
            ox: 12.0,
            oy: 12.0,
            len: 10.0,
            w: 2.0,
        },
        Part {
            name: "r2",
            ox: 12.0,
            oy: 12.0,
            len: 10.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::PathLen,
            t0: 0.055,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 3,
            prop: Prop::PathLen,
            t0: 0.11,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M16.37 3L16.5 3A4.5 4.5 0 0 1 21 7.5L21 16.5A4.5 4.5 0 0 1 16.5 21L7.5 21A4.5 4.5 0 0 1 3 16.5L3 7.5A4.5 4.5 0 0 1 7.5 3L12.67 3\"/></g><g\u{1}><path d=\"M11 7v10\"/></g><g\u{1}><path d=\"M7 10.2h10\"/></g><g\u{1}><path d=\"M7 14.4h10\"/></g>",
};

/// Bars duck and shoot up from the baseline one after another.
static CHART_COLUMN_INCREASING: Pack = Pack {
    id: "chart-column-increasing",
    dur: 0.6,
    parts: &[
        Part {
            name: "b1",
            ox: 6.5,
            oy: 20.0,
            len: 4.5,
            w: 2.0,
        },
        Part {
            name: "b2",
            ox: 12.0,
            oy: 20.0,
            len: 9.5,
            w: 2.0,
        },
        Part {
            name: "b3",
            ox: 17.5,
            oy: 20.0,
            len: 15.5,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.096, 0.4, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.06,
            dur: 0.465,
            keys: &[1.0, 1.096, 0.4, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::ScaleY,
            t0: 0.12,
            dur: 0.465,
            keys: &[1.0, 1.096, 0.4, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::StrokeW,
            t0: 0.23,
            dur: 0.37,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 2,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M6.5 20v-4.5\"/></g><g\u{1}><path d=\"M12 20v-9.5\"/></g><g\u{1}><g\u{2}><path d=\"M17.5 20V4.5\"/></g></g>",
};

/// The marker swipes and a heavy band of ink is laid under its nib.
static HIGHLIGHTER: Pack = Pack {
    id: "highlighter",
    dur: 0.4833,
    parts: &[
        Part {
            name: "marker",
            ox: 8.0,
            oy: 16.0,
            len: 33.2985,
            w: 2.0,
        },
        Part {
            name: "band",
            ox: 9.0,
            oy: 20.5,
            len: 10.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.512, 3.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.64, -4.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.03,
            dur: 0.35,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.11,
            dur: 0.37,
            keys: &[2.0, 2.9, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M14.01 5.49Q15 4.5 15.99 5.49L18.51 8.01Q19.5 9 18.51 9.99L13.49 15.01Q12.5 16 11.1 16L9.4 16Q8 16 8 14.6L8 12.9Q8 11.5 8.99 10.51Z\"/></g><g\u{1}><g\u{2}><path d=\"M4 20.5h10\"/></g></g>",
};

/// The letter hops and the bar is swept in beneath it.
static BASELINE: Pack = Pack {
    id: "baseline",
    dur: 0.5167,
    parts: &[
        Part {
            name: "a",
            ox: 12.0,
            oy: 15.5,
            len: 22.8473,
            w: 2.0,
        },
        Part {
            name: "cross",
            ox: 12.0,
            oy: 11.5,
            len: 5.2,
            w: 2.0,
        },
        Part {
            name: "bar",
            ox: 12.0,
            oy: 20.0,
            len: 14.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.256, -1.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::PathLen,
            t0: 0.06,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::StrokeW,
            t0: 0.14,
            dur: 0.37,
            keys: &[2.0, 2.7, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 2,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M7.5 15.5L12 5l4.5 10.5\"/><g\u{1}><path d=\"M9.4 11.5h5.2\"/></g></g><g\u{1}><g\u{2}><path d=\"M5 20h14\"/></g></g>",
};

/// The rule is drawn with pressure; the letter bobs when it lands.
static UNDERLINE: Pack = Pack {
    id: "underline",
    dur: 0.5833,
    parts: &[
        Part {
            name: "u",
            ox: 12.0,
            oy: 15.0,
            len: 26.1392,
            w: 2.0,
        },
        Part {
            name: "rule",
            ox: 12.0,
            oy: 20.0,
            len: 14.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.09,
            dur: 0.37,
            keys: &[2.0, 2.7, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.15,
            dur: 0.42,
            keys: &[0.0, 0.9, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.15,
            dur: 0.42,
            keys: &[1.0, 0.93, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M7.5 4.5v6a4.5 4.5 0 0 0 9 0v-6\"/></g><g\u{1}><g\u{2}><path d=\"M5 20h14\"/></g></g>",
};

/// The pencil rocks on its point and leaves a scribble that fades.
static PENCIL: Pack = Pack {
    id: "pencil",
    dur: 0.5,
    parts: &[
        Part {
            name: "pencil",
            ox: 3.5,
            oy: 20.5,
            len: 47.2915,
            w: 2.0,
        },
        Part {
            name: "ink",
            ox: 12.0,
            oy: 22.0,
            len: 9.8109,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 1.44, -9.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.224, 1.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.096, -0.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.06,
            dur: 0.3,
            keys: &[0.0, 1.0],
            times: &[],
            eases: &[1],
        },
        Seg {
            part: 1,
            prop: Prop::Opacity,
            t0: 0.06,
            dur: 0.44,
            keys: &[0.0, 0.8, 0.8, 0.0],
            times: &[0.0, 0.2, 0.6, 1.0],
            eases: &[7, 7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M14.651 4.849Q15.5 4 16.349 4.849L19.151 7.651Q20 8.5 19.151 9.349L9.349 19.151Q8.5 20 7.306 20.119L4.694 20.381Q3.5 20.5 3.619 19.306L3.881 16.694Q4 15.5 4.849 14.651Z\"/><path d=\"M13 6.5l4.5 4.5\"/></g><g\u{1}><path d=\"M9 22c1.6-1.6 3 1.2 4.6 0s3-1.2 4.4 0\"/></g>",
};

/// Pressed on, the note swings from its pinned corner and the fold lifts.
static STICKY_NOTE: Pack = Pack {
    id: "sticky-note",
    dur: 0.4833,
    parts: &[
        Part {
            name: "note",
            ox: 3.5,
            oy: 3.5,
            len: 59.981,
            w: 2.0,
        },
        Part {
            name: "fold",
            ox: 17.5,
            oy: 17.5,
            len: 10.9275,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.8, -5.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.06,
            dur: 0.42,
            keys: &[1.0, 0.72, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M3.5 7A3.5 3.5 0 0 1 7 3.5h10A3.5 3.5 0 0 1 20.5 7v7.5l-6 6H7A3.5 3.5 0 0 1 3.5 17z\"/><path d=\"M7.5 8.5h6\"/><g\u{1}><path d=\"M14.5 20.5V17a2.5 2.5 0 0 1 2.5-2.5h3.5\"/></g></g>",
};

/// The lattice slips apart, uprights one way and rails the other, and re-forms.
static HASH: Pack = Pack {
    id: "hash",
    dur: 0.5167,
    parts: &[
        Part {
            name: "v",
            ox: 12.0,
            oy: 12.0,
            len: 16.1245,
            w: 2.0,
        },
        Part {
            name: "h",
            ox: 12.0,
            oy: 12.0,
            len: 15.5,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.24, -1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.045,
            dur: 0.465,
            keys: &[0.0, -0.24, 1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M9.5 4L7.5 20\"/><path d=\"M16.5 4l-2 16\"/></g><g\u{1}><path d=\"M4.5 9H20\"/><path d=\"M3.5 15H19\"/></g>",
};

/// A nudge in its own direction, with a faint echo catching up from behind.
static CHEVRON_LEFT: Pack = Pack {
    id: "chevron-left",
    dur: 0.4667,
    parts: &[
        Part {
            name: "echo",
            ox: 12.0,
            oy: 12.0,
            len: 15.5563,
            w: 2.0,
        },
        Part {
            name: "chev",
            ox: 12.0,
            oy: 12.0,
            len: 15.5563,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.384, -2.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.5, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.04,
            dur: 0.34,
            keys: &[3.2, 0.0],
            times: &[],
            eases: &[1],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.04,
            dur: 0.34,
            keys: &[0.0, 0.45, 0.0],
            times: &[0.0, 0.3, 1.0],
            eases: &[7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M14.5 6.5L9 12l5.5 5.5\"/></g><g\u{1}><g\u{2}><path d=\"M14.5 6.5L9 12l5.5 5.5\"/></g></g>",
};

/// A nudge in its own direction, with a faint echo catching up from behind.
static CHEVRON_RIGHT: Pack = Pack {
    id: "chevron-right",
    dur: 0.4667,
    parts: &[
        Part {
            name: "echo",
            ox: 12.0,
            oy: 12.0,
            len: 15.5563,
            w: 2.0,
        },
        Part {
            name: "chev",
            ox: 12.0,
            oy: 12.0,
            len: 15.5563,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.384, 2.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.5, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.04,
            dur: 0.34,
            keys: &[-3.2, 0.0],
            times: &[],
            eases: &[1],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.04,
            dur: 0.34,
            keys: &[0.0, 0.45, 0.0],
            times: &[0.0, 0.3, 1.0],
            eases: &[7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M9.5 6.5L15 12l-5.5 5.5\"/></g><g\u{1}><g\u{2}><path d=\"M9.5 6.5L15 12l-5.5 5.5\"/></g></g>",
};

/// A nudge in its own direction, with a faint echo catching up from behind.
static CHEVRON_UP: Pack = Pack {
    id: "chevron-up",
    dur: 0.4667,
    parts: &[
        Part {
            name: "echo",
            ox: 12.0,
            oy: 12.0,
            len: 15.5563,
            w: 2.0,
        },
        Part {
            name: "chev",
            ox: 12.0,
            oy: 12.0,
            len: 15.5563,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.384, -2.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.5, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.04,
            dur: 0.34,
            keys: &[3.2, 0.0],
            times: &[],
            eases: &[1],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.04,
            dur: 0.34,
            keys: &[0.0, 0.45, 0.0],
            times: &[0.0, 0.3, 1.0],
            eases: &[7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M6.5 14.5L12 9l5.5 5.5\"/></g><g\u{1}><g\u{2}><path d=\"M6.5 14.5L12 9l5.5 5.5\"/></g></g>",
};

/// A nudge in its own direction, with a faint echo catching up from behind.
static CHEVRON_DOWN: Pack = Pack {
    id: "chevron-down",
    dur: 0.4667,
    parts: &[
        Part {
            name: "echo",
            ox: 12.0,
            oy: 12.0,
            len: 15.5563,
            w: 2.0,
        },
        Part {
            name: "chev",
            ox: 12.0,
            oy: 12.0,
            len: 15.5563,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.384, 2.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.5, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.04,
            dur: 0.34,
            keys: &[-3.2, 0.0],
            times: &[],
            eases: &[1],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.04,
            dur: 0.34,
            keys: &[0.0, 0.45, 0.0],
            times: &[0.0, 0.3, 1.0],
            eases: &[7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M6.5 9.5L12 15l5.5-5.5\"/></g><g\u{1}><g\u{2}><path d=\"M6.5 9.5L12 15l5.5-5.5\"/></g></g>",
};

/// The head jabs first; the shaft stretches after it and both snap home.
static ARROW_DOWN: Pack = Pack {
    id: "arrow-down",
    dur: 0.4667,
    parts: &[
        Part {
            name: "shaft",
            ox: 12.0,
            oy: 4.0,
            len: 9.5,
            w: 2.0,
        },
        Part {
            name: "head",
            ox: 12.0,
            oy: 16.0,
            len: 15.5563,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.352, 2.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.5, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.035,
            dur: 0.42,
            keys: &[0.0, 1.2, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.035,
            dur: 0.42,
            keys: &[1.0, 1.16, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M12 4v9.5\"/></g><g\u{1}><g\u{2}><path d=\"M6.5 13L12 18.5 17.5 13\"/></g></g>",
};

/// The head jabs first; the shaft stretches after it and both snap home.
static ARROW_UP: Pack = Pack {
    id: "arrow-up",
    dur: 0.4667,
    parts: &[
        Part {
            name: "shaft",
            ox: 12.0,
            oy: 20.0,
            len: 9.5,
            w: 2.0,
        },
        Part {
            name: "head",
            ox: 12.0,
            oy: 8.0,
            len: 15.5563,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.352, -2.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.5, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.035,
            dur: 0.42,
            keys: &[0.0, -1.2, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.035,
            dur: 0.42,
            keys: &[1.0, 1.16, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M12 20v-9.5\"/></g><g\u{1}><g\u{2}><path d=\"M6.5 11L12 5.5 17.5 11\"/></g></g>",
};

/// Two arrows pull in opposite directions, one a beat behind the other.
static ARROW_DOWN_UP: Pack = Pack {
    id: "arrow-down-up",
    dur: 0.5333,
    parts: &[
        Part {
            name: "down",
            ox: 7.5,
            oy: 12.0,
            len: 15.0,
            w: 2.0,
        },
        Part {
            name: "up",
            ox: 16.5,
            oy: 12.0,
            len: 15.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.288, 1.8, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.9904, 1.06, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.055,
            dur: 0.465,
            keys: &[0.0, 0.288, -1.8, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.055,
            dur: 0.465,
            keys: &[1.0, 0.9904, 1.06, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M7.5 4.5v15\"/><path d=\"M4 16l3.5 3.5L11 16\"/></g><g\u{1}><path d=\"M16.5 19.5v-15\"/><path d=\"M13 8l3.5-3.5L20 8\"/></g>",
};

/// Two lanes slide past each other, each in its own direction.
static ARROW_RIGHT_LEFT: Pack = Pack {
    id: "arrow-right-left",
    dur: 0.5333,
    parts: &[
        Part {
            name: "top",
            ox: 12.0,
            oy: 8.0,
            len: 16.0,
            w: 2.0,
        },
        Part {
            name: "bottom",
            ox: 12.0,
            oy: 16.0,
            len: 16.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.32, 2.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleX,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.992, 1.05, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.055,
            dur: 0.465,
            keys: &[0.0, 0.32, -2.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleX,
            t0: 0.055,
            dur: 0.465,
            keys: &[1.0, 0.992, 1.05, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M4 8h16\"/><path d=\"M16.5 4.5L20 8l-3.5 3.5\"/></g><g\u{1}><path d=\"M20 16H4\"/><path d=\"M7.5 12.5L4 16l3.5 3.5\"/></g>",
};

/// The arrow pushes off from the line, which recoils and springs back.
static ARROW_RIGHT_FROM_LINE: Pack = Pack {
    id: "arrow-right-from-line",
    dur: 0.4667,
    parts: &[
        Part {
            name: "line",
            ox: 4.0,
            oy: 12.0,
            len: 14.0,
            w: 2.0,
        },
        Part {
            name: "arrow",
            ox: 14.0,
            oy: 12.0,
            len: 11.5,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.384, 2.4, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.045,
            dur: 0.42,
            keys: &[0.0, -1.0, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.045,
            dur: 0.42,
            keys: &[1.0, 0.82, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M4 5v14\"/></g><g\u{1}><path d=\"M8.5 12H20\"/><path d=\"M15.5 7.5L20 12l-4.5 4.5\"/></g>",
};

/// The curve is re-inked backwards to the head, which kicks as it arrives.
static UNDO_2: Pack = Pack {
    id: "undo-2",
    dur: 0.6,
    parts: &[
        Part {
            name: "curve",
            ox: 12.0,
            oy: 12.0,
            len: 25.2067,
            w: 2.0,
        },
        Part {
            name: "head",
            ox: 6.0,
            oy: 9.0,
            len: 11.3137,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.35,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 0,
            prop: Prop::PathOff,
            t0: 0.0,
            dur: 0.35,
            keys: &[0.0, 0.94, 0.0],
            times: &[0.0, 0.2, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.18,
            dur: 0.42,
            keys: &[0.0, -1.8, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.18,
            dur: 0.42,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M8.5 9h6a5 5 0 0 1 0 10H11\"/></g><g\u{1}><g\u{2}><path d=\"M8 5L4 9l4 4\"/></g></g>",
};

/// One deliberate lap that winds back first and lands exactly on itself.
static REFRESH_CW: Pack = Pack {
    id: "refresh-cw",
    dur: 0.5667,
    parts: &[Part {
        name: "spin",
        ox: 12.0,
        oy: 12.0,
        len: 43.0072,
        w: 2.0,
    }],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.56,
            keys: &[0.0, -25.2, 360.0],
            times: &[0.0, 0.0804, 1.0],
            eases: &[2, 5],
        },
        Seg {
            part: 0,
            prop: Prop::StrokeW,
            t0: 0.06,
            dur: 0.37,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 0,
            width: true,
        },
    ],
    svg: "<g\u{1}><g\u{2}><path d=\"M18.304 7.075A8 8 0 1 1 12 4\"/><path d=\"M9.6 1.3L12.8 4 9.6 6.8\"/></g></g>",
};

/// The loop swaps its two lanes: half a turn that lands on itself.
static REPEAT: Pack = Pack {
    id: "repeat",
    dur: 0.5333,
    parts: &[Part {
        name: "loop",
        ox: 12.0,
        oy: 12.0,
        len: 18.4986,
        w: 2.0,
    }],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.52,
            keys: &[0.0, -12.6, 180.0],
            times: &[0.0, 0.0865, 1.0],
            eases: &[2, 5],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.42,
            keys: &[1.0, 0.9, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[Hole {
        part: 0,
        width: false,
    }],
    svg: "<g\u{1}><path d=\"M4 11V9.5A3.5 3.5 0 0 1 7.5 6H19\"/><path d=\"M16 3l3 3-3 3\"/><path d=\"M20 13v1.5a3.5 3.5 0 0 1-3.5 3.5H5\"/><path d=\"M8 15l-3 3 3 3\"/></g>",
};

/// The halves part, the bar between them thins, and they click back together.
static LINK: Pack = Pack {
    id: "link",
    dur: 0.4667,
    parts: &[
        Part {
            name: "l",
            ox: 6.0,
            oy: 12.0,
            len: 17.1392,
            w: 2.0,
        },
        Part {
            name: "r",
            ox: 18.0,
            oy: 12.0,
            len: 17.1392,
            w: 2.0,
        },
        Part {
            name: "bar",
            ox: 12.0,
            oy: 12.0,
            len: 6.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.24, -1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.24, 1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::ScaleX,
            t0: 0.04,
            dur: 0.42,
            keys: &[1.0, 0.6, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::StrokeW,
            t0: 0.04,
            dur: 0.42,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 2,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M9.5 16.5H8a4.5 4.5 0 0 1 0-9h1.5\"/></g><g\u{1}><path d=\"M14.5 7.5H16a4.5 4.5 0 0 1 0 9h-1.5\"/></g><g\u{1}><g\u{2}><path d=\"M9 12h6\"/></g></g>",
};

/// Rows type themselves out: each bullet pops and its line wipes in.
static LIST: Pack = Pack {
    id: "list",
    dur: 0.4333,
    parts: &[
        Part {
            name: "d1",
            ox: 4.5,
            oy: 6.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "l1",
            ox: 12.0,
            oy: 12.0,
            len: 11.0,
            w: 2.0,
        },
        Part {
            name: "d2",
            ox: 4.5,
            oy: 12.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "l2",
            ox: 12.0,
            oy: 12.0,
            len: 11.0,
            w: 2.0,
        },
        Part {
            name: "d3",
            ox: 4.5,
            oy: 18.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "l3",
            ox: 12.0,
            oy: 12.0,
            len: 7.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.29,
            keys: &[1.0, 1.5, 1.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.26,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.1923, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::Scale,
            t0: 0.065,
            dur: 0.29,
            keys: &[1.0, 1.5, 1.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::PathLen,
            t0: 0.065,
            dur: 0.26,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.1923, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 4,
            prop: Prop::Scale,
            t0: 0.13,
            dur: 0.29,
            keys: &[1.0, 1.5, 1.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 5,
            prop: Prop::PathLen,
            t0: 0.13,
            dur: 0.26,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.1923, 1.0],
            eases: &[3, 1],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
        Hole {
            part: 4,
            width: false,
        },
        Hole {
            part: 5,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M3.2 6a1.3 1.3 0 1 0 2.6 0a1.3 1.3 0 1 0 -2.6 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M9 6h11\"/></g><g\u{1}><path d=\"M3.2 12a1.3 1.3 0 1 0 2.6 0a1.3 1.3 0 1 0 -2.6 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M9 12h11\"/></g><g\u{1}><path d=\"M3.2 18a1.3 1.3 0 1 0 2.6 0a1.3 1.3 0 1 0 -2.6 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M9 18h7\"/></g>",
};

/// The tree unfolds: the trunk is drawn down, then each child grows out of it.
static LIST_TREE: Pack = Pack {
    id: "list-tree",
    dur: 0.4667,
    parts: &[
        Part {
            name: "root",
            ox: 8.5,
            oy: 5.5,
            len: 9.0,
            w: 2.0,
        },
        Part {
            name: "trunk",
            ox: 12.0,
            oy: 12.0,
            len: 10.4276,
            w: 2.0,
        },
        Part {
            name: "c1",
            ox: 12.0,
            oy: 12.0,
            len: 10.0,
            w: 2.0,
        },
        Part {
            name: "c2",
            ox: 12.0,
            oy: 12.0,
            len: 6.5,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.37,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.33,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2121, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::PathLen,
            t0: 0.08,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 3,
            prop: Prop::PathLen,
            t0: 0.15,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 0,
            width: true,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
    ],
    svg: "<g\u{1}><g\u{2}><path d=\"M4 5.5h9\"/></g></g><g\u{1}><path d=\"M6 10v5a2.5 2.5 0 0 0 2.5 2.5H10\"/></g><g\u{1}><path d=\"M10 11.5h10\"/></g><g\u{1}><path d=\"M13.5 17.5H20\"/></g>",
};

/// Ticks are struck one after the other; each row nudges as it is checked.
static LIST_CHECKS: Pack = Pack {
    id: "list-checks",
    dur: 0.65,
    parts: &[
        Part {
            name: "k1",
            ox: 12.0,
            oy: 12.0,
            len: 8.1435,
            w: 2.0,
        },
        Part {
            name: "l1",
            ox: 16.0,
            oy: 6.5,
            len: 7.5,
            w: 2.0,
        },
        Part {
            name: "k2",
            ox: 12.0,
            oy: 12.0,
            len: 8.1435,
            w: 2.0,
        },
        Part {
            name: "l2",
            ox: 16.0,
            oy: 16.5,
            len: 7.5,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.25,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.09,
            dur: 0.42,
            keys: &[0.0, 1.3, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::PathLen,
            t0: 0.13,
            dur: 0.25,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 3,
            prop: Prop::X,
            t0: 0.22,
            dur: 0.42,
            keys: &[0.0, 1.3, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M3.5 6.5l2 2 3.5-4\"/></g><g\u{1}><path d=\"M12.5 6.5H20\"/></g><g\u{1}><path d=\"M3.5 16.5l2 2 3.5-4\"/></g><g\u{1}><path d=\"M12.5 16.5H20\"/></g>",
};

/// Three drops bounce in turn: the familiar ‘more to come’.
static ELLIPSIS: Pack = Pack {
    id: "ellipsis",
    dur: 0.5667,
    parts: &[
        Part {
            name: "d1",
            ox: 5.0,
            oy: 12.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "d2",
            ox: 12.0,
            oy: 12.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "d3",
            ox: 19.0,
            oy: 12.0,
            len: 0.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.425,
            keys: &[0.0, 0.48, -3.0, 0.0],
            times: &[0.0, 0.1059, 0.3412, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.425,
            keys: &[1.0, 0.9808, 1.12, 1.0],
            times: &[0.0, 0.1059, 0.3412, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.07,
            dur: 0.425,
            keys: &[0.0, 0.48, -3.0, 0.0],
            times: &[0.0, 0.1059, 0.3412, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.07,
            dur: 0.425,
            keys: &[1.0, 0.9808, 1.12, 1.0],
            times: &[0.0, 0.1059, 0.3412, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.14,
            dur: 0.425,
            keys: &[0.0, 0.48, -3.0, 0.0],
            times: &[0.0, 0.1059, 0.3412, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Scale,
            t0: 0.14,
            dur: 0.425,
            keys: &[1.0, 0.9808, 1.12, 1.0],
            times: &[0.0, 0.1059, 0.3412, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M3.3 12a1.7 1.7 0 1 0 3.4 0a1.7 1.7 0 1 0 -3.4 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M10.3 12a1.7 1.7 0 1 0 3.4 0a1.7 1.7 0 1 0 -3.4 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M17.3 12a1.7 1.7 0 1 0 3.4 0a1.7 1.7 0 1 0 -3.4 0Z\" fill=\"white\" stroke=\"none\"/></g>",
};

/// One confident stroke, short leg then long, pressed home at the end.
static CHECK: Pack = Pack {
    id: "check",
    dur: 0.6167,
    parts: &[Part {
        name: "tick",
        ox: 12.0,
        oy: 12.0,
        len: 21.5711,
        w: 2.0,
    }],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.33,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2121, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.19,
            dur: 0.42,
            keys: &[1.0, 1.12, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::StrokeW,
            t0: 0.19,
            dur: 0.37,
            keys: &[2.0, 2.6, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 0,
            width: true,
        },
    ],
    svg: "<g\u{1}><g\u{2}><path d=\"M4.5 12.5l5 5L19.5 7\"/></g></g>",
};

/// The ring swells and the tick is struck through its pen-lift.
static CIRCLE_CHECK: Pack = Pack {
    id: "circle-check",
    dur: 0.55,
    parts: &[
        Part {
            name: "ring",
            ox: 12.0,
            oy: 12.0,
            len: 50.1532,
            w: 2.0,
        },
        Part {
            name: "tick",
            ox: 12.0,
            oy: 12.0,
            len: 14.5751,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.9904, 1.06, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 5],
        },
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.04,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.18,
            dur: 0.37,
            keys: &[2.0, 2.6, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M20.577 9.274A9 9 0 1 1 16.72 4.337\"/></g><g\u{1}><g\u{2}><path d=\"M8 12.3l3 3 7-7.6\"/></g></g>",
};

/// The stem lifts and stamps, the drop pops, and the sign shivers.
static CIRCLE_ALERT: Pack = Pack {
    id: "circle-alert",
    dur: 0.6,
    parts: &[
        Part {
            name: "ring",
            ox: 12.0,
            oy: 12.0,
            len: 52.8535,
            w: 2.0,
        },
        Part {
            name: "stem",
            ox: 12.0,
            oy: 12.5,
            len: 5.0,
            w: 2.0,
        },
        Part {
            name: "drop",
            ox: 12.0,
            oy: 16.3,
            len: 0.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.256, -1.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.9808, 1.12, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Scale,
            t0: 0.17,
            dur: 0.42,
            keys: &[1.0, 1.7, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.15,
            dur: 0.33,
            keys: &[0.0, 8.0, -5.0, 2.5, 0.0],
            times: &[0.0, 0.2, 0.5, 0.78, 1.0],
            eases: &[3, 3, 3, 3],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M19.529 7.069A9 9 0 1 1 16.931 4.471\"/></g><g\u{1}><path d=\"M12 7.5v5\"/></g><g\u{1}><path d=\"M10.7 16.3a1.3 1.3 0 1 0 2.6 0a1.3 1.3 0 1 0 -2.6 0Z\" fill=\"white\" stroke=\"none\"/></g>",
};

/// The hands are wound forward one full lap; the dial swells with them.
static CLOCK: Pack = Pack {
    id: "clock",
    dur: 0.5833,
    parts: &[
        Part {
            name: "dial",
            ox: 12.0,
            oy: 12.0,
            len: 52.8535,
            w: 2.0,
        },
        Part {
            name: "hands",
            ox: 12.0,
            oy: 12.0,
            len: 8.6715,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.58,
            keys: &[0.0, -25.2, 360.0],
            times: &[0.0, 0.0776, 1.0],
            eases: &[2, 5],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.04,
            dur: 0.42,
            keys: &[1.0, 1.05, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 5],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M19.529 7.069A9 9 0 1 1 16.931 4.471\"/></g><g\u{1}><path d=\"M12 7v5l3.2 1.8\"/></g>",
};

/// The strike is slashed across and the sign recoils: denied.
static BAN: Pack = Pack {
    id: "ban",
    dur: 0.5,
    parts: &[
        Part {
            name: "ring",
            ox: 12.0,
            oy: 12.0,
            len: 52.8535,
            w: 2.0,
        },
        Part {
            name: "slash",
            ox: 12.0,
            oy: 12.0,
            len: 11.3137,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.29,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.09,
            dur: 0.37,
            keys: &[2.0, 2.6, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.08,
            dur: 0.42,
            keys: &[0.0, 16.0, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.08,
            dur: 0.42,
            keys: &[1.0, 0.94, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M19.529 7.069A9 9 0 1 1 16.931 4.471\"/></g><g\u{1}><g\u{2}><path d=\"M8 8l8 8\"/></g></g>",
};

/// The leaning book is knocked upright and falls back; its neighbours hop.
static LIBRARY_BIG: Pack = Pack {
    id: "library-big",
    dur: 0.6,
    parts: &[
        Part {
            name: "b1",
            ox: 5.25,
            oy: 20.0,
            len: 38.2545,
            w: 2.0,
        },
        Part {
            name: "b2",
            ox: 11.75,
            oy: 20.0,
            len: 38.2545,
            w: 2.0,
        },
        Part {
            name: "lean",
            ox: 20.4,
            oy: 20.0,
            len: 29.4996,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 2,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -1.76, 11.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.07,
            dur: 0.465,
            keys: &[0.0, 0.208, -1.3, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.125,
            dur: 0.465,
            keys: &[0.0, 0.208, -1.3, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M4.6 4L5.9 4A1.6 1.6 0 0 1 7.5 5.6L7.5 18.4A1.6 1.6 0 0 1 5.9 20L4.6 20A1.6 1.6 0 0 1 3 18.4L3 5.6A1.6 1.6 0 0 1 4.6 4Z\"/></g><g\u{1}><path d=\"M11.1 4L12.4 4A1.6 1.6 0 0 1 14 5.6L14 18.4A1.6 1.6 0 0 1 12.4 20L11.1 20A1.6 1.6 0 0 1 9.5 18.4L9.5 5.6A1.6 1.6 0 0 1 11.1 4Z\"/></g><g\u{1}><path d=\"M20.176 20.053Q18.9 20.3 18.629 19.029L16.571 9.371Q16.3 8.1 17.57 7.822L18.23 7.678Q19.5 7.4 19.759 8.674L21.741 18.426Q22 19.7 20.724 19.947Z\"/></g>",
};

/// Spines hop along the shelf in turn; the slanted one tips as it lands.
static SQUARE_LIBRARY: Pack = Pack {
    id: "square-library",
    dur: 0.5833,
    parts: &[
        Part {
            name: "frame",
            ox: 12.0,
            oy: 12.0,
            len: 60.5783,
            w: 2.0,
        },
        Part {
            name: "s1",
            ox: 8.0,
            oy: 16.0,
            len: 8.0,
            w: 2.0,
        },
        Part {
            name: "s2",
            ox: 12.0,
            oy: 16.0,
            len: 8.0,
            w: 2.0,
        },
        Part {
            name: "s3",
            ox: 16.8,
            oy: 16.0,
            len: 7.6688,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.208, -1.3, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.055,
            dur: 0.465,
            keys: &[0.0, 0.208, -1.3, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Rotate,
            t0: 0.11,
            dur: 0.465,
            keys: &[0.0, 2.08, -13.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M16.37 3L16.5 3A4.5 4.5 0 0 1 21 7.5L21 16.5A4.5 4.5 0 0 1 16.5 21L7.5 21A4.5 4.5 0 0 1 3 16.5L3 7.5A4.5 4.5 0 0 1 7.5 3L12.67 3\"/></g><g\u{1}><path d=\"M8 8v8\"/></g><g\u{1}><path d=\"M12 8v8\"/></g><g\u{1}><path d=\"M15.2 8.5l1.6 7.5\"/></g>",
};

/// A tap: the folder squashes on its base and springs back.
static FOLDER: Pack = Pack {
    id: "folder",
    dur: 0.4667,
    parts: &[Part {
        name: "body",
        ox: 12.0,
        oy: 20.0,
        len: 57.4614,
        w: 2.0,
    }],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.0224, 0.86, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleX,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.9936, 1.04, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[Hole {
        part: 0,
        width: false,
    }],
    svg: "<g\u{1}><path d=\"M3 12.45V17a3 3 0 0 0 3 3h12a3 3 0 0 0 3-3V9.8a3 3 0 0 0-3-3h-5.3a2 2 0 0 1-1.6-.8l-.9-1.2A2 2 0 0 0 8.6 4H6a3 3 0 0 0-3 3v1.75\"/></g>",
};

/// The front flap shears open from its hinge and swings shut again.
static FOLDER_OPEN: Pack = Pack {
    id: "folder-open",
    dur: 0.4833,
    parts: &[
        Part {
            name: "back",
            ox: 12.0,
            oy: 12.0,
            len: 30.797,
            w: 2.0,
        },
        Part {
            name: "flap",
            ox: 12.7,
            oy: 20.0,
            len: 42.509,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::SkewX,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 2.08, -13.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.016, 0.9, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.05,
            dur: 0.42,
            keys: &[0.0, -0.5, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 5],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M4 17.5V7a3 3 0 0 1 3-3h1.9a2 2 0 0 1 1.6.8l.9 1.2a2 2 0 0 0 1.6.8h5a2.5 2.5 0 0 1 2.5 2\"/></g><g\u{1}><path d=\"M7.6 20Q5.8 20 6.289 18.268L7.711 13.232Q8.2 11.5 10 11.5L20.2 11.5Q22 11.5 21.511 13.232L20.089 18.268Q19.6 20 17.8 20Z\"/></g>",
};

/// The plus drops in with a quarter twist and the folder bounces under it.
static FOLDER_PLUS: Pack = Pack {
    id: "folder-plus",
    dur: 0.6,
    parts: &[
        Part {
            name: "body",
            ox: 12.0,
            oy: 20.0,
            len: 57.4614,
            w: 2.0,
        },
        Part {
            name: "plus",
            ox: 12.0,
            oy: 13.5,
            len: 6.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.512, -3.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.42,
            keys: &[0.0, -6.3, 90.0],
            times: &[0.0, 0.1071, 1.0],
            eases: &[2, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.165,
            dur: 0.42,
            keys: &[1.0, 0.92, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M3 12.45V17a3 3 0 0 0 3 3h12a3 3 0 0 0 3-3V9.8a3 3 0 0 0-3-3h-5.3a2 2 0 0 1-1.6-.8l-.9-1.2A2 2 0 0 0 8.6 4H6a3 3 0 0 0-3 3v1.75\"/></g><g\u{1}><path d=\"M12 10.5v6\"/><path d=\"M9 13.5h6\"/></g>",
};

/// The lens scans sideways across the folder, tilting with the motion.
static FOLDER_SEARCH: Pack = Pack {
    id: "folder-search",
    dur: 0.5167,
    parts: &[
        Part {
            name: "body",
            ox: 12.0,
            oy: 12.0,
            len: 57.4614,
            w: 2.0,
        },
        Part {
            name: "glass",
            ox: 11.3,
            oy: 13.0,
            len: 16.3386,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.505,
            keys: &[0.0, -0.416, 2.6, 2.6, 0.0],
            times: &[0.0, 0.0891, 0.3267, 0.4059, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.505,
            keys: &[0.0, -1.6, 10.0, 10.0, 0.0],
            times: &[0.0, 0.0891, 0.3267, 0.4059, 1.0],
            eases: &[2, 1, 0, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M3 12.45V17a3 3 0 0 0 3 3h12a3 3 0 0 0 3-3V9.8a3 3 0 0 0-3-3h-5.3a2 2 0 0 1-1.6-.8l-.9-1.2A2 2 0 0 0 8.6 4H6a3 3 0 0 0-3 3v1.75\"/></g><g\u{1}><path d=\"M8.7 13A2.6 2.6 0 1 1 13.9 13A2.6 2.6 0 1 1 8.7 13Z\"/><path d=\"M13.4 15.1l1.8 1.8\"/></g>",
};

/// The front folder lifts while the one behind dips and dims.
static FOLDERS: Pack = Pack {
    id: "folders",
    dur: 0.5167,
    parts: &[
        Part {
            name: "back",
            ox: 9.0,
            oy: 15.0,
            len: 26.2131,
            w: 2.0,
        },
        Part {
            name: "front",
            ox: 14.25,
            oy: 18.5,
            len: 47.2011,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.24, -1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.08, 0.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.04,
            dur: 0.465,
            keys: &[0.0, -0.176, 1.1, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.04,
            dur: 0.465,
            keys: &[0.0, 0.08, -0.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.04,
            dur: 0.465,
            keys: &[1.0, 1.0, 0.5, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M3 8v10.5a3 3 0 0 0 3 3h11\"/></g><g\u{1}><path d=\"M7 10.5V9a2.5 2.5 0 0 1 2.5-2.5h1.6a2 2 0 0 1 1.6.8l.6.9a2 2 0 0 0 1.6.8H19a2.5 2.5 0 0 1 2.5 2.5V16a2.5 2.5 0 0 1-2.5 2.5H9.5A2.5 2.5 0 0 1 7 16z\"/></g>",
};

/// The lens runs up the page, swells where it finds the line, and returns.
static FILE_SEARCH: Pack = Pack {
    id: "file-search",
    dur: 0.5167,
    parts: &[
        Part {
            name: "page",
            ox: 12.0,
            oy: 12.0,
            len: 38.7043,
            w: 2.0,
        },
        Part {
            name: "glass",
            ox: 15.5,
            oy: 16.5,
            len: 18.8522,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.515,
            keys: &[0.0, 0.512, -3.2, -3.2, 0.0],
            times: &[0.0, 0.0874, 0.3204, 0.4175, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.515,
            keys: &[0.0, 0.256, -1.6, -1.6, 0.0],
            times: &[0.0, 0.0874, 0.3204, 0.4175, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.515,
            keys: &[1.0, 0.984, 1.1, 1.1, 1.0],
            times: &[0.0, 0.0874, 0.3204, 0.4175, 1.0],
            eases: &[2, 1, 0, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M10 21H8a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3h5.5L19 8.5v2\"/></g><g\u{1}><path d=\"M12.5 16.5A3 3 0 1 1 18.5 16.5A3 3 0 1 1 12.5 16.5Z\"/><path d=\"M17.9 18.9l2.1 2.1\"/></g>",
};

/// The activity lights blink out in turn and the drive settles.
static HARD_DRIVE: Pack = Pack {
    id: "hard-drive",
    dur: 0.45,
    parts: &[
        Part {
            name: "body",
            ox: 12.0,
            oy: 12.0,
            len: 53.4363,
            w: 2.0,
        },
        Part {
            name: "led1",
            ox: 8.0,
            oy: 15.8,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "led2",
            ox: 11.4,
            oy: 15.8,
            len: 0.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.03,
            dur: 0.42,
            keys: &[0.0, 0.8, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 5],
        },
        Seg {
            part: 1,
            prop: Prop::Opacity,
            t0: 0.0,
            dur: 0.26,
            keys: &[1.0, 0.1, 1.0],
            times: &[],
            eases: &[7, 7],
        },
        Seg {
            part: 2,
            prop: Prop::Opacity,
            t0: 0.11,
            dur: 0.26,
            keys: &[1.0, 0.1, 1.0],
            times: &[],
            eases: &[7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M16.65 5L17 5A4 4 0 0 1 21 9L21 15A4 4 0 0 1 17 19L7 19A4 4 0 0 1 3 15L3 9A4 4 0 0 1 7 5L12.95 5\"/><path d=\"M7.5 12h9\"/><g\u{1}><path d=\"M6.9 15.8a1.1 1.1 0 1 0 2.2 0a1.1 1.1 0 1 0 -2.2 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M10.3 15.8a1.1 1.1 0 1 0 2.2 0a1.1 1.1 0 1 0 -2.2 0Z\" fill=\"white\" stroke=\"none\"/></g></g>",
};

/// The head drops, shaft stretches, and tray bounces on impact.
static DOWNLOAD: Pack = Pack {
    id: "download",
    dur: 0.5667,
    parts: &[
        Part {
            name: "tray",
            ox: 12.0,
            oy: 20.0,
            len: 21.4261,
            w: 2.0,
        },
        Part {
            name: "shaft",
            ox: 12.0,
            oy: 3.5,
            len: 6.0,
            w: 2.0,
        },
        Part {
            name: "head",
            ox: 12.0,
            oy: 12.0,
            len: 12.7279,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.416, 2.6, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.5, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.035,
            dur: 0.42,
            keys: &[0.0, 1.6, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.035,
            dur: 0.42,
            keys: &[1.0, 1.2, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.14,
            dur: 0.42,
            keys: &[0.0, 1.0, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.14,
            dur: 0.42,
            keys: &[1.0, 0.86, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 2,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M4.5 15.5V17a3 3 0 0 0 3 3h9a3 3 0 0 0 3-3v-1.5\"/></g><g\u{1}><path d=\"M12 3.5v6\"/></g><g\u{1}><g\u{2}><path d=\"M7.5 9.5l4.5 4.5 4.5-4.5\"/></g></g>",
};

/// The lid lifts from its hinge; the can shudders and the ribs drop.
static TRASH_2: Pack = Pack {
    id: "trash-2",
    dur: 0.5833,
    parts: &[
        Part {
            name: "lid",
            ox: 20.0,
            oy: 6.5,
            len: 16.0,
            w: 2.0,
        },
        Part {
            name: "can",
            ox: 12.0,
            oy: 20.5,
            len: 30.4261,
            w: 2.0,
        },
        Part {
            name: "ribs",
            ox: 12.0,
            oy: 14.5,
            len: 5.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -2.72, 17.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.128, -0.8, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Rotate,
            t0: 0.11,
            dur: 0.34,
            keys: &[0.0, 3.0, -2.2, 1.0, 0.0],
            times: &[0.0, 0.2, 0.5, 0.78, 1.0],
            eases: &[3, 3, 3, 3],
        },
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.15,
            dur: 0.42,
            keys: &[0.0, 0.7, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M4 6.5h16\"/><path d=\"M9.5 6.5V5A1.5 1.5 0 0 1 11 3.5h2A1.5 1.5 0 0 1 14.5 5v1.5\"/></g><g\u{1}><path d=\"M6 10v7.5a3 3 0 0 0 3 3h6a3 3 0 0 0 3-3V10\"/><g\u{1}><path d=\"M10 12v5\"/><path d=\"M14 12v5\"/></g></g>",
};

/// The units un-stack a touch and re-seat; their lights blink in turn.
static SERVER: Pack = Pack {
    id: "server",
    dur: 0.5167,
    parts: &[
        Part {
            name: "u1",
            ox: 12.0,
            oy: 7.0,
            len: 45.1954,
            w: 2.0,
        },
        Part {
            name: "led1",
            ox: 7.0,
            oy: 7.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "u2",
            ox: 12.0,
            oy: 17.0,
            len: 45.1954,
            w: 2.0,
        },
        Part {
            name: "led2",
            ox: 7.0,
            oy: 17.0,
            len: 0.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.144, -0.9, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.04,
            dur: 0.465,
            keys: &[0.0, -0.144, 0.9, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Opacity,
            t0: 0.06,
            dur: 0.24,
            keys: &[1.0, 0.1, 1.0],
            times: &[],
            eases: &[7, 7],
        },
        Seg {
            part: 3,
            prop: Prop::Opacity,
            t0: 0.17,
            dur: 0.24,
            keys: &[1.0, 0.1, 1.0],
            times: &[],
            eases: &[7, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M5.8 3.5L18.2 3.5A2.8 2.8 0 0 1 21 6.3L21 7.7A2.8 2.8 0 0 1 18.2 10.5L5.8 10.5A2.8 2.8 0 0 1 3 7.7L3 6.3A2.8 2.8 0 0 1 5.8 3.5Z\"/><path d=\"M11.5 7H17\"/><g\u{1}><path d=\"M5.85 7a1.15 1.15 0 1 0 2.3 0a1.15 1.15 0 1 0 -2.3 0Z\" fill=\"white\" stroke=\"none\"/></g></g><g\u{1}><path d=\"M5.8 13.5L18.2 13.5A2.8 2.8 0 0 1 21 16.3L21 17.7A2.8 2.8 0 0 1 18.2 20.5L5.8 20.5A2.8 2.8 0 0 1 3 17.7L3 16.3A2.8 2.8 0 0 1 5.8 13.5Z\"/><path d=\"M11.5 17H17\"/><g\u{1}><path d=\"M5.85 17a1.15 1.15 0 1 0 2.3 0a1.15 1.15 0 1 0 -2.3 0Z\" fill=\"white\" stroke=\"none\"/></g></g>",
};

/// The core pulses while the pins fire clockwise around the die.
static CPU: Pack = Pack {
    id: "cpu",
    dur: 0.4667,
    parts: &[
        Part {
            name: "die",
            ox: 12.0,
            oy: 12.0,
            len: 38.8522,
            w: 2.0,
        },
        Part {
            name: "core",
            ox: 12.0,
            oy: 12.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "top",
            ox: 12.0,
            oy: 12.0,
            len: 1.6,
            w: 2.0,
        },
        Part {
            name: "right",
            ox: 12.0,
            oy: 12.0,
            len: 1.6,
            w: 2.0,
        },
        Part {
            name: "bottom",
            ox: 12.0,
            oy: 12.0,
            len: 1.6,
            w: 2.0,
        },
        Part {
            name: "left",
            ox: 12.0,
            oy: 12.0,
            len: 1.6,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.936, 1.4, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.29,
            keys: &[0.0, -1.1, 0.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::X,
            t0: 0.05,
            dur: 0.29,
            keys: &[0.0, 1.1, 0.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 4,
            prop: Prop::Y,
            t0: 0.1,
            dur: 0.29,
            keys: &[0.0, 1.1, 0.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 5,
            prop: Prop::X,
            t0: 0.15,
            dur: 0.29,
            keys: &[0.0, -1.1, 0.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
        Hole {
            part: 4,
            width: false,
        },
        Hole {
            part: 5,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M9.5 6.5L14.5 6.5A3 3 0 0 1 17.5 9.5L17.5 14.5A3 3 0 0 1 14.5 17.5L9.5 17.5A3 3 0 0 1 6.5 14.5L6.5 9.5A3 3 0 0 1 9.5 6.5Z\"/></g><g\u{1}><path d=\"M10 12a2 2 0 1 0 4 0a2 2 0 1 0 -4 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M9.5 2v1.6\"/><path d=\"M14.5 2v1.6\"/></g><g\u{1}><path d=\"M22 9.5h-1.6\"/><path d=\"M22 14.5h-1.6\"/></g><g\u{1}><path d=\"M9.5 22v-1.6\"/><path d=\"M14.5 22v-1.6\"/></g><g\u{1}><path d=\"M2 9.5h1.6\"/><path d=\"M2 14.5h1.6\"/></g>",
};

/// The key winds back, turns about its bow and clicks home.
static KEY_ROUND: Pack = Pack {
    id: "key-round",
    dur: 0.4667,
    parts: &[Part {
        name: "key",
        ox: 7.5,
        oy: 16.5,
        len: 28.2783,
        w: 2.0,
    }],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -4.16, 26.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::StrokeW,
            t0: 0.06,
            dur: 0.37,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 0,
            width: true,
        },
    ],
    svg: "<g\u{1}><g\u{2}><path d=\"M3 16.5A4.5 4.5 0 1 1 12 16.5A4.5 4.5 0 1 1 3 16.5Z\"/><path d=\"M10.7 13.3L20 4\"/><path d=\"M15.6 8.4l2.6 2.6\"/><path d=\"M18.6 5.4l1.9 1.9\"/></g></g>",
};

/// The letters take a breath and a spark escapes through the frame's pen-lift.
static AI_MARK: Pack = Pack {
    id: "ai-mark",
    dur: 0.5333,
    parts: &[
        Part {
            name: "frame",
            ox: 12.0,
            oy: 12.0,
            len: 56.5783,
            w: 2.0,
        },
        Part {
            name: "a",
            ox: 9.5,
            oy: 15.5,
            len: 15.0785,
            w: 2.0,
        },
        Part {
            name: "i",
            ox: 16.3,
            oy: 15.5,
            len: 7.0,
            w: 2.0,
        },
        Part {
            name: "spark",
            ox: 19.6,
            oy: 2.5,
            len: 3.4,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.16, -1.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.984, 1.1, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::ScaleY,
            t0: 0.055,
            dur: 0.465,
            keys: &[1.0, 0.9712, 1.18, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Opacity,
            t0: 0.12,
            dur: 0.32,
            keys: &[0.0, 1.0, 0.0],
            times: &[0.0, 0.3, 1.0],
            eases: &[7, 7],
        },
        Seg {
            part: 3,
            prop: Prop::Scale,
            t0: 0.12,
            dur: 0.32,
            keys: &[0.3, 1.15],
            times: &[],
            eases: &[1],
        },
        Seg {
            part: 3,
            prop: Prop::Rotate,
            t0: 0.12,
            dur: 0.32,
            keys: &[0.0, 90.0],
            times: &[],
            eases: &[1],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M16.85 4.5L17 4.5A4.5 4.5 0 0 1 21.5 9L21.5 15A4.5 4.5 0 0 1 17 19.5L7 19.5A4.5 4.5 0 0 1 2.5 15L2.5 9A4.5 4.5 0 0 1 7 4.5L13.15 4.5\"/></g><g\u{1}><path d=\"M6.7 15.5l2.8-7 2.8 7\"/><path d=\"M7.9 12.8h3.2\"/></g><g\u{1}><path d=\"M16.3 8.5v7\"/></g><g\u{1}><path d=\"M19.6 .8v3.4\"/><path d=\"M17.9 2.5h3.4\"/></g>",
};

/// A back-and-forth: the source glyph speaks, then the target letter answers.
static LANGUAGES: Pack = Pack {
    id: "languages",
    dur: 0.6167,
    parts: &[
        Part {
            name: "src",
            ox: 7.5,
            oy: 9.0,
            len: 8.0,
            w: 2.0,
        },
        Part {
            name: "dst",
            ox: 17.0,
            oy: 16.0,
            len: 19.6977,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.208, -1.3, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.9872, 1.08, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.15,
            dur: 0.465,
            keys: &[0.0, 0.208, -1.3, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.15,
            dur: 0.465,
            keys: &[1.0, 0.9872, 1.08, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M3.5 6.5h8\"/><path d=\"M7.5 4v2.5\"/><path d=\"M9.8 6.5c-.6 3.4-2.6 5.8-6 7.5\"/><path d=\"M5.2 9.8c1 1.9 2.5 3.2 4.6 4\"/></g><g\u{1}><path d=\"M13 20l4-9 4 9\"/><path d=\"M14.4 17h5.2\"/></g>",
};

/// The feather lifts and sways from its quill: weightless.
static FEATHER: Pack = Pack {
    id: "feather",
    dur: 0.6167,
    parts: &[Part {
        name: "feather",
        ox: 4.0,
        oy: 21.0,
        len: 37.3726,
        w: 2.0,
    }],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.605,
            keys: &[0.0, 1.76, -11.0, 0.0],
            times: &[0.0, 0.0744, 0.3719, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.605,
            keys: &[0.0, 0.224, -1.4, 0.0],
            times: &[0.0, 0.0744, 0.3719, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[Hole {
        part: 0,
        width: false,
    }],
    svg: "<g\u{1}><path d=\"M8.5 16.5V14c0-6 5-10 11-9.5.5 6.5-4 12-11 12z\"/><path d=\"M4 21l9-9\"/></g>",
};

/// Three lines fold up into the first, then open out again: a summary.
static SCROLL_TEXT: Pack = Pack {
    id: "scroll-text",
    dur: 0.5667,
    parts: &[
        Part {
            name: "page",
            ox: 12.0,
            oy: 12.0,
            len: 53.7795,
            w: 2.0,
        },
        Part {
            name: "l1",
            ox: 12.0,
            oy: 12.0,
            len: 6.0,
            w: 2.0,
        },
        Part {
            name: "l2",
            ox: 12.0,
            oy: 12.0,
            len: 6.0,
            w: 2.0,
        },
        Part {
            name: "l3",
            ox: 12.0,
            oy: 12.0,
            len: 3.5,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.525,
            keys: &[0.0, 0.352, -2.2, -2.2, 0.0],
            times: &[0.0, 0.0857, 0.3143, 0.4286, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Opacity,
            t0: 0.0,
            dur: 0.525,
            keys: &[1.0, 1.0, 0.25, 0.25, 1.0],
            times: &[0.0, 0.0857, 0.3143, 0.4286, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Y,
            t0: 0.03,
            dur: 0.525,
            keys: &[0.0, 0.704, -4.4, -4.4, 0.0],
            times: &[0.0, 0.0857, 0.3143, 0.4286, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Opacity,
            t0: 0.03,
            dur: 0.525,
            keys: &[1.0, 1.0, 0.1, 0.1, 1.0],
            times: &[0.0, 0.0857, 0.3143, 0.4286, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 1,
            prop: Prop::StrokeW,
            t0: 0.15,
            dur: 0.37,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.04,
            dur: 0.42,
            keys: &[1.0, 0.95, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 5],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 1,
            width: true,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M19 11.25L19 17.2A3.8 3.8 0 0 1 15.2 21L8.8 21A3.8 3.8 0 0 1 5 17.2L5 6.8A3.8 3.8 0 0 1 8.8 3L15.2 3A3.8 3.8 0 0 1 19 6.8L19 7.55\"/></g><g\u{1}><g\u{2}><path d=\"M9 8h6\"/></g></g><g\u{1}><path d=\"M9 12h6\"/></g><g\u{1}><path d=\"M9 16h3.5\"/></g>",
};

/// A press: the head pulls back, compresses and lunges forward.
static PLAY: Pack = Pack {
    id: "play",
    dur: 0.4667,
    parts: &[Part {
        name: "tri",
        ox: 12.0,
        oy: 12.0,
        len: 35.9861,
        w: 2.0,
    }],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.32, 2.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleX,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.0192, 0.88, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::StrokeW,
            t0: 0.0,
            dur: 0.465,
            keys: &[2.0, 2.0, 2.4, 2.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 0,
            width: true,
        },
    ],
    svg: "<g\u{1}><g\u{2}><path d=\"M8 7.2Q8 5 9.856 6.181L17.144 10.819Q19 12 17.144 13.181L9.856 17.819Q8 19 8 16.8Z\"/></g></g>",
};

/// The bars squeeze toward each other and part again: a held breath.
static PAUSE: Pack = Pack {
    id: "pause",
    dur: 0.4667,
    parts: &[
        Part {
            name: "b1",
            ox: 8.0,
            oy: 12.0,
            len: 32.5681,
            w: 2.0,
        },
        Part {
            name: "b2",
            ox: 16.0,
            oy: 12.0,
            len: 32.5681,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.208, 1.3, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.0192, 0.88, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.208, -1.3, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.0192, 0.88, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M8 5L8 5A2 2 0 0 1 10 7L10 17A2 2 0 0 1 8 19L8 19A2 2 0 0 1 6 17L6 7A2 2 0 0 1 8 5Z\"/></g><g\u{1}><path d=\"M16 5L16 5A2 2 0 0 1 18 7L18 17A2 2 0 0 1 16 19L16 19A2 2 0 0 1 14 17L14 7A2 2 0 0 1 16 5Z\"/></g>",
};

/// The head lunges back into the bar, which recoils and springs back.
static SKIP_BACK: Pack = Pack {
    id: "skip-back",
    dur: 0.5,
    parts: &[
        Part {
            name: "tri",
            ox: 14.0,
            oy: 12.0,
            len: 33.1346,
            w: 2.0,
        },
        Part {
            name: "bar",
            ox: 5.0,
            oy: 12.0,
            len: 12.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.352, -2.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.075,
            dur: 0.42,
            keys: &[0.0, -1.2, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.075,
            dur: 0.42,
            keys: &[1.0, 0.84, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M19 7.5Q19 5.5 17.323 6.59L10.677 10.91Q9 12 10.677 13.09L17.323 17.41Q19 18.5 19 16.5Z\"/></g><g\u{1}><path d=\"M5 6v12\"/></g>",
};

/// The head lunges into the bar, which recoils and springs back.
static SKIP_FORWARD: Pack = Pack {
    id: "skip-forward",
    dur: 0.5,
    parts: &[
        Part {
            name: "tri",
            ox: 10.0,
            oy: 12.0,
            len: 33.1346,
            w: 2.0,
        },
        Part {
            name: "bar",
            ox: 19.0,
            oy: 12.0,
            len: 12.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.352, 2.2, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::X,
            t0: 0.075,
            dur: 0.42,
            keys: &[0.0, 1.2, 0.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.075,
            dur: 0.42,
            keys: &[1.0, 0.84, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M5 7.5Q5 5.5 6.677 6.59L13.323 10.91Q15 12 13.323 13.09L6.677 17.41Q5 18.5 5 16.5Z\"/></g><g\u{1}><path d=\"M19 6v12\"/></g>",
};

/// The square is pressed in while the ring's pen-lift is swept once around.
static CIRCLE_STOP: Pack = Pack {
    id: "circle-stop",
    dur: 0.5667,
    parts: &[
        Part {
            name: "ring",
            ox: 12.0,
            oy: 12.0,
            len: 52.8535,
            w: 2.0,
        },
        Part {
            name: "sq",
            ox: 12.0,
            oy: 12.0,
            len: 21.2545,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.56,
            keys: &[0.0, -25.2, 360.0],
            times: &[0.0, 0.0804, 1.0],
            eases: &[2, 5],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 1.048, 0.7, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M19.529 7.069A9 9 0 1 1 16.931 4.471\"/></g><g\u{1}><path d=\"M10.6 9L13.4 9A1.6 1.6 0 0 1 15 10.6L15 13.4A1.6 1.6 0 0 1 13.4 15L10.6 15A1.6 1.6 0 0 1 9 13.4L9 10.6A1.6 1.6 0 0 1 10.6 9Z\"/></g>",
};

/// The headphones bob to a beat while a small equaliser plays between the cups.
static HEADPHONES: Pack = Pack {
    id: "headphones",
    dur: 0.4667,
    parts: &[
        Part {
            name: "phones",
            ox: 12.0,
            oy: 12.0,
            len: 24.5653,
            w: 2.0,
        },
        Part {
            name: "beat",
            ox: 12.0,
            oy: 19.5,
            len: 2.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -0.144, 0.9, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Opacity,
            t0: 0.0,
            dur: 0.46,
            keys: &[0.0, 0.85, 0.85, 0.0],
            times: &[0.0, 0.15, 0.75, 1.0],
            eases: &[7, 7, 7],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.0,
            dur: 0.46,
            keys: &[0.4, 1.15, 0.55, 1.0, 0.5],
            times: &[],
            eases: &[3, 3, 3, 3],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M4.5 12.5V12a7.5 7.5 0 0 1 15 0v.5\"/><path d=\"M5 15.5L5.5 15.5A2 2 0 0 1 7.5 17.5L7.5 19A2 2 0 0 1 5.5 21L5 21A2 2 0 0 1 3 19L3 17.5A2 2 0 0 1 5 15.5Z\"/><path d=\"M18.5 15.5L19 15.5A2 2 0 0 1 21 17.5L21 19A2 2 0 0 1 19 21L18.5 21A2 2 0 0 1 16.5 19L16.5 17.5A2 2 0 0 1 18.5 15.5Z\"/></g><g\u{1}><path d=\"M10 19.5v-2\"/><path d=\"M12 19.5v-4\"/><path d=\"M14 19.5v-2.6\"/></g>",
};

/// The waveform is traced left to right, then rings out vertically.
static AUDIO_WAVEFORM: Pack = Pack {
    id: "audio-waveform",
    dur: 0.6833,
    parts: &[Part {
        name: "wave",
        ox: 12.0,
        oy: 12.0,
        len: 43.1966,
        w: 2.0,
    }],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.39,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.1795, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 0,
            prop: Prop::ScaleY,
            t0: 0.25,
            dur: 0.42,
            keys: &[1.0, 1.22, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[Hole {
        part: 0,
        width: false,
    }],
    svg: "<g\u{1}><path d=\"M2.5 12h2l2-6 3.5 12 3-9 2 5 1.5-2h5\"/></g>",
};

/// The cone swells and two waves are thrown outward, one after the other.
static VOLUME_2: Pack = Pack {
    id: "volume-2",
    dur: 0.5833,
    parts: &[
        Part {
            name: "cone",
            ox: 7.0,
            oy: 12.0,
            len: 32.8279,
            w: 2.0,
        },
        Part {
            name: "w1",
            ox: 12.0,
            oy: 12.0,
            len: 6.5987,
            w: 2.0,
        },
        Part {
            name: "w2",
            ox: 12.0,
            oy: 12.0,
            len: 12.8816,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.465,
            keys: &[1.0, 0.984, 1.1, 1.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.04,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::PathOff,
            t0: 0.04,
            dur: 0.31,
            keys: &[0.0, 0.47, 0.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::PathLen,
            t0: 0.11,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::PathOff,
            t0: 0.11,
            dur: 0.31,
            keys: &[0.0, 0.47, 0.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::StrokeW,
            t0: 0.2,
            dur: 0.37,
            keys: &[2.0, 2.5, 2.0],
            times: &[0.0, 0.2973, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 2,
            width: true,
        },
    ],
    svg: "<g\u{1}><path d=\"M3.5 10.9Q3.5 9.5 4.9 9.5L5.4 9.5Q6.8 9.5 7.814 8.534L9.986 6.466Q11 5.5 11 6.9L11 17.1Q11 18.5 9.986 17.534L7.814 15.466Q6.8 14.5 5.4 14.5L4.9 14.5Q3.5 14.5 3.5 13.1Z\"/></g><g\u{1}><path d=\"M13.97 9.03A4.2 4.2 0 0 1 13.97 14.97\"/></g><g\u{1}><g\u{2}><path d=\"M16.798 6.202A8.2 8.2 0 0 1 16.798 17.798\"/></g></g>",
};

/// Rays advance one notch and flare while the core tightens.
static SUN: Pack = Pack {
    id: "sun",
    dur: 0.45,
    parts: &[
        Part {
            name: "core",
            ox: 12.0,
            oy: 12.0,
            len: 25.1363,
            w: 2.0,
        },
        Part {
            name: "rays",
            ox: 12.0,
            oy: 12.0,
            len: 2.2,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.44,
            keys: &[0.0, -3.15, 45.0],
            times: &[0.0, 0.1023, 1.0],
            eases: &[2, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.42,
            keys: &[1.0, 1.14, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.03,
            dur: 0.42,
            keys: &[1.0, 0.84, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M8 12A4 4 0 1 1 16 12A4 4 0 1 1 8 12Z\"/></g><g\u{1}><path d=\"M19.60 12.00L21.80 12.00\"/><path d=\"M17.37 17.37L18.93 18.93\"/><path d=\"M12.00 19.60L12.00 21.80\"/><path d=\"M6.63 17.37L5.07 18.93\"/><path d=\"M4.40 12.00L2.20 12.00\"/><path d=\"M6.63 6.63L5.07 5.07\"/><path d=\"M12.00 4.40L12.00 2.20\"/><path d=\"M17.37 6.63L18.93 5.07\"/></g>",
};

/// The moon tips back and two stars twinkle in the hollow of its crescent.
static MOON: Pack = Pack {
    id: "moon",
    dur: 0.4833,
    parts: &[
        Part {
            name: "moon",
            ox: 12.0,
            oy: 12.0,
            len: 55.1726,
            w: 2.0,
        },
        Part {
            name: "s1",
            ox: 16.8,
            oy: 6.1,
            len: 3.0,
            w: 2.0,
        },
        Part {
            name: "s2",
            ox: 20.0,
            oy: 9.7,
            len: 2.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 2.24, -14.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Opacity,
            t0: 0.09,
            dur: 0.3,
            keys: &[0.0, 1.0, 0.0],
            times: &[0.0, 0.3, 1.0],
            eases: &[7, 7],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.09,
            dur: 0.3,
            keys: &[0.3, 1.1],
            times: &[],
            eases: &[1],
        },
        Seg {
            part: 2,
            prop: Prop::Opacity,
            t0: 0.17,
            dur: 0.3,
            keys: &[0.0, 1.0, 0.0],
            times: &[0.0, 0.3, 1.0],
            eases: &[7, 7],
        },
        Seg {
            part: 2,
            prop: Prop::Scale,
            t0: 0.17,
            dur: 0.3,
            keys: &[0.3, 1.1],
            times: &[],
            eases: &[1],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M20.5 13.5A8.5 8.5 0 1 1 10.5 3.5a7.5 7.5 0 0 0 10 10z\"/></g><g\u{1}><path d=\"M16.8 4.6v3\"/><path d=\"M15.3 6.1h3\"/></g><g\u{1}><path d=\"M20 8.7v2\"/><path d=\"M19 9.7h2\"/></g>",
};

/// Dusk: the rays draw in and dim while the night's edge swings round.
static SUN_MOON: Pack = Pack {
    id: "sun-moon",
    dur: 0.5333,
    parts: &[
        Part {
            name: "core",
            ox: 12.0,
            oy: 12.0,
            len: 25.1363,
            w: 2.0,
        },
        Part {
            name: "rays",
            ox: 12.0,
            oy: 12.0,
            len: 2.2,
            w: 2.0,
        },
        Part {
            name: "edge",
            ox: 12.0,
            oy: 12.0,
            len: 15.7928,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.525,
            keys: &[1.0, 1.0288, 0.82, 0.82, 1.0],
            times: &[0.0, 0.0857, 0.3143, 0.4286, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Opacity,
            t0: 0.0,
            dur: 0.525,
            keys: &[1.0, 1.0, 0.3, 0.3, 1.0],
            times: &[0.0, 0.0857, 0.3143, 0.4286, 1.0],
            eases: &[2, 1, 0, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Rotate,
            t0: 0.04,
            dur: 0.465,
            keys: &[0.0, -4.48, 28.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M8 12A4 4 0 1 1 16 12A4 4 0 1 1 8 12Z\"/></g><g\u{1}><path d=\"M12.00 19.60L12.00 21.80\"/><path d=\"M6.63 17.37L5.07 18.93\"/><path d=\"M4.40 12.00L2.20 12.00\"/><path d=\"M6.63 6.63L5.07 5.07\"/><path d=\"M12.00 4.40L12.00 2.20\"/></g><g\u{1}><path d=\"M17.356 5.144A8.7 8.7 0 0 1 17.356 18.856\"/></g>",
};

/// The shadow swings a full turn around the disc, which dims as it is covered.
static ECLIPSE: Pack = Pack {
    id: "eclipse",
    dur: 0.5833,
    parts: &[
        Part {
            name: "disc",
            ox: 12.0,
            oy: 12.0,
            len: 52.8535,
            w: 2.0,
        },
        Part {
            name: "shadow",
            ox: 12.0,
            oy: 12.0,
            len: 13.0701,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.58,
            keys: &[0.0, -25.2, 360.0],
            times: &[0.0, 0.0776, 1.0],
            eases: &[2, 5],
        },
        Seg {
            part: 0,
            prop: Prop::Opacity,
            t0: 0.0,
            dur: 0.52,
            keys: &[1.0, 0.5, 0.5, 1.0],
            times: &[0.0, 0.3077, 0.5, 1.0],
            eases: &[1, 0, 7],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M19.529 7.069A9 9 0 1 1 16.931 4.471\"/></g><g\u{1}><path d=\"M12.067 17.674A7.2 7.2 0 0 1 12.067 6.326\"/></g>",
};

/// The palette tilts and its wells fill with ink in turn.
static PALETTE: Pack = Pack {
    id: "palette",
    dur: 0.4833,
    parts: &[
        Part {
            name: "blob",
            ox: 12.0,
            oy: 12.0,
            len: 58.7705,
            w: 2.0,
        },
        Part {
            name: "w1",
            ox: 7.4,
            oy: 12.2,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "w2",
            ox: 9.2,
            oy: 8.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "w3",
            ox: 13.4,
            oy: 6.8,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "w4",
            ox: 17.0,
            oy: 9.4,
            len: 0.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 1.6, -10.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.04,
            dur: 0.29,
            keys: &[1.0, 1.6, 1.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Scale,
            t0: 0.09,
            dur: 0.29,
            keys: &[1.0, 1.6, 1.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Scale,
            t0: 0.14,
            dur: 0.29,
            keys: &[1.0, 1.6, 1.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 4,
            prop: Prop::Scale,
            t0: 0.19,
            dur: 0.29,
            keys: &[1.0, 1.6, 1.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
        Hole {
            part: 4,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M12 3a9 9 0 0 0 0 18 2.3 2.3 0 0 0 1.8-3.8 2.3 2.3 0 0 1 1.7-3.8H17a4 4 0 0 0 4-4c0-3.6-4-6.4-9-6.4z\"/><g\u{1}><path d=\"M6.2 12.2a1.2 1.2 0 1 0 2.4 0a1.2 1.2 0 1 0 -2.4 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M8 8a1.2 1.2 0 1 0 2.4 0a1.2 1.2 0 1 0 -2.4 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M12.2 6.8a1.2 1.2 0 1 0 2.4 0a1.2 1.2 0 1 0 -2.4 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M15.8 9.4a1.2 1.2 0 1 0 2.4 0a1.2 1.2 0 1 0 -2.4 0Z\" fill=\"white\" stroke=\"none\"/></g></g>",
};

/// Three gusts blow through left to right, each inked in from its tail.
static WIND: Pack = Pack {
    id: "wind",
    dur: 0.4667,
    parts: &[
        Part {
            name: "g1",
            ox: 12.0,
            oy: 12.0,
            len: 22.2826,
            w: 2.0,
        },
        Part {
            name: "g2",
            ox: 12.0,
            oy: 12.0,
            len: 27.2826,
            w: 2.0,
        },
        Part {
            name: "g3",
            ox: 12.0,
            oy: 12.0,
            len: 19.2826,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.32,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.1875, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.07,
            dur: 0.32,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.1875, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::PathLen,
            t0: 0.14,
            dur: 0.32,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.1875, 1.0],
            eases: &[3, 1],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M3 8h10.5a2.5 2.5 0 1 0-2.5-2.5\"/></g><g\u{1}><path d=\"M3 12h15.5a2.5 2.5 0 1 1-2.5 2.5\"/></g><g\u{1}><path d=\"M3 16h7.5a2.5 2.5 0 1 1-2.5 2.5\"/></g>",
};

/// A heartbeat: lub-dub, with the pulse inked across it.
static KOKORO: Pack = Pack {
    id: "kokoro",
    dur: 0.5333,
    parts: &[
        Part {
            name: "heart",
            ox: 12.0,
            oy: 13.0,
            len: 51.8117,
            w: 2.0,
        },
        Part {
            name: "pulse",
            ox: 12.0,
            oy: 13.0,
            len: 14.2946,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.52,
            keys: &[1.0, 1.13, 1.0, 1.08, 1.0],
            times: &[0.0, 0.16, 0.4, 0.56, 1.0],
            eases: &[1, 3, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.06,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M12 20l-7.2-7.2a4.9 4.9 0 0 1 7-6.9l.2.2.2-.2a4.9 4.9 0 0 1 7 6.9z\"/><g\u{1}><path d=\"M7.6 11.6h2l1.2-2.2 2 4.4 1.3-2.2h2.4\"/></g></g>",
};

/// Three pipes sound in turn, the tallest first.
static PIPER: Pack = Pack {
    id: "piper",
    dur: 0.6,
    parts: &[
        Part {
            name: "p1",
            ox: 6.25,
            oy: 20.0,
            len: 29.9971,
            w: 2.0,
        },
        Part {
            name: "p2",
            ox: 12.0,
            oy: 20.0,
            len: 35.9971,
            w: 2.0,
        },
        Part {
            name: "p3",
            ox: 17.75,
            oy: 20.0,
            len: 23.9971,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::Y,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, 0.24, -1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 0,
            prop: Prop::Y,
            t0: 0.06,
            dur: 0.465,
            keys: &[0.0, 0.24, -1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Y,
            t0: 0.12,
            dur: 0.465,
            keys: &[0.0, 0.24, -1.5, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M6.25 7L6.25 7A1.75 1.75 0 0 1 8 8.75L8 18.25A1.75 1.75 0 0 1 6.25 20L6.25 20A1.75 1.75 0 0 1 4.5 18.25L4.5 8.75A1.75 1.75 0 0 1 6.25 7Z\"/></g><g\u{1}><path d=\"M12 4L12 4A1.75 1.75 0 0 1 13.75 5.75L13.75 18.25A1.75 1.75 0 0 1 12 20L12 20A1.75 1.75 0 0 1 10.25 18.25L10.25 5.75A1.75 1.75 0 0 1 12 4Z\"/></g><g\u{1}><path d=\"M17.75 10L17.75 10A1.75 1.75 0 0 1 19.5 11.75L19.5 18.25A1.75 1.75 0 0 1 17.75 20L17.75 20A1.75 1.75 0 0 1 16 18.25L16 11.75A1.75 1.75 0 0 1 17.75 10Z\"/></g>",
};

/// The head tilts, curious, and the eyes blink.
static KITTEN: Pack = Pack {
    id: "kitten",
    dur: 0.4667,
    parts: &[
        Part {
            name: "face",
            ox: 12.0,
            oy: 20.5,
            len: 56.708,
            w: 2.0,
        },
        Part {
            name: "eyes",
            ox: 12.0,
            oy: 12.4,
            len: 0.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::Rotate,
            t0: 0.0,
            dur: 0.465,
            keys: &[0.0, -1.12, 7.0, 0.0],
            times: &[0.0, 0.0968, 0.3548, 1.0],
            eases: &[2, 1, 4],
        },
        Seg {
            part: 1,
            prop: Prop::ScaleY,
            t0: 0.12,
            dur: 0.26,
            keys: &[1.0, 0.1, 1.0],
            times: &[0.0, 0.2308, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M4.5 5l3.8 2.3a8.6 8.6 0 0 1 7.4 0L19.5 5l-.6 5.2c.5 1 .7 2 .7 3 0 4.2-3.4 7.3-7.6 7.3s-7.6-3.1-7.6-7.3c0-1 .2-2 .7-3z\"/><path d=\"M10.8 16.2l1.2.9 1.2-.9\"/><g\u{1}><path d=\"M8.05 12.4a1.15 1.15 0 1 0 2.3 0a1.15 1.15 0 1 0 -2.3 0Z\" fill=\"white\" stroke=\"none\"/><path d=\"M13.65 12.4a1.15 1.15 0 1 0 2.3 0a1.15 1.15 0 1 0 -2.3 0Z\" fill=\"white\" stroke=\"none\"/></g></g>",
};

/// The wave is traced through and its nodes ring one after another.
static PHONON: Pack = Pack {
    id: "phonon",
    dur: 0.5333,
    parts: &[
        Part {
            name: "wave",
            ox: 12.0,
            oy: 12.0,
            len: 37.1153,
            w: 2.0,
        },
        Part {
            name: "n1",
            ox: 7.25,
            oy: 12.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "n2",
            ox: 12.0,
            oy: 12.0,
            len: 0.0,
            w: 2.0,
        },
        Part {
            name: "n3",
            ox: 16.75,
            oy: 12.0,
            len: 0.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.37,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.1892, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.09,
            dur: 0.29,
            keys: &[1.0, 1.6, 1.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 2,
            prop: Prop::Scale,
            t0: 0.16,
            dur: 0.29,
            keys: &[1.0, 1.6, 1.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
        Seg {
            part: 3,
            prop: Prop::Scale,
            t0: 0.23,
            dur: 0.29,
            keys: &[1.0, 1.6, 1.0],
            times: &[0.0, 0.2414, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
        Hole {
            part: 3,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M2.5 12c1.6-5 3.2-5 4.75 0s3.2 5 4.75 0 3.2-5 4.75 0 3.2 5 4.75 0\"/></g><g\u{1}><path d=\"M5.85 12a1.4 1.4 0 1 0 2.8 0a1.4 1.4 0 1 0 -2.8 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M10.6 12a1.4 1.4 0 1 0 2.8 0a1.4 1.4 0 1 0 -2.8 0Z\" fill=\"white\" stroke=\"none\"/></g><g\u{1}><path d=\"M15.35 12a1.4 1.4 0 1 0 2.8 0a1.4 1.4 0 1 0 -2.8 0Z\" fill=\"white\" stroke=\"none\"/></g>",
};

/// A pulse is traced across the screen and the stand widens under it.
static SYSTEM: Pack = Pack {
    id: "system",
    dur: 0.5,
    parts: &[
        Part {
            name: "screen",
            ox: 12.0,
            oy: 12.0,
            len: 51.809,
            w: 2.0,
        },
        Part {
            name: "pulse",
            ox: 12.0,
            oy: 12.0,
            len: 15.8881,
            w: 2.0,
        },
        Part {
            name: "base",
            ox: 12.0,
            oy: 20.5,
            len: 7.0,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 1,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 2,
            prop: Prop::ScaleX,
            t0: 0.08,
            dur: 0.42,
            keys: &[1.0, 1.25, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
        Hole {
            part: 2,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M17.098 4L17.8 4A3.2 3.2 0 0 1 21 7.2L21 13.3A3.2 3.2 0 0 1 17.8 16.5L6.2 16.5A3.2 3.2 0 0 1 3 13.3L3 7.2A3.2 3.2 0 0 1 6.2 4L13.398 4\"/></g><g\u{1}><path d=\"M7 10.4h2l1.3-2.4 2.2 4.8 1.4-2.4H17\"/></g><g\u{1}><path d=\"M8.5 20.5h7\"/></g>",
};

/// The pulse is traced and runs out into a small, closed ring.
static SILENT: Pack = Pack {
    id: "silent",
    dur: 0.6333,
    parts: &[
        Part {
            name: "line",
            ox: 12.0,
            oy: 12.0,
            len: 19.8447,
            w: 2.0,
        },
        Part {
            name: "o",
            ox: 19.2,
            oy: 12.0,
            len: 14.4534,
            w: 2.0,
        },
    ],
    segs: &[
        Seg {
            part: 0,
            prop: Prop::PathLen,
            t0: 0.0,
            dur: 0.31,
            keys: &[1.0, 0.06, 1.0],
            times: &[0.0, 0.2258, 1.0],
            eases: &[3, 1],
        },
        Seg {
            part: 1,
            prop: Prop::Scale,
            t0: 0.2,
            dur: 0.42,
            keys: &[1.0, 1.3, 1.0],
            times: &[0.0, 0.2857, 1.0],
            eases: &[1, 4],
        },
    ],
    holes: &[
        Hole {
            part: 0,
            width: false,
        },
        Hole {
            part: 1,
            width: false,
        },
    ],
    svg: "<g\u{1}><path d=\"M3 12h3l1.5-3 2.2 6 1.6-3H15\"/></g><g\u{1}><path d=\"M16.9 12A2.3 2.3 0 1 1 21.5 12A2.3 2.3 0 1 1 16.9 12Z\"/></g>",
};

/// Every declaration, in generator order.
pub(crate) const ALL: [&Pack; 104] = [
    &PANEL_LEFT,
    &PANEL_LEFT_CLOSE,
    &PANEL_RIGHT,
    &COLUMNS_2,
    &LAYOUT_GRID,
    &LAYOUT_TEMPLATE,
    &GALLERY_VERTICAL,
    &GALLERY_HORIZONTAL,
    &MENU,
    &X,
    &MINUS,
    &PLUS,
    &SQUARE,
    &COPY,
    &MAXIMIZE,
    &MINIMIZE,
    &EYE_OFF,
    &EYE,
    &KEYBOARD,
    &SLIDERS_HORIZONTAL,
    &PIN,
    &SETTINGS,
    &BOOK_OPEN,
    &BOOK_OPEN_TEXT,
    &BOOK_A,
    &BOOK_SEARCH,
    &BOOK_PLUS,
    &BOOKMARK,
    &ZOOM_IN,
    &ZOOM_OUT,
    &SEARCH,
    &MOVE_HORIZONTAL,
    &SHRINK,
    &RECTANGLE_VERTICAL,
    &TYPE,
    &IMAGE,
    &TABLE,
    &CHART_COLUMN_INCREASING,
    &HIGHLIGHTER,
    &BASELINE,
    &UNDERLINE,
    &PENCIL,
    &STICKY_NOTE,
    &HASH,
    &CHEVRON_LEFT,
    &CHEVRON_RIGHT,
    &CHEVRON_UP,
    &CHEVRON_DOWN,
    &ARROW_DOWN,
    &ARROW_UP,
    &ARROW_DOWN_UP,
    &ARROW_RIGHT_LEFT,
    &ARROW_RIGHT_FROM_LINE,
    &UNDO_2,
    &REFRESH_CW,
    &REPEAT,
    &LINK,
    &LIST,
    &LIST_TREE,
    &LIST_CHECKS,
    &ELLIPSIS,
    &CHECK,
    &CIRCLE_CHECK,
    &CIRCLE_ALERT,
    &CLOCK,
    &BAN,
    &LIBRARY_BIG,
    &SQUARE_LIBRARY,
    &FOLDER,
    &FOLDER_OPEN,
    &FOLDER_PLUS,
    &FOLDER_SEARCH,
    &FOLDERS,
    &FILE_SEARCH,
    &HARD_DRIVE,
    &DOWNLOAD,
    &TRASH_2,
    &SERVER,
    &CPU,
    &KEY_ROUND,
    &AI_MARK,
    &LANGUAGES,
    &FEATHER,
    &SCROLL_TEXT,
    &PLAY,
    &PAUSE,
    &SKIP_BACK,
    &SKIP_FORWARD,
    &CIRCLE_STOP,
    &HEADPHONES,
    &AUDIO_WAVEFORM,
    &VOLUME_2,
    &SUN,
    &MOON,
    &SUN_MOON,
    &ECLIPSE,
    &PALETTE,
    &WIND,
    &KOKORO,
    &PIPER,
    &KITTEN,
    &PHONON,
    &SYSTEM,
    &SILENT,
];
