//! Bounded HTTPS text fetches and dynamic SHA-256 verified downloads.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::AtomicU8;
use std::time::Duration;

use reqwest::StatusCode;
use sha2::{Digest, Sha256};

use crate::{Halt, Stopped};

/// Fetches UTF-8 text from `url`, refusing bodies larger than `max_bytes`.
pub fn get_text(url: &str, max_bytes: u64, headers: &[(&str, &str)]) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("mareader2/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let response = req.send().map_err(|e| e.to_string())?;
    let status = response.status();
    if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
        return Err("Update server rate limit reached; try again later.".into());
    }
    if !status.is_success() {
        return Err(format!("Update server answered {status}."));
    }
    if response.content_length().is_some_and(|n| n > max_bytes) {
        return Err("Response exceeded the size limit.".into());
    }
    let mut body = Vec::new();
    let limit = max_bytes.saturating_add(1);
    response
        .take(limit)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if (body.len() as u64) > max_bytes {
        return Err("Response exceeded the size limit.".into());
    }
    String::from_utf8(body).map_err(|_| "Response was not valid UTF-8.".into())
}

/// Computes the lowercase 64-hex SHA-256 digest of the file at `path`.
pub fn sha256_hex(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(buf.get(..n).unwrap_or_default());
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Streams `url` into `part`, enforcing `max_bytes` and `expected_sha256`.
pub fn download_verified(
    url: &str,
    part: &Path,
    expected_sha256: &str,
    max_bytes: u64,
    halt: &AtomicU8,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), Stopped> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(20 * 60))
        .user_agent(concat!("mareader2/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| Stopped::Failed(e.to_string()))?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| Stopped::Failed(e.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(Stopped::Failed(format!(
            "Download server answered {status}."
        )));
    }
    let total = response.content_length();
    if total.is_some_and(|n| n > max_bytes) {
        return Err(Stopped::Failed("Download exceeded the size limit.".into()));
    }
    let (got, hex) = stream_and_hash(&mut response, part, total, max_bytes, halt, progress)?;
    if total.is_some_and(|t| got < t) {
        let _ = std::fs::remove_file(part);
        return Err(Stopped::Failed("Download was cut short.".into()));
    }
    if !hex.eq_ignore_ascii_case(expected_sha256) {
        let _ = std::fs::remove_file(part);
        return Err(Stopped::Failed(
            "Downloaded file failed SHA-256 verification.".into(),
        ));
    }
    Ok(())
}

fn stream_and_hash(
    response: &mut reqwest::blocking::Response,
    part: &Path,
    total: Option<u64>,
    max_bytes: u64,
    halt: &AtomicU8,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(u64, String), Stopped> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(part)
        .map_err(|e| Stopped::Failed(e.to_string()))?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut got = 0u64;
    progress(0, total);
    loop {
        match Halt::read(halt) {
            Halt::Go => {}
            h => {
                drop(file);
                let _ = std::fs::remove_file(part);
                return Err(Stopped::Halted(h));
            }
        }
        let n = response
            .read(&mut buf)
            .map_err(|e| Stopped::Failed(e.to_string()))?;
        if n == 0 {
            break;
        }
        let chunk = buf.get(..n).unwrap_or_default();
        got = got.saturating_add(n as u64);
        if got > max_bytes {
            drop(file);
            let _ = std::fs::remove_file(part);
            return Err(Stopped::Failed("Download exceeded the size limit.".into()));
        }
        file.write_all(chunk)
            .map_err(|e| Stopped::Failed(e.to_string()))?;
        hash.update(chunk);
        progress(got, total);
    }
    file.flush().map_err(|e| Stopped::Failed(e.to_string()))?;
    let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok((got, hex))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    use super::*;

    fn serve_once(status: &str, body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let status = status.to_owned();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                if line == "\r\n" || line == "\n" {
                    break;
                }
                line.clear();
            }
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        });
        format!("http://{addr}/asset")
    }

    #[test]
    fn get_text_fetches_and_caps_body() {
        let url = serve_once("200 OK", b"hello".to_vec());
        assert_eq!(get_text(&url, 16, &[]).unwrap(), "hello");
        let big = serve_once("200 OK", vec![b'x'; 32]);
        assert!(get_text(&big, 16, &[]).is_err());
    }

    #[test]
    fn download_verified_checks_sha256() {
        let payload = b"mareader update payload".to_vec();
        let good_hex: String = Sha256::digest(&payload)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let dir = std::env::temp_dir().join(format!("mareader-fetch-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let part = dir.join("update.bin");
        let halt = AtomicU8::new(0);

        let url = serve_once("200 OK", payload.clone());
        let mut seen = 0u64;
        download_verified(&url, &part, &good_hex, 1024, &halt, &mut |got, _| {
            seen = got;
        })
        .unwrap();
        assert_eq!(seen, payload.len() as u64);
        assert_eq!(sha256_hex(&part).unwrap(), good_hex);

        let bad_url = serve_once("200 OK", payload);
        let err = download_verified(
            &bad_url,
            &part,
            &"0".repeat(64),
            1024,
            &halt,
            &mut |_, _| {},
        );
        assert!(matches!(err, Err(Stopped::Failed(_))));
        assert!(!part.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
