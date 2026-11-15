//! Synchronous loopback TCP client for Mareader's live UI control channel.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

use mareader_engine::journal::Step;
use serde::Deserialize;
use serde_json::{Map, Value, json};

const IO_TIMEOUT: Duration = Duration::from_secs(15);

/// Connection parameters parsed from `--control-file`.
#[derive(Clone, Debug, Deserialize)]
pub struct ControlEndpoint {
    pub host: String,
    pub port: u16,
    pub token: String,
    #[serde(default)]
    pub pid: Option<u32>,
}

/// Loopback client speaking newline-delimited JSON to a live desktop instance.
#[derive(Clone, Debug)]
pub struct ControlClient {
    port: u16,
    token: String,
}

impl ControlClient {
    /// Builds a client targeting `127.0.0.1:port` with `token`.
    pub fn new(port: u16, token: String) -> Self {
        Self { port, token }
    }

    /// Loads loopback port and token from a `--control-file` JSON descriptor.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("read control file {}: {e}", path.display()))?;
        let ep: ControlEndpoint = serde_json::from_str(&text)
            .map_err(|e| format!("parse control file {}: {e}", path.display()))?;
        if ep.host != "127.0.0.1" && ep.host != "localhost" {
            return Err(format!("refusing non-loopback control host {:?}", ep.host));
        }
        Ok(Self::new(ep.port, ep.token))
    }

    /// Sends one authenticated JSON-line request and returns the reply payload.
    pub fn call(&self, op: &str, extra: Value) -> Result<Value, String> {
        let mut body = match extra {
            Value::Object(map) => map,
            Value::Null => Map::new(),
            _ => return Err("request arguments must be a JSON object".to_owned()),
        };
        body.insert("token".to_owned(), Value::String(self.token.clone()));
        body.insert("op".to_owned(), Value::String(op.to_owned()));

        let mut line =
            serde_json::to_vec(&Value::Object(body)).map_err(|e| format!("encode request: {e}"))?;
        line.push(b'\n');

        let addr = format!("127.0.0.1:{}", self.port);
        let mut stream = TcpStream::connect(&addr).map_err(|e| format!("connect {addr}: {e}"))?;
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        stream
            .write_all(&line)
            .map_err(|e| format!("write request: {e}"))?;
        stream.flush().map_err(|e| format!("flush request: {e}"))?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .map_err(|e| format!("read response: {e}"))?;
        if response.trim().is_empty() {
            return Err("empty response from live UI control server".to_owned());
        }
        let parsed: Value =
            serde_json::from_str(&response).map_err(|e| format!("invalid reply JSON: {e}"))?;
        let ok = parsed.get("ok").and_then(Value::as_bool).unwrap_or(false);
        if ok {
            Ok(parsed.get("result").cloned().unwrap_or(Value::Null))
        } else {
            let msg = parsed
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("control request failed");
            Err(msg.to_owned())
        }
    }

    pub fn ping(&self) -> Result<Value, String> {
        self.call("ping", Value::Null)
    }

    pub fn state(&self) -> Result<Value, String> {
        self.call("state", Value::Null)
    }

    pub fn commands(&self) -> Result<Value, String> {
        self.call("commands", Value::Null)
    }

    pub fn exec(&self, id: &str, params: Value) -> Result<Value, String> {
        self.call("exec", json!({ "id": id, "params": params }))
    }

    pub fn input(&self, actions: Value) -> Result<Value, String> {
        self.call("input", json!({ "actions": actions }))
    }

    pub fn screenshot(&self, path: Option<&str>) -> Result<Value, String> {
        self.call("screenshot", json!({ "path": path }))
    }

    pub fn journal(&self, clear: bool) -> Result<Value, String> {
        self.call("journal", json!({ "clear": clear }))
    }

    pub fn replay(&self, steps: &[Step]) -> Result<Value, String> {
        self.call("replay", json!({ "steps": steps }))
    }

    pub fn query(&self, kind: &str, params: Value) -> Result<Value, String> {
        self.call("query", json!({ "kind": kind, "params": params }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn client_roundtrips_json_line_over_loopback() {
        let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else {
            return;
        };
        let Ok(addr) = listener.local_addr() else {
            return;
        };
        let handle = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(s) => s,
                Err(_) => return,
            });
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let ok = line.contains("\"token\":\"secret\"") && line.contains("\"op\":\"ping\"");
            let reply = if ok {
                "{\"ok\":true,\"result\":{\"pong\":true}}\n"
            } else {
                "{\"ok\":false,\"error\":\"bad request\"}\n"
            };
            let _ = stream.write_all(reply.as_bytes());
        });

        let client = ControlClient::new(addr.port(), "secret".to_owned());
        let res = client.ping();
        let _ = handle.join();
        assert_eq!(res, Ok(json!({ "pong": true })));
    }
}
