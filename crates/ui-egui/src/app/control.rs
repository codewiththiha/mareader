//! Live UI control channel handler wired into [`MareaderApp`].

use std::sync::Arc;

use egui::{ColorImage, Context, Event, UserData, ViewportCommand};
use mareader_engine::command_spec::SPECS;
use mareader_engine::inspect;
use serde_json::{Value, json};

use crate::app::MareaderApp;
use crate::control::{
    CapturePhase, ControlReply, ControlRequest, ControlSnap, PendingScreenshot, encode_base64,
    encode_png, encode_png_rgba, translate_actions,
};
use crate::ui_command::{Sent, UI_COMMANDS, parse_ui_command};

impl MareaderApp {
    /// Processes incoming control requests and resolves completed screenshots.
    pub(super) fn poll_control(&mut self, ctx: &Context) {
        if self.ui.control.is_none() {
            return;
        }
        self.finish_control_screenshot(ctx);
        self.step_oneshot_capture(ctx);
        let envelopes = match self.ui.control.as_ref() {
            Some(host) => host.drain_requests(),
            None => return,
        };
        if envelopes.is_empty() {
            return;
        }
        let mut repaint = false;
        for env in envelopes {
            match env.request {
                ControlRequest::Screenshot { path } => {
                    self.start_control_screenshot(ctx, path, env.reply);
                    repaint = true;
                }
                other => {
                    let (reply, dirty) = self.handle_control_request(ctx, other);
                    repaint |= dirty;
                    let _ = env.reply.send(reply);
                }
            }
        }
        if repaint
            || self
                .ui
                .control
                .as_ref()
                .is_some_and(|h| h.has_pending_input())
        {
            ctx.request_repaint();
        }
    }

    fn handle_control_request(
        &mut self,
        ctx: &Context,
        req: ControlRequest,
    ) -> (ControlReply, bool) {
        let now = ctx.input(|i| i.time);
        match req {
            ControlRequest::Ping => (ControlReply::ok(json!({ "pong": true })), false),
            ControlRequest::State => (ControlReply::ok(self.control_state_json(ctx)), false),
            ControlRequest::Commands => (ControlReply::ok(self.control_commands_json()), false),
            ControlRequest::Exec { id, params } => self.control_exec(ctx, &id, params, now),
            ControlRequest::Input { actions } => self.control_input(&actions),
            ControlRequest::Journal { clear } => {
                let entries = self.session.journal().entries();
                if clear {
                    self.session.clear_journal();
                }
                (ControlReply::ok(json!({ "entries": entries })), false)
            }
            ControlRequest::Replay { steps } => match self.session.replay(&steps, now) {
                Ok(replayed) => {
                    self.drain(ctx);
                    (ControlReply::ok(json!({ "replayed": replayed })), true)
                }
                Err(why) => (ControlReply::err(why), false),
            },
            ControlRequest::Query { kind, params } => (self.control_query(&kind, &params), false),
            ControlRequest::Screenshot { .. } => {
                (ControlReply::err("screenshot routed separately"), false)
            }
        }
    }

    fn control_query(&self, kind: &str, params: &Value) -> ControlReply {
        let pane = params.get("pane").and_then(Value::as_u64);
        let outcome = match kind {
            "read_pages" => {
                let start = opt_usize(params, "start_page").or_else(|| opt_usize(params, "page"));
                let end = opt_usize(params, "end_page").or(start);
                inspect::read_pages(&self.session, pane, start, end)
            }
            "outline" => inspect::outline(&self.session, pane),
            "marks" => inspect::marks(&self.session, pane),
            "export_notes" => {
                let fmt = params.get("format").and_then(Value::as_str);
                inspect::export_notes(&self.session, pane, fmt)
            }
            "search" => {
                let max = opt_usize(params, "max_hits").unwrap_or(100);
                inspect::search_hits(&self.session, pane, max)
            }
            "library" => {
                let q = params.get("query").and_then(Value::as_str);
                let shelf = params.get("shelf").and_then(Value::as_str);
                Ok(inspect::library(&self.session, q, shelf))
            }
            "render_page" => self.control_render_page(pane, params),
            other => Err(format!("unknown query kind {other:?}")),
        };
        match outcome {
            Ok(val) => ControlReply::ok(val),
            Err(why) => ControlReply::err(why),
        }
    }

    fn control_render_page(&self, pane: Option<u64>, params: &Value) -> Result<Value, String> {
        let page = opt_usize(params, "page");
        let scale = params
            .get("scale")
            .and_then(Value::as_f64)
            .map_or(1.5, |s| s as f32);
        let path = params.get("path").and_then(Value::as_str);
        let rendered = inspect::render_page(&self.session, pane, page, scale)?;
        write_or_encode_page(rendered, path)
    }

    fn control_exec(
        &mut self,
        ctx: &Context,
        id: &str,
        params: Value,
        now: f64,
    ) -> (ControlReply, bool) {
        if id.starts_with("ui.") {
            return match parse_ui_command(id) {
                Ok(cmd) => {
                    self.apply(ctx, Sent::Ui(cmd));
                    (ControlReply::ok(json!({ "id": id, "kind": "ui" })), true)
                }
                Err(why) => (ControlReply::err(why), false),
            };
        }
        self.session.set_area(Self::area(ctx));
        match self.session.execute_json(id, params, now) {
            Ok(done) => {
                self.drain(ctx);
                let payload = json!({ "id": id, "kind": "engine", "dirty": done.dirty });
                (ControlReply::ok(payload), true)
            }
            Err(why) => (ControlReply::err(why), false),
        }
    }

    fn control_input(&mut self, actions: &[crate::control::InputAction]) -> (ControlReply, bool) {
        let events = match translate_actions(actions) {
            Ok(events) => events,
            Err(why) => return (ControlReply::err(why), false),
        };
        let Some(host) = self.ui.control.as_mut() else {
            return (ControlReply::err("control host not attached"), false);
        };
        match host.enqueue_events(events) {
            Ok(queued) => (ControlReply::ok(json!({ "queued": queued })), true),
            Err(why) => (ControlReply::err(why), false),
        }
    }

    fn start_control_screenshot(
        &mut self,
        ctx: &Context,
        path: Option<String>,
        reply: std::sync::mpsc::SyncSender<ControlReply>,
    ) {
        let Some(host) = self.ui.control.as_mut() else {
            let _ = reply.send(ControlReply::err("control host not attached"));
            return;
        };
        if host.pending_screenshot.is_some() {
            let _ = reply.send(ControlReply::err("another screenshot is already in flight"));
            return;
        }
        host.pending_screenshot = Some(PendingScreenshot { path, reply });
        ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::new(ControlSnap)));
        ctx.request_repaint();
    }

    fn finish_control_screenshot(&mut self, ctx: &Context) {
        let image: Option<Arc<ColorImage>> = ctx.input(|i| {
            i.events.iter().find_map(|event| match event {
                Event::Screenshot {
                    user_data, image, ..
                } if user_data
                    .data
                    .as_ref()
                    .and_then(|d| d.downcast_ref::<ControlSnap>())
                    .is_some() =>
                {
                    Some(Arc::clone(image))
                }
                _ => None,
            })
        });
        let Some(image) = image else {
            return;
        };
        if let Some(pending) = self
            .ui
            .control
            .as_mut()
            .and_then(|h| h.pending_screenshot.take())
        {
            let reply = build_screenshot_reply(&image, pending.path.as_deref());
            let _ = pending.reply.send(reply);
            return;
        }
        let oneshot_path = self.ui.control.as_mut().and_then(|h| {
            let cap = h.oneshot.as_mut()?;
            if cap.phase == CapturePhase::Capturing {
                cap.phase = CapturePhase::Done;
                Some(cap.path.clone())
            } else {
                None
            }
        });
        if let Some(target) = oneshot_path {
            let reply = build_screenshot_reply(&image, Some(&target));
            if let Some(err) = reply.error {
                eprintln!("mareader --screenshot: {err}");
            }
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn step_oneshot_capture(&mut self, ctx: &Context) {
        let phase = match self.ui.control.as_ref().and_then(|h| h.oneshot.as_ref()) {
            Some(cap) => cap.phase,
            None => return,
        };
        let opening = self.session.documents().opening();
        match phase {
            CapturePhase::WaitOpen => {
                if !opening && let Some(cap) = self.oneshot_mut() {
                    cap.phase = CapturePhase::RunSteps;
                }
                ctx.request_repaint();
            }
            CapturePhase::RunSteps => {
                let steps = match self.oneshot_mut() {
                    Some(cap) => {
                        cap.phase = CapturePhase::WaitFrames(cap.wait_frames);
                        std::mem::take(&mut cap.steps)
                    }
                    None => return,
                };
                let now = ctx.input(|i| i.time);
                for step in steps {
                    let _ = self.control_exec(ctx, &step.id, step.params, now);
                }
                ctx.request_repaint();
            }
            CapturePhase::WaitFrames(left) => {
                if !opening && let Some(cap) = self.oneshot_mut() {
                    if left == 0 {
                        cap.phase = CapturePhase::Capturing;
                        ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::new(
                            ControlSnap,
                        )));
                    } else {
                        cap.phase = CapturePhase::WaitFrames(left - 1);
                    }
                }
                ctx.request_repaint();
            }
            CapturePhase::Capturing | CapturePhase::Done => {}
        }
    }

    fn oneshot_mut(&mut self) -> Option<&mut crate::control::OneShotCapture> {
        self.ui.control.as_mut().and_then(|h| h.oneshot.as_mut())
    }

    fn control_state_json(&self, ctx: &Context) -> Value {
        let mut state = inspect::session_state(&self.session);
        let toasts: Vec<&str> = self.ui.toasts.shown().map(|t| t.message.as_str()).collect();
        let rect = ctx.content_rect();
        if let Some(obj) = state.as_object_mut() {
            obj.insert("mode".to_owned(), json!("live"));
            obj.insert(
                "ui".to_owned(),
                json!({
                    "sidebar_open": self.ui.sidebar.is_open(),
                    "sidebar_panel": self.ui.sidebar.panel().label(),
                    "palette_open": crate::palette::is_open(ctx),
                    "shortcuts_open": crate::shortcuts::is_open(ctx),
                    "settings_open": self.ui.settings_window.is_open(),
                    "find_open": self.ui.find.is_open(),
                    "chromeless": self.ui.chromeless,
                    "toasts": toasts,
                }),
            );
            obj.insert(
                "window".to_owned(),
                json!({
                    "width": rect.width(),
                    "height": rect.height(),
                    "pixels_per_point": ctx.pixels_per_point(),
                }),
            );
        }
        state
    }

    fn control_commands_json(&self) -> Value {
        let engine: Vec<Value> = SPECS
            .iter()
            .map(|spec| {
                let run = (spec.can_run)(&self.session);
                json!({
                    "id": spec.id,
                    "title": spec.title,
                    "label": spec.label,
                    "menu": spec.menu,
                    "shortcut": spec.shortcut,
                    "journal": spec.journal,
                    "can_run": run.is_ok(),
                    "disabled_reason": run.err(),
                })
            })
            .collect();
        let ui: Vec<Value> = UI_COMMANDS
            .iter()
            .map(|meta| {
                json!({
                    "id": meta.id,
                    "title": meta.title,
                    "label": meta.label,
                    "menu": meta.menu,
                    "shortcut": meta.shortcut,
                })
            })
            .collect();
        json!({ "engine": engine, "ui": ui })
    }
}

fn build_screenshot_reply(image: &ColorImage, path: Option<&str>) -> ControlReply {
    let bytes = match encode_png(image) {
        Ok(bytes) => bytes,
        Err(why) => return ControlReply::err(why),
    };
    let width = image.size[0];
    let height = image.size[1];
    if let Some(target) = path {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Err(why) = std::fs::write(target, &bytes) {
                return ControlReply::err(format!("write {target}: {why}"));
            }
            return ControlReply::ok(json!({
                "width": width,
                "height": height,
                "bytes": bytes.len(),
                "path": target,
            }));
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = target;
            return ControlReply::err("saving screenshots to disk is unavailable on web");
        }
    }
    ControlReply::ok(json!({
        "width": width,
        "height": height,
        "bytes": bytes.len(),
        "png_base64": encode_base64(&bytes),
    }))
}

fn opt_usize(params: &Value, key: &str) -> Option<usize> {
    params
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
}

fn write_or_encode_page(
    rendered: inspect::RenderedPage,
    path: Option<&str>,
) -> Result<Value, String> {
    let inspect::RenderedPage {
        page,
        page_count,
        width,
        height,
        pixels,
    } = rendered;
    let bytes = encode_png_rgba(width, height, pixels)?;
    if let Some(target) = path {
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::write(target, &bytes).map_err(|e| format!("write {target}: {e}"))?;
            return Ok(json!({
                "page": page,
                "page_count": page_count,
                "width": width,
                "height": height,
                "bytes": bytes.len(),
                "path": target,
            }));
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = target;
            return Err("saving rendered pages to disk is unavailable on web".to_owned());
        }
    }
    Ok(json!({
        "page": page,
        "page_count": page_count,
        "width": width,
        "height": height,
        "bytes": bytes.len(),
        "png_base64": encode_base64(&bytes),
    }))
}
