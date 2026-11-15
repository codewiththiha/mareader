//! Loopback TCP JSON-line control server for live UI automation (`--control`).

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mareader_ui_egui::control::{ControlBridge, ControlReply, ControlRequest};
use serde_json::{Value, json};

const MAX_LINE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const ACCEPT_POLL: Duration = Duration::from_millis(50);

/// Parsed `--control`, `--control-file`, and `--screenshot` CLI options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlArgs {
    pub port: Option<u16>,
    pub file: Option<PathBuf>,
    pub screenshot: Option<PathBuf>,
    pub script: Option<PathBuf>,
    pub wait_frames: u32,
}

impl Default for ControlArgs {
    fn default() -> Self {
        Self {
            port: None,
            file: None,
            screenshot: None,
            script: None,
            wait_frames: 10,
        }
    }
}

impl ControlArgs {
    /// Whether this launch runs isolated from any single-instance window.
    pub fn isolated(&self) -> bool {
        self.port.is_some() || self.screenshot.is_some()
    }
}

/// Splits automation CLI flags from launch file arguments.
pub fn parse_args<I>(args: I) -> Result<(ControlArgs, Vec<PathBuf>), String>
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    let mut cfg = ControlArgs::default();
    let mut files = Vec::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let Some(text) = arg.to_str() else {
            files.push(PathBuf::from(arg));
            continue;
        };
        if text == "--control" {
            cfg.port = Some(parse_u16_arg(&mut iter, "--control")?);
        } else if let Some(raw) = text.strip_prefix("--control=") {
            cfg.port = Some(
                raw.parse::<u16>()
                    .map_err(|_| format!("invalid --control port {raw:?}"))?,
            );
        } else if text == "--control-file" {
            cfg.file = Some(parse_path_arg(&mut iter, "--control-file")?);
        } else if let Some(raw) = text.strip_prefix("--control-file=") {
            cfg.file = Some(PathBuf::from(raw));
        } else if text == "--screenshot" {
            cfg.screenshot = Some(parse_path_arg(&mut iter, "--screenshot")?);
        } else if let Some(raw) = text.strip_prefix("--screenshot=") {
            cfg.screenshot = Some(PathBuf::from(raw));
        } else if text == "--script" {
            cfg.script = Some(parse_path_arg(&mut iter, "--script")?);
        } else if let Some(raw) = text.strip_prefix("--script=") {
            cfg.script = Some(PathBuf::from(raw));
        } else if text == "--wait-frames" {
            cfg.wait_frames = parse_u32_arg(&mut iter, "--wait-frames")?;
        } else if let Some(raw) = text.strip_prefix("--wait-frames=") {
            cfg.wait_frames = raw
                .parse::<u32>()
                .map_err(|_| format!("invalid --wait-frames {raw:?}"))?;
        } else {
            files.push(PathBuf::from(arg));
        }
    }
    Ok((cfg, files))
}

fn parse_path_arg(
    iter: &mut impl Iterator<Item = std::ffi::OsString>,
    flag: &str,
) -> Result<PathBuf, String> {
    iter.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{flag} requires a path"))
}

fn parse_u16_arg(
    iter: &mut impl Iterator<Item = std::ffi::OsString>,
    flag: &str,
) -> Result<u16, String> {
    let raw = iter
        .next()
        .ok_or_else(|| format!("{flag} requires a port number (0 for ephemeral)"))?;
    let raw = raw
        .to_str()
        .ok_or_else(|| format!("{flag} port must be UTF-8"))?;
    raw.parse::<u16>()
        .map_err(|_| format!("invalid {flag} port {raw:?}"))
}

fn parse_u32_arg(
    iter: &mut impl Iterator<Item = std::ffi::OsString>,
    flag: &str,
) -> Result<u32, String> {
    let raw = iter
        .next()
        .ok_or_else(|| format!("{flag} requires a non-negative integer"))?;
    let raw = raw
        .to_str()
        .ok_or_else(|| format!("{flag} value must be UTF-8"))?;
    raw.parse::<u32>()
        .map_err(|_| format!("invalid {flag} {raw:?}"))
}

/// Running loopback control server; stops its thread and cleans up on drop.
pub struct ControlServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    file: Option<PathBuf>,
}

impl ControlServer {
    /// Binds `127.0.0.1:port`, writes `--control-file`, and serves requests.
    pub fn start(
        port: u16,
        file: Option<PathBuf>,
        bridge: ControlBridge,
        ctx: egui::Context,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| format!("bind control socket on 127.0.0.1:{port}: {e}"))?;
        let addr = listener
            .local_addr()
            .map_err(|e| format!("control local_addr: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("control set_nonblocking: {e}"))?;

        let token = generate_token();
        if let Some(path) = &file {
            write_control_file(path, addr, &token)?;
        }
        eprintln!("mareader: control listening on 127.0.0.1:{}", addr.port());

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_token = token;
        thread::Builder::new()
            .name("mareader-control".to_owned())
            .spawn(move || serve_loop(listener, thread_stop, thread_token, bridge, ctx))
            .map_err(|e| format!("spawn control thread: {e}"))?;

        Ok(Self { addr, stop, file })
    }

    /// Loopback socket address bound by this server.
    #[allow(dead_code)]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(path) = &self.file {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn serve_loop(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    token: String,
    bridge: ControlBridge,
    ctx: egui::Context,
) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _peer)) => handle_client(stream, &token, &bridge, &ctx),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(_) => break,
        }
    }
}

fn handle_client(
    stream: TcpStream,
    expected_token: &str,
    bridge: &ControlBridge,
    ctx: &egui::Context,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(REQUEST_TIMEOUT));
    let _ = stream.set_write_timeout(Some(REQUEST_TIMEOUT));
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut raw = Vec::new();
    let reply = match read_bounded_line(&mut reader, &mut raw) {
        Ok(true) => dispatch_line(&raw, expected_token, bridge, ctx),
        Ok(false) => return,
        Err(why) => ControlReply::err(why),
    };
    if let Ok(mut encoded) = serde_json::to_vec(&reply) {
        encoded.push(b'\n');
        let _ = writer.write_all(&encoded);
        let _ = writer.flush();
    }
}

fn read_bounded_line(reader: &mut BufReader<TcpStream>, out: &mut Vec<u8>) -> Result<bool, String> {
    out.clear();
    loop {
        let filled = reader.fill_buf().map_err(|e| format!("read error: {e}"))?;
        if filled.is_empty() {
            return Ok(!out.is_empty());
        }
        let pos = filled.iter().position(|&b| b == b'\n');
        let take = pos.map_or(filled.len(), |idx| idx + 1);
        if out.len().saturating_add(take) > MAX_LINE_BYTES {
            return Err(format!("request exceeds {MAX_LINE_BYTES} bytes"));
        }
        let chunk = &filled[..take];
        out.extend_from_slice(chunk);
        reader.consume(take);
        if pos.is_some() {
            return Ok(true);
        }
    }
}

fn dispatch_line(
    raw: &[u8],
    expected_token: &str,
    bridge: &ControlBridge,
    ctx: &egui::Context,
) -> ControlReply {
    let line = match std::str::from_utf8(raw) {
        Ok(s) => s.trim(),
        Err(_) => return ControlReply::err("request must be UTF-8 JSON"),
    };
    let mut value: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return ControlReply::err(format!("invalid JSON: {e}")),
    };
    let Some(obj) = value.as_object_mut() else {
        return ControlReply::err("request must be a JSON object");
    };
    let supplied = obj
        .remove("token")
        .and_then(|v| v.as_str().map(str::to_owned));
    if supplied.as_deref() != Some(expected_token) {
        return ControlReply::err("unauthorized control token");
    }
    let request: ControlRequest = match serde_json::from_value(value) {
        Ok(req) => req,
        Err(e) => return ControlReply::err(format!("invalid control request: {e}")),
    };
    let rx = match bridge.send(request) {
        Ok(rx) => rx,
        Err(why) => return ControlReply::err(why),
    };
    ctx.request_repaint();
    match rx.recv_timeout(REQUEST_TIMEOUT) {
        Ok(reply) => reply,
        Err(_) => ControlReply::err("UI thread timed out handling request"),
    }
}

fn write_control_file(path: &Path, addr: SocketAddr, token: &str) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
    }
    let payload = json!({
        "host": "127.0.0.1",
        "port": addr.port(),
        "token": token,
        "pid": std::process::id(),
    });
    let text =
        serde_json::to_string_pretty(&payload).map_err(|e| format!("encode control file: {e}"))?;
    std::fs::write(path, format!("{text}\n"))
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }
    Ok(())
}

fn generate_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = u64::from(std::process::id());
    let mut a = RandomState::new().build_hasher();
    a.write_u128(nanos);
    a.write_u64(pid);
    let mut b = RandomState::new().build_hasher();
    b.write_u64(a.finish());
    b.write_u128(nanos.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    format!("{:016x}{:016x}", a.finish(), b.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn parse_args_extracts_control_flags_and_keeps_files() {
        let raw = [
            "--control",
            "0",
            "--control-file=/tmp/ctrl.json",
            "book.pdf",
        ]
        .into_iter()
        .map(OsString::from);
        let (cfg, files) = parse_args(raw).expect("valid args");
        assert_eq!(cfg.port, Some(0));
        assert_eq!(cfg.file, Some(PathBuf::from("/tmp/ctrl.json")));
        assert_eq!(files, vec![PathBuf::from("book.pdf")]);
    }

    #[test]
    fn parse_args_extracts_screenshot_script_and_wait_frames() {
        let raw = [
            "--screenshot",
            "/tmp/out.png",
            "--script=/tmp/steps.jsonl",
            "--wait-frames",
            "15",
            "book.epub",
        ]
        .into_iter()
        .map(OsString::from);
        let (cfg, files) = parse_args(raw).expect("valid args");
        assert!(cfg.isolated());
        assert_eq!(cfg.screenshot, Some(PathBuf::from("/tmp/out.png")));
        assert_eq!(cfg.script, Some(PathBuf::from("/tmp/steps.jsonl")));
        assert_eq!(cfg.wait_frames, 15);
        assert_eq!(files, vec![PathBuf::from("book.epub")]);
    }

    #[test]
    fn generate_token_is_32_hex_chars() {
        let tok = generate_token();
        assert_eq!(tok.len(), 32);
        assert!(tok.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
