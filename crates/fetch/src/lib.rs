//! Downloads (L0): mirrors in turn, resumable, checked before kept.
//! Desktop only; see `docs/architecture.md`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]
#![cfg(not(target_arch = "wasm32"))]

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};
use sha2::{Digest, Sha256};

/// A parquet starts and ends with this; an error page does neither.
const PARQUET: &[u8; 4] = b"PAR1";
/// Every zstd frame starts with this.
const ZSTD: &[u8; 4] = &[0x28, 0xb5, 0x2f, 0xfd];
/// No unlisted download comes near this; a bigger body is something else.
const MAX_BYTES: u64 = 256 * 1024 * 1024;
/// Tries per address before the next mirror.
const TRIES: usize = 2;

/// Set a worker's inline mode when one is attached.
pub fn set_inline<D>(desk: &mut Option<D>, inline: bool, set: impl FnOnce(&mut D, bool)) {
    if let Some(desk) = desk {
        set(desk, inline);
    }
}

/// Whether a native worker is attached.
pub fn attached<D>(desk: &Option<D>) -> bool {
    desk.is_some()
}

/// Forward inline mode on native, and do nothing in a browser.
#[macro_export]
macro_rules! inline_method {
    () => {
        #[cfg(not(target_arch = "wasm32"))]
        pub fn set_inline(&mut self, inline: bool) {
            $crate::set_inline(&mut self.desk, inline, |desk, value| desk.inline = value);
        }

        #[cfg(target_arch = "wasm32")]
        pub fn set_inline(&mut self, _inline: bool) {}
    };
}

/// Report the native worker's presence, always false in a browser.
#[macro_export]
macro_rules! attached_method {
    () => {
        pub fn attached(&self) -> bool {
            #[cfg(not(target_arch = "wasm32"))]
            return $crate::attached(&self.desk);
            #[cfg(target_arch = "wasm32")]
            false
        }
    };
}

/// What a download is told to do, between chunks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Halt {
    Go = 0,
    /// Stop and keep the part, to resume.
    Pause = 1,
    /// Stop and delete the part.
    Cancel = 2,
}

impl Halt {
    pub fn read(flag: &AtomicU8) -> Self {
        match flag.load(Ordering::Relaxed) {
            1 => Self::Pause,
            2 => Self::Cancel,
            _ => Self::Go,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stopped {
    /// Told to stop; the part is kept on a pause.
    Halted(Halt),
    /// Every address failed; the last reason.
    Failed(String),
}

/// What a whole download must look like to be kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The magic at both ends, so a cut-short file shows.
    Parquet,
    /// The frame magic at the start; length comes from the server.
    Zstd,
    /// Exactly this long, with this SHA-256 in lowercase hex.
    Sha256 { sha256: &'static str, bytes: u64 },
}

impl Kind {
    /// The most a body of this kind may weigh.
    pub fn most(self) -> u64 {
        match self {
            Self::Sha256 { bytes, .. } => bytes,
            Self::Parquet | Self::Zstd => MAX_BYTES,
        }
    }

    /// Whether the file at `path` is this kind, in the reader's words.
    pub fn check(self, path: &Path) -> Result<(), String> {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        if let Self::Sha256 { sha256, bytes } = self {
            return listed(&mut file, len, (sha256, bytes));
        }
        let mut head = [0; 4];
        file.read_exact(&mut head)
            .map_err(|_| "the file is empty")?;
        let wrong = || Err("the server sent the wrong kind of file".into());
        match self {
            Self::Zstd if &head == ZSTD => Ok(()),
            Self::Zstd => wrong(),
            Self::Parquet if &head != PARQUET => wrong(),
            Self::Sha256 { .. } => Ok(()),
            Self::Parquet => {
                let mut tail = [0; 4];
                file.seek(SeekFrom::End(-4)).map_err(|e| e.to_string())?;
                file.read_exact(&mut tail).map_err(|e| e.to_string())?;
                if &tail == PARQUET && len >= 8 {
                    Ok(())
                } else {
                    Err("the file is cut short".into())
                }
            }
        }
    }
}

/// Whether `file` is `bytes` long and hashes to `sha256`.
fn listed(file: &mut File, len: u64, (sha256, bytes): (&str, u64)) -> Result<(), String> {
    if len < bytes {
        return Err("the file is cut short".into());
    }
    if len > bytes {
        return Err("the file is longer than listed".into());
    }
    let mut hash = Sha256::new();
    let mut buf = vec![0; 256 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(buf.get(..n).unwrap_or_default());
    }
    let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    match hex.eq_ignore_ascii_case(sha256) {
        true => Ok(()),
        false => Err("the file isn't the one listed".into()),
    }
}

/// Fetches the first `kind` file `urls` serve into `part`, with progress.
pub fn fetch(
    urls: &[String],
    part: &Path,
    kind: Kind,
    halt: &AtomicU8,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), Stopped> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30 * 60))
        .user_agent(concat!("mareader2/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| Stopped::Failed(e.to_string()))?;
    let mut why = String::from("no address to fetch from");
    for url in urls {
        for _ in 0..TRIES {
            match one(&client, url, part, kind, halt, progress) {
                Ok(()) => match kind.check(part) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        // A whole body of the wrong kind: try the next.
                        let _ = std::fs::remove_file(part);
                        why = e;
                        break;
                    }
                },
                Err(Stopped::Halted(h)) => {
                    if h == Halt::Cancel {
                        let _ = std::fs::remove_file(part);
                    }
                    return Err(Stopped::Halted(h));
                }
                Err(Stopped::Failed(e)) => why = e,
            }
        }
    }
    Err(Stopped::Failed(why))
}

/// One request: the rest of the body after what `part` holds.
fn one(
    client: &reqwest::blocking::Client,
    url: &str,
    part: &Path,
    kind: Kind,
    halt: &AtomicU8,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), Stopped> {
    let failed = |e: &dyn std::fmt::Display| Stopped::Failed(e.to_string());
    let have = std::fs::metadata(part).map_or(0, |m| m.len());
    let mut request = client.get(url);
    if have > 0 {
        request = request.header(RANGE, format!("bytes={have}-"));
    }
    let mut response = request.send().map_err(|e| failed(&e))?;
    let status = response.status();
    let (mut got, total) = match status {
        StatusCode::PARTIAL_CONTENT if starts_at(&response) == Some(have) => {
            (have, response.content_length().map(|n| n + have))
        }
        StatusCode::RANGE_NOT_SATISFIABLE if have > 0 => {
            // Already whole, or the part is longer than the file.
            return match kind.check(part) {
                Ok(()) => Ok(()),
                Err(_) => {
                    let _ = std::fs::remove_file(part);
                    Err(Stopped::Failed("the partial download was stale".into()))
                }
            };
        }
        s if s.is_success() => (0, response.content_length()),
        s => return Err(Stopped::Failed(format!("the server answered {s}"))),
    };
    if total.is_some_and(|t| t > kind.most()) {
        return Err(Stopped::Failed("the file is far too large".into()));
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(got > 0)
        .write(true)
        .truncate(got == 0)
        .open(part)
        .map_err(|e| failed(&e))?;
    progress(got, total);
    let mut buf = vec![0; 64 * 1024];
    loop {
        match Halt::read(halt) {
            Halt::Go => {}
            h => return Err(Stopped::Halted(h)),
        }
        let n = response.read(&mut buf).map_err(|e| failed(&e))?;
        if n == 0 {
            break;
        }
        let chunk = buf.get(..n).unwrap_or_default();
        file.write_all(chunk).map_err(|e| failed(&e))?;
        got += n as u64;
        if got > kind.most() {
            return Err(Stopped::Failed("the file is far too large".into()));
        }
        progress(got, total);
    }
    file.flush().map_err(|e| failed(&e))?;
    match total {
        Some(t) if got < t => Err(Stopped::Failed("the download was cut short".into())),
        _ => Ok(()),
    }
}

/// Where a 206 body starts, from `Content-Range: bytes N-M/T`.
fn starts_at(response: &reqwest::blocking::Response) -> Option<u64> {
    let range = response.headers().get(CONTENT_RANGE)?.to_str().ok()?;
    let rest = range.strip_prefix("bytes ")?;
    rest.split('-').next()?.trim().parse().ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use super::*;

    /// A body that is a parquet at both ends.
    fn body() -> Vec<u8> {
        let mut b = b"PAR1".to_vec();
        b.extend((0..200_000u32).map(|i| (i % 251) as u8));
        b.extend_from_slice(b"PAR1");
        b
    }

    /// `/pack` takes ranges, `/cut` drops once halfway, `/page` is HTML,
    /// else 404.
    fn serve() -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let resumes = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&resumes);
        std::thread::spawn(move || {
            let mut cuts = 0;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split(' ').nth(1).unwrap_or("/").to_owned();
                let mut from = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap() <= 2 {
                        break;
                    }
                    if let Some(r) = h.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        from = r.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                if from > 0 {
                    counted.fetch_add(1, Ordering::Relaxed);
                }
                let full = body();
                let (status, bytes, extra) = match path.as_str() {
                    "/pack" | "/cut" if from > 0 => {
                        let range = format!(
                            "Content-Range: bytes {from}-{}/{}\r\n",
                            full.len() - 1,
                            full.len()
                        );
                        ("206 Partial Content", full[from..].to_vec(), range)
                    }
                    "/pack" | "/cut" => ("200 OK", full, String::new()),
                    "/page" => ("200 OK", b"<html>Not here</html>".to_vec(), String::new()),
                    _ => ("404 Not Found", b"no".to_vec(), String::new()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    bytes.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let send = if path == "/cut" && from == 0 && cuts == 0 {
                    cuts += 1;
                    bytes.len() / 2
                } else {
                    bytes.len()
                };
                let _ = stream.write_all(&bytes[..send]);
            }
        });
        (format!("http://{addr}"), resumes)
    }

    #[test]
    fn mirrors_are_tried_in_turn_and_an_error_page_is_never_kept() {
        let (base, _) = serve();
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("p.part");
        let go = AtomicU8::new(0);
        let urls = [
            format!("{base}/gone"),
            format!("{base}/page"),
            format!("{base}/pack"),
        ];
        let mut seen = Vec::new();
        let parquet = Kind::Parquet;
        fetch(&urls, &part, parquet, &go, &mut |got, total| {
            seen.push((got, total));
        })
        .unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), body());
        assert_eq!(
            seen.last(),
            Some(&(body().len() as u64, Some(body().len() as u64)))
        );
        let only_bad = [format!("{base}/page")];
        let err = fetch(&only_bad, &part, parquet, &go, &mut |_, _| {});
        let wrong = "the server sent the wrong kind of file";
        assert_eq!(err, Err(Stopped::Failed(wrong.into())));
        assert!(!part.exists());
    }

    #[test]
    fn a_dropped_connection_resumes_where_it_stopped() {
        let (base, resumes) = serve();
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("p.part");
        let go = AtomicU8::new(0);
        let urls = [format!("{base}/cut")];
        fetch(&urls, &part, Kind::Parquet, &go, &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), body());
        assert_eq!(resumes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_pause_keeps_the_part_and_a_cancel_drops_it() {
        let (base, _) = serve();
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("p.part");
        let urls = [format!("{base}/pack")];
        let pause = AtomicU8::new(Halt::Pause as u8);
        let err = fetch(&urls, &part, Kind::Parquet, &pause, &mut |_, _| {});
        assert_eq!(err, Err(Stopped::Halted(Halt::Pause)));
        assert!(part.exists());
        let cancel = AtomicU8::new(Halt::Cancel as u8);
        let err = fetch(&urls, &part, Kind::Parquet, &cancel, &mut |_, _| {});
        assert_eq!(err, Err(Stopped::Halted(Halt::Cancel)));
        assert!(!part.exists());
    }

    #[test]
    fn a_parquet_is_checked_at_both_ends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        let check = |bytes: &[u8]| {
            std::fs::write(&path, bytes).unwrap();
            Kind::Parquet.check(&path)
        };
        assert_eq!(check(b"PAR1 half"), Err("the file is cut short".into()));
        assert!(check(b"").is_err());
        assert_eq!(check(b"PAR1PAR1"), Ok(()));
    }

    #[test]
    fn a_listed_file_is_checked_by_length_and_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        std::fs::write(&path, b"abc").unwrap();
        // SHA-256 of "abc".
        let sha256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(Kind::Sha256 { sha256, bytes: 3 }.check(&path), Ok(()));
        let long = Kind::Sha256 { sha256, bytes: 4 };
        assert_eq!(long.check(&path), Err("the file is cut short".into()));
        let short = Kind::Sha256 { sha256, bytes: 2 };
        assert_eq!(
            short.check(&path),
            Err("the file is longer than listed".into())
        );
        let zeros = "0000000000000000000000000000000000000000000000000000000000000000";
        let other = Kind::Sha256 {
            sha256: zeros,
            bytes: 3,
        };
        assert_eq!(
            other.check(&path),
            Err("the file isn't the one listed".into())
        );
        assert_eq!(long.most(), 4);
    }

    #[test]
    fn a_zstd_frame_is_checked_by_its_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        std::fs::write(&path, [0x28, 0xb5, 0x2f, 0xfd, 1, 2]).unwrap();
        assert_eq!(Kind::Zstd.check(&path), Ok(()));
        std::fs::write(&path, b"<html>").unwrap();
        assert!(Kind::Zstd.check(&path).is_err());
        assert!(Kind::Parquet.check(&path).is_err());
    }
}
