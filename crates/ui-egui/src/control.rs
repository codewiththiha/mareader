//! Live UI control channel protocol, synthetic input queue, and PNG encoder.
//!
//! This module stays portable across native and web targets. Desktop binaries
//! wrap [`ControlBridge`] in a loopback TCP server, while [`ControlHost`] runs
//! on the UI thread inside [`crate::MareaderApp`].

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};

use egui::{ColorImage, Event, Key, Modifiers, PointerButton, Pos2, RawInput, TouchPhase, vec2};
use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Maximum synthetic input actions accepted in one control request.
pub const MAX_INPUT_ACTIONS: usize = 64;

/// Maximum queued raw egui events awaiting `raw_input_hook`.
pub const MAX_QUEUED_EVENTS: usize = 256;

/// Marker type stored in [`egui::UserData`] to isolate control screenshots.
#[derive(Clone, Copy, Debug)]
pub struct ControlSnap;

/// A control request sent from the loopback server to the UI thread.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ControlRequest {
    Ping,
    State,
    Commands,
    Exec {
        id: String,
        #[serde(default)]
        params: Value,
    },
    Input {
        actions: Vec<InputAction>,
    },
    Screenshot {
        #[serde(default)]
        path: Option<String>,
    },
    Journal {
        #[serde(default)]
        clear: bool,
    },
    Replay {
        steps: Vec<mareader_engine::journal::Step>,
    },
    Query {
        kind: String,
        #[serde(default)]
        params: Value,
    },
}

/// One synthetic pointer, wheel, key, or text action for live UI automation.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputAction {
    Move {
        x: f32,
        y: f32,
    },
    Click {
        x: f32,
        y: f32,
        #[serde(default)]
        button: Option<String>,
    },
    Press {
        x: f32,
        y: f32,
        #[serde(default)]
        button: Option<String>,
    },
    Release {
        x: f32,
        y: f32,
        #[serde(default)]
        button: Option<String>,
    },
    Scroll {
        #[serde(default)]
        dx: f32,
        dy: f32,
    },
    Key {
        key: String,
        #[serde(default)]
        modifiers: Vec<String>,
        #[serde(default)]
        pressed: Option<bool>,
    },
    Text {
        text: String,
    },
}

/// JSON reply sent back for a [`ControlRequest`].
#[derive(Clone, Debug, Serialize)]
pub struct ControlReply {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ControlReply {
    /// Successful reply carrying a JSON payload.
    pub fn ok(result: Value) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    /// Error reply describing why the request failed.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(message.into()),
        }
    }
}

/// Internal request envelope paired with a one-shot reply channel.
pub struct Envelope {
    pub request: ControlRequest,
    pub reply: SyncSender<ControlReply>,
}

/// Thread-safe sender half held by the transport server.
#[derive(Clone)]
pub struct ControlBridge {
    tx: Sender<Envelope>,
}

impl ControlBridge {
    /// Queues `request` for the UI thread and returns the reply receiver.
    pub fn send(&self, request: ControlRequest) -> Result<Receiver<ControlReply>, String> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.tx
            .send(Envelope { request, reply })
            .map_err(|_| "UI control host is closed".to_owned())?;
        Ok(rx)
    }
}

/// Pending screenshot capture awaiting an [`Event::Screenshot`] match.
pub struct PendingScreenshot {
    pub path: Option<String>,
    pub reply: SyncSender<ControlReply>,
}

/// Lifecycle phase for desktop CLI `--screenshot` one-shot capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePhase {
    WaitOpen,
    RunSteps,
    WaitFrames(u32),
    Capturing,
    Done,
}

/// Automated screenshot-and-exit job driven by `apps/mareader --screenshot`.
#[derive(Clone, Debug)]
pub struct OneShotCapture {
    pub path: String,
    pub steps: Vec<mareader_engine::journal::Step>,
    pub wait_frames: u32,
    pub phase: CapturePhase,
}

impl OneShotCapture {
    /// Creates a new one-shot desktop screenshot job.
    pub fn new(path: String, steps: Vec<mareader_engine::journal::Step>, wait_frames: u32) -> Self {
        Self {
            path,
            steps,
            wait_frames: wait_frames.max(2),
            phase: CapturePhase::WaitOpen,
        }
    }
}

/// UI-thread half attached to [`crate::MareaderApp`].
pub struct ControlHost {
    rx: Receiver<Envelope>,
    pending_events: VecDeque<Event>,
    pub(crate) pending_screenshot: Option<PendingScreenshot>,
    pub(crate) oneshot: Option<OneShotCapture>,
}

/// Creates a connected `(`[`ControlBridge`]`, `[`ControlHost`]`)` pair.
pub fn channel() -> (ControlBridge, ControlHost) {
    let (tx, rx) = mpsc::channel();
    (
        ControlBridge { tx },
        ControlHost {
            rx,
            pending_events: VecDeque::new(),
            pending_screenshot: None,
            oneshot: None,
        },
    )
}

impl ControlHost {
    /// Builds a standalone host running only a one-shot `--screenshot` capture.
    pub fn oneshot(capture: OneShotCapture) -> Self {
        let (_tx, rx) = mpsc::channel();
        Self {
            rx,
            pending_events: VecDeque::new(),
            pending_screenshot: None,
            oneshot: Some(capture),
        }
    }

    /// Attaches a one-shot `--screenshot` capture job to an existing host.
    pub fn set_oneshot(&mut self, capture: OneShotCapture) {
        self.oneshot = Some(capture);
    }
    /// Drains all queued control requests without blocking.
    pub fn drain_requests(&self) -> Vec<Envelope> {
        let mut out = Vec::new();
        while let Ok(env) = self.rx.try_recv() {
            out.push(env);
        }
        out
    }

    /// Appends synthetic input events up to [`MAX_QUEUED_EVENTS`].
    pub fn enqueue_events(&mut self, events: Vec<Event>) -> Result<usize, String> {
        let added = events.len();
        if self.pending_events.len().saturating_add(added) > MAX_QUEUED_EVENTS {
            return Err(format!("input queue exceeds {MAX_QUEUED_EVENTS} events"));
        }
        self.pending_events.extend(events);
        Ok(added)
    }

    /// Injects queued synthetic events into `raw_input` in `raw_input_hook`.
    pub fn inject_raw_input(&mut self, raw_input: &mut RawInput) {
        if self.pending_events.is_empty() {
            return;
        }
        raw_input.events.extend(self.pending_events.drain(..));
    }

    /// Whether any synthetic input events are queued for the next frame.
    pub fn has_pending_input(&self) -> bool {
        !self.pending_events.is_empty()
    }
}

/// Translates [`InputAction`] items into bounded egui [`Event`] values.
pub fn translate_actions(actions: &[InputAction]) -> Result<Vec<Event>, String> {
    if actions.len() > MAX_INPUT_ACTIONS {
        return Err(format!("too many actions (max {MAX_INPUT_ACTIONS})"));
    }
    let mut events = Vec::with_capacity(actions.len() * 2);
    for action in actions {
        match action {
            InputAction::Move { x, y } => events.push(Event::PointerMoved(Pos2::new(*x, *y))),
            InputAction::Click { x, y, button } => {
                let pos = Pos2::new(*x, *y);
                let button = parse_button(button.as_deref())?;
                events.push(Event::PointerMoved(pos));
                events.push(pointer_button(pos, button, true));
                events.push(pointer_button(pos, button, false));
            }
            InputAction::Press { x, y, button } => {
                let pos = Pos2::new(*x, *y);
                let button = parse_button(button.as_deref())?;
                events.push(Event::PointerMoved(pos));
                events.push(pointer_button(pos, button, true));
            }
            InputAction::Release { x, y, button } => {
                let pos = Pos2::new(*x, *y);
                let button = parse_button(button.as_deref())?;
                events.push(pointer_button(pos, button, false));
            }
            InputAction::Scroll { dx, dy } => events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(*dx, *dy),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            }),
            InputAction::Key {
                key,
                modifiers,
                pressed,
            } => {
                let Some(parsed) = Key::from_name(key) else {
                    return Err(format!("unknown key name {key:?}"));
                };
                let mods = parse_modifiers(modifiers)?;
                match pressed {
                    Some(down) => events.push(key_event(parsed, *down, mods)),
                    None => {
                        events.push(key_event(parsed, true, mods));
                        events.push(key_event(parsed, false, mods));
                    }
                }
            }
            InputAction::Text { text } => events.push(Event::Text(text.clone())),
        }
    }
    Ok(events)
}

fn pointer_button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn key_event(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    }
}

fn parse_button(name: Option<&str>) -> Result<PointerButton, String> {
    match name.unwrap_or("primary") {
        "primary" | "left" => Ok(PointerButton::Primary),
        "secondary" | "right" => Ok(PointerButton::Secondary),
        "middle" => Ok(PointerButton::Middle),
        other => Err(format!("unknown pointer button {other:?}")),
    }
}

fn parse_modifiers(list: &[String]) -> Result<Modifiers, String> {
    let mut mods = Modifiers::NONE;
    for item in list {
        match item.to_ascii_lowercase().as_str() {
            "command" | "cmd" | "mod" => mods |= Modifiers::COMMAND,
            "ctrl" | "control" => mods |= Modifiers::CTRL,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "option" => mods |= Modifiers::ALT,
            other => return Err(format!("unknown modifier {other:?}")),
        }
    }
    Ok(mods)
}

/// Encodes raw RGBA8 pixels (`width * height * 4` bytes) into PNG bytes.
pub fn encode_png_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Vec<u8>, String> {
    let Some(buf) = RgbaImage::from_raw(width, height, rgba) else {
        return Err("pixel buffer size mismatch".to_owned());
    };
    let mut out = Cursor::new(Vec::new());
    buf.write_to(&mut out, ImageFormat::Png)
        .map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(out.into_inner())
}

/// Encodes an egui [`ColorImage`] into PNG bytes.
pub fn encode_png(image: &ColorImage) -> Result<Vec<u8>, String> {
    let width = u32::try_from(image.size[0]).map_err(|_| "image width overflow".to_owned())?;
    let height = u32::try_from(image.size[1]).map_err(|_| "image height overflow".to_owned())?;
    let mut rgba = Vec::with_capacity(image.pixels.len().saturating_mul(4));
    for px in &image.pixels {
        rgba.extend_from_slice(&px.to_array());
    }
    encode_png_rgba(width, height, rgba)
}

pub use mareader_engine::inspect::encode_base64;

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Color32;

    #[test]
    fn base64_encodes_rfc4648_vectors() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn translate_actions_handles_click_key_and_bounds() {
        let actions = vec![
            InputAction::Click {
                x: 10.0,
                y: 20.0,
                button: None,
            },
            InputAction::Key {
                key: "K".to_owned(),
                modifiers: vec!["command".to_owned()],
                pressed: None,
            },
        ];
        let events = translate_actions(&actions);
        assert_eq!(events.as_ref().map(Vec::len), Ok(5));
        let bad = vec![InputAction::Key {
            key: "NoSuchKey".to_owned(),
            modifiers: Vec::new(),
            pressed: None,
        }];
        assert!(translate_actions(&bad).is_err());
    }

    #[test]
    fn encode_png_produces_valid_header() {
        let img = ColorImage::new([2, 2], vec![Color32::WHITE; 4]);
        let bytes = encode_png(&img);
        assert!(matches!(bytes, Ok(ref b) if b.starts_with(b"\x89PNG\r\n\x1a\n")));
    }
}
