//! The frame wire protocol: the serialized form of the Shell ⇄ runtime
//! boundary once a runtime lives in its own frame (guide §7–§9, §35).
//!
//! Every type here is pure data — no `js-sys`, no `web-sys` — so both
//! artifacts and the Shell serialize it from the one crate, and the shape is
//! unit-testable off-wasm.
//!
//! Channel discipline (§8): the Shell hands each frame a dedicated
//! `MessagePort`, so the port itself authenticates the channel; the
//! `generation` on every envelope is the stale-frame guard — a message from a
//! disposed or superseded generation is dropped by whoever receives it,
//! never applied to a live session (§35). The `nonce` authenticates the init
//! handshake itself: a runtime acknowledges its init only when the nonce it
//! echoes is the one the frame was created with.

use serde::{Deserialize, Serialize};

use crate::boundary::{DocStatusReport, LaunchDocument, ReadPoint};
use crate::covers::CoverImage;

/// Which runtime occupies a frame. The Shell keeps at most one ACTIVE frame
/// and at most one WARM frame (§33's "live runtime frames" invariant, restated
/// for the warm-slot lifecycle: one runtime is on screen, one is booted and
/// waiting behind it); the kind says which artifact the frame booted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeKind {
    Library,
    Reader,
}

/// The boot handshake stages (§9): a runtime does not get to claim "ready"
/// because its wasm module initialized — ready means the session exists, the
/// DOM is mounted and the first paint was given a chance. The Shell keeps
/// the loading cover until [`RuntimeFrame::Painted`] arrives, and a stage of
/// [`BootStage::Failed`] carries its cause into the Shell's visible runtime
/// error state (§11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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

/// Every Shell → runtime message, wrapped in the frame identity: the runtime
/// accepts a message only for ITS generation, and the init nonce authenticates
/// the channel establishment itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellEnvelope {
    pub generation: u64,
    pub nonce: String,
    #[serde(flatten)]
    pub body: ShellFrame,
}

/// What the Shell can tell a runtime. These are commands and answers — the
/// Shell asks, the runtime executes; durable state never travels Shell-owned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ShellFrame {
    /// First message on the port after the frame's document loads: the
    /// identity the runtime must echo, plus the reader's launch descriptor
    /// when the frame was created to open a document.
    Init {
        runtime: RuntimeKind,
        launch: Option<Box<LaunchDocument>>,
        /// The frame was booted ahead of the navigation that will use it.
        ///
        /// A warm runtime mounts and paints but does no WORK: the library
        /// holds off its startup passes — store migration, watched-folder
        /// rescan, backfill, cover bakes — because they write durable state
        /// a live reader is meanwhile editing, and the reader opens nothing
        /// until it is handed a launch. Both resume on [`ShellFrame::Refresh`]
        /// / [`ShellFrame::Launch`]. Warming is meant to buy a boot, not to
        /// run two runtimes' workloads at once.
        warm: bool,
    },
    /// A document opened while this reader frame is already live (an
    /// in-session drop/dialog, or the promotion of a warm reader): the
    /// reader runs its open pipeline with this descriptor.
    Launch { document: Box<LaunchDocument> },
    /// The runtime has been promoted from warm to visible. A warm runtime
    /// boots BEFORE it is needed, so durable state another runtime wrote
    /// while it waited is not in the signals it seeded at boot — this is its
    /// cue to re-read that state. Deliberately cheap: a store read, never a
    /// reboot (a refresh that cost as much as a boot would defeat warming).
    Refresh,
    /// §12 phase 1: flush, cancel, dispose, then answer
    /// [`RuntimeFrame::DisposeComplete`]. The Shell removes the iframe only
    /// after that answer (or after the forced-dispose timeout).
    Dispose,
    /// Answer to [`RuntimeFrame::BakeCover`]: the shelf bake, performed by
    /// the Shell's own bake frame (a pdf.js-only page the Shell mounts while
    /// the queue drains and removes after it) — `image: None` when the bake
    /// failed.
    CoverBaked {
        path: String,
        image: Option<CoverImage>,
    },
    /// Recycle a disposed Library frame: mount a fresh WARM session in the
    /// same document, only after [`RuntimeFrame::DisposeComplete`]. Reader
    /// hosts and document realms are never rearmed; their frames are removed.
    Rearm,
    /// Answer to [`RuntimeFrame::ResolveLaunch`], matched by `request`.
    ResolveLaunchAnswer {
        request: u64,
        document: Option<Box<LaunchDocument>>,
    },
    /// Files dropped on the window from the OS while the LIBRARY is the
    /// runtime on screen: the shelf imports them where it is looking. The
    /// Shell's one drop listener sends it to the library only — a drop over
    /// the reader is nothing — and the paths are already filtered to the
    /// formats the app opens.
    ImportFiles { paths: Vec<String> },
}

/// Every runtime → Shell message, wrapped in the frame generation. The Shell
/// drops any envelope whose generation is not the live frame's (§8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEnvelope {
    pub generation: u64,
    #[serde(flatten)]
    pub body: RuntimeFrame,
}

/// What a runtime can tell the Shell: the lifecycle signals of the boot
/// handshake, the timer-free [`crate::boundary::ShellApi`] vocabulary
/// serialized, and the disposal acknowledgement. The fat payloads are boxed
/// (transparent on the wire — same JSON) so the envelope's in-memory size
/// stays the size of its smallest variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuntimeFrame {
    /// Session created and durable state loaded ( boot stages up to
    /// [`BootStage::Ready`] ). The Shell does not treat this as paint.
    Ready,
    /// The runtime's root DOM exists and has had its paint opportunity
    /// (§9): the Shell may now lift the loading cover.
    Painted,
    /// A boot-stage transition, so the Shell's error surface can say which
    /// stage a failure happened in (§11).
    Status { stage: BootStage },
    /// The runtime found its own failure: surface it as a runtime error,
    /// never a blank window and never a silent fallback (§11).
    Failed { stage: BootStage, cause: String },
    /// §12 phase 1 done: durable state flushed, document/render/search/
    /// prefetch/virtualizer work cancelled, timers and observers released.
    /// The Shell may now remove the iframe.
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
    /// `ShellApi::reload` over the wire.
    Reload,
    /// The one query the bridge answered synchronously becomes a
    /// request/answer pair over the port; `request` matches the answer.
    ResolveLaunch { request: u64, path: String },
}

/// The Shell's runtime error record (§11): visible in the error state,
/// complete enough to say WHICH runtime, WHERE in the boot it died, WHICH
/// generation it belonged to and WHY. The Shell never panics over a runtime
/// that failed — the frame model exists so a runtime can fail without the
/// host following it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
                warm: false,
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        assert_eq!(
            json,
            r#"{"generation":17,"nonce":"f7a2","kind":"init","runtime":"reader","launch":null,"warm":false}"#
        );
        // The same boot claimed as warm: the shelf reads that flag before
        // its first effect runs, so it rides the init and nowhere else.
        let warm = ShellEnvelope {
            generation: 18,
            nonce: "f7a2".to_string(),
            body: ShellFrame::Init {
                runtime: RuntimeKind::Library,
                launch: None,
                warm: true,
            },
        };
        assert_eq!(
            serde_json::to_string(&warm).unwrap(),
            r#"{"generation":18,"nonce":"f7a2","kind":"init","runtime":"library","launch":null,"warm":true}"#
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
    fn the_recycle_and_cover_routing_frames_wire_by_kind() {
        let rearm = ShellEnvelope {
            generation: 4,
            nonce: "n".to_string(),
            body: ShellFrame::Rearm,
        };
        assert_eq!(
            serde_json::to_string(&rearm).unwrap(),
            r#"{"generation":4,"nonce":"n","kind":"rearm"}"#
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
        // The frames replace the TRANSPORT, not the vocabulary: the same
        // names the same-page bridge used must be the names on the port wire,
        // so a runtime's ShellApi call is byte-identical whichever transport
        // carries it.
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
