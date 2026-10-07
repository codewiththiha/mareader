//! The frame wire protocol: the Shell to runtime boundary, serialized.
use serde::{Deserialize, Serialize};

use crate::boundary::{DocStatusReport, LaunchDocument, ReadPoint};
use crate::covers::CoverImage;

/// Which artifact a route frame booted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeKind {
    Library,
    Reader,
}

/// The boot handshake stages.
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

/// Every Shell to runtime message, wrapped in the frame identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellEnvelope {
    pub generation: u64,
    pub nonce: String,
    #[serde(flatten)]
    pub body: ShellFrame,
}

/// What the Shell can tell a runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ShellFrame {
    /// First message on the port after the document loads.
    Init {
        runtime: RuntimeKind,
        launch: Option<Box<LaunchDocument>>,
        /// The incoming frame is laid out but not visible yet.
        hidden: bool,
    },
    /// A document opened inside an active Reader workspace.
    Launch { document: Box<LaunchDocument> },
    /// The frame is now visible.
    Refresh,
    /// Phase 1: flush, cancel, dispose, then answer.
    Dispose,
    /// Answer to [`RuntimeFrame::BakeCover`]: the shelf bake.
    CoverBaked {
        path: String,
        image: Option<CoverImage>,
    },
    /// Answer to [`RuntimeFrame::ResolveLaunch`], matched by `request`.
    ResolveLaunchAnswer {
        request: u64,
        document: Option<Box<LaunchDocument>>,
    },
    /// Files dropped on the window while the LIBRARY is on screen.
    ImportFiles { paths: Vec<String> },
}

/// Every runtime to Shell message, wrapped in the frame generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEnvelope {
    pub generation: u64,
    #[serde(flatten)]
    pub body: RuntimeFrame,
}

/// What a runtime can tell the Shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuntimeFrame {
    /// Session created and durable state loaded.
    Ready,
    /// The runtime's root DOM exists and had its paint opportunity.
    Painted,
    /// A boot-stage transition.
    Status { stage: BootStage },
    /// The runtime found its own failure: surface it.
    Failed { stage: BootStage, cause: String },
    /// Phase 1 done: state flushed, work cancelled, resources released.
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
    /// The one synchronous query becomes a request and answer pair.
    ResolveLaunch { request: u64, path: String },
}

/// The Shell's runtime error record.
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
        // Frames replace the TRANSPORT, not the vocabulary.
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
