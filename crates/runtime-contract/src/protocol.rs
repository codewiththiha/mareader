/
/
!

T
h
e

f
r
a
m
e

w
i
r
e

p
r
o
t
o
c
o
l
:

t
h
e

S
h
e
l
l

t
o

r
u
n
t
i
m
e

b
o
u
n
d
a
r
y
,

s
e
r
i
a
l
i
z
e
d
.

use serde::{Deserialize, Serialize};

use crate::boundary::{DocStatusReport, LaunchDocument, ReadPoint};
use crate::covers::CoverImage;

/
/
/

W
h
i
c
h

a
r
t
i
f
a
c
t

a

r
o
u
t
e

f
r
a
m
e

b
o
o
t
e
d
.
pub enum RuntimeKind {
    Library,
    Reader,
}

/
/
/

T
h
e

b
o
o
t

h
a
n
d
s
h
a
k
e

s
t
a
g
e
s
.
pub enum BootStage {
    /// Document fetched, module loading.
    Loading,
    /// Wasm initialized, session not yet created.
    Initialized,
    /// Runtime root mounted in the frame's DOM.
    Mounted,
    /// Session created; durable state loaded.
    Ready,
    /// Graceful disposal in progress (§12 phase 1).
    Disposing,
    /// Disposal acknowledged or the frame removed by the forced path.
    Disposed,
    /// Any bounded stage timed out or the runtime reported a failure.
    Failed,
}

/
/
/

E
v
e
r
y

S
h
e
l
l

t
o

r
u
n
t
i
m
e

m
e
s
s
a
g
e
,

w
r
a
p
p
e
d

i
n

t
h
e

f
r
a
m
e

i
d
e
n
t
i
t
y
.
pub struct ShellEnvelope {
    pub generation: u64,
    pub nonce: String,
    #[serde(flatten)]
    pub body: ShellFrame,
}

/
/
/

W
h
a
t

t
h
e

S
h
e
l
l

c
a
n

t
e
l
l

a

r
u
n
t
i
m
e
.
pub enum ShellFrame {
    /
    /
    /

    F
    i
    r
    s
    t

    m
    e
    s
    s
    a
    g
    e

    o
    n

    t
    h
    e

    p
    o
    r
    t

    a
    f
    t
    e
    r

    t
    h
    e

    d
    o
    c
    u
    m
    e
    n
    t

    l
    o
    a
    d
    s
    .
    Init {
        runtime: RuntimeKind,
        launch: Option<Box<LaunchDocument>>,
        /
        /
        /

        T
        h
        e

        i
        n
        c
        o
        m
        i
        n
        g

        f
        r
        a
        m
        e

        i
        s

        l
        a
        i
        d

        o
        u
        t

        b
        u
        t

        n
        o
        t

        v
        i
        s
        i
        b
        l
        e

        y
        e
        t
        .
        hidden: bool,
    },
    /
    /
    /

    A

    d
    o
    c
    u
    m
    e
    n
    t

    o
    p
    e
    n
    e
    d

    i
    n
    s
    i
    d
    e

    a
    n

    a
    c
    t
    i
    v
    e

    R
    e
    a
    d
    e
    r

    w
    o
    r
    k
    s
    p
    a
    c
    e
    .
    Launch { document: Box<LaunchDocument> },
    /
    /
    /

    T
    h
    e

    f
    r
    a
    m
    e

    i
    s

    n
    o
    w

    v
    i
    s
    i
    b
    l
    e
    .
    Refresh,
    /
    /
    /

    P
    h
    a
    s
    e

    1
    :

    f
    l
    u
    s
    h
    ,

    c
    a
    n
    c
    e
    l
    ,

    d
    i
    s
    p
    o
    s
    e
    ,

    t
    h
    e
    n

    a
    n
    s
    w
    e
    r
    .
    Dispose,
    /
    /
    /

    A
    n
    s
    w
    e
    r

    t
    o

    [
    `
    R
    u
    n
    t
    i
    m
    e
    F
    r
    a
    m
    e
    :
    :
    B
    a
    k
    e
    C
    o
    v
    e
    r
    `
    ]
    :

    t
    h
    e

    s
    h
    e
    l
    f

    b
    a
    k
    e
    .
    CoverBaked {
        path: String,
        image: Option<CoverImage>,
    },
    /// Answer to [`RuntimeFrame::ResolveLaunch`], matched by `request`.
    ResolveLaunchAnswer {
        request: u64,
        document: Option<Box<LaunchDocument>>,
    },
    /
    /
    /

    F
    i
    l
    e
    s

    d
    r
    o
    p
    p
    e
    d

    o
    n

    t
    h
    e

    w
    i
    n
    d
    o
    w

    w
    h
    i
    l
    e

    t
    h
    e

    L
    I
    B
    R
    A
    R
    Y

    i
    s

    o
    n

    s
    c
    r
    e
    e
    n
    .
    ImportFiles { paths: Vec<String> },
}

/
/
/

E
v
e
r
y

r
u
n
t
i
m
e

t
o

S
h
e
l
l

m
e
s
s
a
g
e
,

w
r
a
p
p
e
d

i
n

t
h
e

f
r
a
m
e

g
e
n
e
r
a
t
i
o
n
.
pub struct RuntimeEnvelope {
    pub generation: u64,
    #[serde(flatten)]
    pub body: RuntimeFrame,
}

/
/
/

W
h
a
t

a

r
u
n
t
i
m
e

c
a
n

t
e
l
l

t
h
e

S
h
e
l
l
.
pub enum RuntimeFrame {
    /
    /
    /

    S
    e
    s
    s
    i
    o
    n

    c
    r
    e
    a
    t
    e
    d

    a
    n
    d

    d
    u
    r
    a
    b
    l
    e

    s
    t
    a
    t
    e

    l
    o
    a
    d
    e
    d
    .
    Ready,
    /
    /
    /

    T
    h
    e

    r
    u
    n
    t
    i
    m
    e
    '
    s

    r
    o
    o
    t

    D
    O
    M

    e
    x
    i
    s
    t
    s

    a
    n
    d

    h
    a
    d

    i
    t
    s

    p
    a
    i
    n
    t

    o
    p
    p
    o
    r
    t
    u
    n
    i
    t
    y
    .
    Painted,
    /
    /
    /

    A

    b
    o
    o
    t
    -
    s
    t
    a
    g
    e

    t
    r
    a
    n
    s
    i
    t
    i
    o
    n
    .
    Status { stage: BootStage },
    /
    /
    /

    T
    h
    e

    r
    u
    n
    t
    i
    m
    e

    f
    o
    u
    n
    d

    i
    t
    s

    o
    w
    n

    f
    a
    i
    l
    u
    r
    e
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
    .
    Failed { stage: BootStage, cause: String },
    /
    /
    /

    P
    h
    a
    s
    e

    1

    d
    o
    n
    e
    :

    s
    t
    a
    t
    e

    f
    l
    u
    s
    h
    e
    d
    ,

    w
    o
    r
    k

    c
    a
    n
    c
    e
    l
    l
    e
    d
    ,

    r
    e
    s
    o
    u
    r
    c
    e
    s

    r
    e
    l
    e
    a
    s
    e
    d
    .
    DisposeComplete,
    /// `ShellApi::open_document` over the wire.
    OpenDocument { launch: Box<LaunchDocument> },
    /// `ShellApi::navigate_library` over the wire.
    NavigateLibrary,
    /// `ShellApi::read_point` over the wire.
    ReadPoint { point: Box<ReadPoint> },
    /// `ShellApi::save_settings` over the wire.
    SaveSettings {
        settings: Box<reader_core::settings::Settings>,
    },
    /// `ShellApi::save_cover` over the wire.
    SaveCover { path: String, image: CoverImage },
    /// `ShellApi::save_gloss` over the wire.
    SaveGloss { key: String, marks: String },
    /// `ShellApi::bake_cover` over the wire — answered by
    /// [`ShellFrame::CoverBaked`].
    BakeCover { path: String },
    /// `ShellApi::doc_status` over the wire.
    DocStatus { report: DocStatusReport },
    /// `ShellApi::publish_digest` over the wire.
    PublishDigest { json: String },
    /
    /
    /

    T
    h
    e

    o
    n
    e

    s
    y
    n
    c
    h
    r
    o
    n
    o
    u
    s

    q
    u
    e
    r
    y

    b
    e
    c
    o
    m
    e
    s

    a

    r
    e
    q
    u
    e
    s
    t

    a
    n
    d

    a
    n
    s
    w
    e
    r

    p
    a
    i
    r
    .
    ResolveLaunch { request: u64, path: String },
}

/
/
/

T
h
e

S
h
e
l
l
'
s

r
u
n
t
i
m
e

e
r
r
o
r

r
e
c
o
r
d
.
pub struct BootError {
    pub runtime: RuntimeKind,
    pub generation: u64,
    pub stage: BootStage,
    pub cause: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_init_envelope_carries_identity_next_to_the_payload() {
        let env = ShellEnvelope {
            generation: 17,
            nonce: "f7a2".to_string(),
            body: ShellFrame::Init {
                runtime: RuntimeKind::Reader,
                launch: None,
                hidden: false,
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        assert_eq!(
            json,
            r#"{"generation":17,"nonce":"f7a2","kind":"init","runtime":"reader","launch":null,"hidden":false}"#
        );
        // Hidden describes an incoming frame, not a retained runtime.
        let incoming = ShellEnvelope {
            generation: 18,
            nonce: "f7a2".to_string(),
            body: ShellFrame::Init {
                runtime: RuntimeKind::Library,
                launch: None,
                hidden: true,
            },
        };
        assert_eq!(
            serde_json::to_string(&incoming).unwrap(),
            r#"{"generation":18,"nonce":"f7a2","kind":"init","runtime":"library","launch":null,"hidden":true}"#
        );
        let back: ShellEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn a_runtime_envelope_round_trips_with_its_generation() {
        let env = RuntimeEnvelope {
            generation: 11,
            body: RuntimeFrame::BakeCover {
                path: "/books/a.pdf".to_string(),
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        assert_eq!(
            json,
            r#"{"generation":11,"kind":"bakeCover","path":"/books/a.pdf"}"#
        );
        let back: RuntimeEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn a_gloss_save_wires_its_key_and_the_encoded_list() {
        let env = RuntimeEnvelope {
            generation: 7,
            body: RuntimeFrame::SaveGloss {
                key: "b-12".to_string(),
                marks: "[]".to_string(),
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        assert_eq!(
            json,
            r#"{"generation":7,"kind":"saveGloss","key":"b-12","marks":"[]"}"#
        );
        assert_eq!(serde_json::from_str::<RuntimeEnvelope>(&json).unwrap(), env);
    }

    #[test]
    fn retired_realms_cannot_be_rearmed_and_cover_answers_round_trip() {
        assert!(
            serde_json::from_str::<ShellEnvelope>(r#"{"generation":4,"nonce":"n","kind":"rearm"}"#)
                .is_err()
        );
        let baked = ShellEnvelope {
            generation: 4,
            nonce: "n".to_string(),
            body: ShellFrame::CoverBaked {
                path: "/b.pdf".to_string(),
                image: None,
            },
        };
        let json = serde_json::to_string(&baked).unwrap();
        assert_eq!(
            json,
            r#"{"generation":4,"nonce":"n","kind":"coverBaked","path":"/b.pdf","image":null}"#
        );
        assert_eq!(serde_json::from_str::<ShellEnvelope>(&json).unwrap(), baked);
    }

    #[test]
    fn boot_stages_wire_as_the_guides_vocabulary() {
        let cases = [
            (BootStage::Loading, "loading"),
            (BootStage::Initialized, "initialized"),
            (BootStage::Mounted, "mounted"),
            (BootStage::Ready, "ready"),
            (BootStage::Disposing, "disposing"),
            (BootStage::Disposed, "disposed"),
            (BootStage::Failed, "failed"),
        ];
        for (stage, wire) in cases {
            assert_eq!(
                serde_json::to_string(&stage).unwrap(),
                format!("\"{wire}\"")
            );
        }
    }

    #[test]
    fn the_shell_vocabulary_keeps_the_bridge_wire_names() {
        /
        /

        F
        r
        a
        m
        e
        s

        r
        e
        p
        l
        a
        c
        e

        t
        h
        e

        T
        R
        A
        N
        S
        P
        O
        R
        T
        ,

        n
        o
        t

        t
        h
        e

        v
        o
        c
        a
        b
        u
        l
        a
        r
        y
        .
        let env = RuntimeEnvelope {
            generation: 1,
            body: RuntimeFrame::NavigateLibrary,
        };
        assert_eq!(
            serde_json::to_string(&env).unwrap(),
            r#"{"generation":1,"kind":"navigateLibrary"}"#
        );
    }

    #[test]
    fn the_error_record_carries_runtime_stage_generation_cause() {
        let err = BootError {
            runtime: RuntimeKind::Reader,
            generation: 18,
            stage: BootStage::Initialized,
            cause: "wasm failed to initialize".to_string(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(
            json,
            r#"{"runtime":"reader","generation":18,"stage":"initialized","cause":"wasm failed to initialize"}"#
        );
    }
}
