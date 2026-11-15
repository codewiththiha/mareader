//! Session driver: opens, executes commands, pumps background jobs, settles.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mareader_engine::command::Command;
use mareader_engine::event::Event;
use mareader_engine::host::Shared;
use mareader_engine::session::Session;
use serde_json::{Map, Value};

use crate::host::{CliHost, Gate};

const SETTLE_LIMIT: Duration = Duration::from_secs(30);
const WAIT_STEP: Duration = Duration::from_millis(10);

/// Running headless session paired with its host, waker gate, and event log.
pub struct Runner {
    pub session: Session,
    pub host: Arc<CliHost>,
    pub gate: Gate,
    pub events: Vec<Event>,
    pub now: f64,
}

impl Runner {
    /// Creates a session backed by `state_dir`, resuming any saved document.
    pub fn open(state_dir: Option<PathBuf>) -> Self {
        let host = Arc::new(CliHost::new(state_dir));
        let gate = Gate::new();
        let cloned = Arc::clone(&host);
        let shared: Shared = cloned;
        let mut session = Session::new(shared, gate.waker());
        session.set_progressive_open(false);
        let mut runner = Self {
            session,
            host,
            gate,
            events: Vec::new(),
            now: 0.0,
        };
        runner.settle_jobs();
        if let Some(req) = runner.session.resume_request() {
            runner.session.execute(Command::Open(req), runner.now);
            runner.settle_jobs();
        }
        runner
    }

    /// Runs one registered command id with JSON params and waits for jobs.
    pub fn exec(&mut self, id: &str, params: &str) -> Result<(), String> {
        self.session.execute_id(id, params, self.now)?;
        self.settle_jobs();
        Ok(())
    }

    /// Replays a JSONL command script (`-` reads stdin) step by step.
    pub fn run_script(&mut self, path: &Path) -> Result<usize, String> {
        let text = read_script(path)?;
        let mut steps = 0usize;
        for (idx, raw) in text.lines().enumerate() {
            let n = idx + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            let mut value: Value = serde_json::from_str(line).map_err(|e| at(n, e))?;
            expand_value(&mut value);
            let Some(id) = value.get("id").and_then(Value::as_str) else {
                return Err(at(n, "missing string \"id\""));
            };
            let id = id.to_owned();
            let empty = Value::Object(Map::new());
            let params = value.get("params").unwrap_or(&empty);
            let json = serde_json::to_string(params).map_err(|e| at(n, e))?;
            self.exec(&id, &json).map_err(|e| at(n, e))?;
            steps += 1;
        }
        Ok(steps)
    }

    /// Flushes settings, session resume state, and library changes to disk.
    pub fn persist(&mut self) {
        let last = self.session.last_document();
        self.session.settle(last);
        let drained = self.session.drain();
        self.events.extend(drained);
    }

    /// Pumps documents, ticks services, and waits until background jobs finish.
    pub fn settle_jobs(&mut self) {
        let deadline = Instant::now() + SETTLE_LIMIT;
        loop {
            let seq = self.gate.seq();
            self.session.pump();
            let tick = self.session.tick(self.now);
            self.now += 0.016;
            let got = self.session.drain();
            let had_events = !got.is_empty();
            self.events.extend(got);
            if !self.session.busy() && !tick.repaint && !had_events {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            if self.session.busy() {
                self.gate.wait_change(seq, WAIT_STEP);
            }
        }
    }
}

fn at(line: usize, why: impl std::fmt::Display) -> String {
    format!("line {line}: {why}")
}

fn read_script(path: &Path) -> Result<String, String> {
    if path == Path::new("-") {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("stdin: {e}"))?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn expand_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = expand_env(text),
        Value::Array(items) => items.iter_mut().for_each(expand_value),
        Value::Object(map) => map.values_mut().for_each(expand_value),
        _ => {}
    }
}

/// Replaces `${VAR}` references with environment variables when set.
fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some((before, after)) = rest.split_once("${") {
        out.push_str(before);
        match after.split_once('}') {
            Some((name, tail)) => {
                match std::env::var(name) {
                    Ok(val) => out.push_str(&val),
                    Err(_) => {
                        out.push_str("${");
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = tail;
            }
            None => {
                out.push_str("${");
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}
