//! One HTTP attempt against one mirror, and the adoption of what it landed.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::HeaderName;
use reqwest::{Client, Response, StatusCode};
use tokio::io::AsyncWriteExt;

use crate::Phase;
use crate::flags::Flags;
use crate::job::Verify;
use crate::partial::{Sidecar, discard, partial_of, sidecar_of};

/// Bytes are reported at this pace; a phase change always is.
const EMIT_EVERY: Duration = Duration::from_millis(120);

/// The speed window; a shorter one reads as jitter, a longer one as lag.
const METER_WINDOW: Duration = Duration::from_millis(750);

/// How one attempt ended.
pub(crate) enum Step {
    /// Every byte is on disk at the partial.
    Complete,
    /// The caller stopped the read.
    Stopped(Phase),
    /// The same mirror may answer again after a backoff.
    Transient(String),
    /// This mirror will not answer; spend the next one.
    Permanent(String),
}

/// Byte progress: received, declared total, and the smoothed speed.
pub(crate) type Note<'a> = &'a (dyn Fn(u64, Option<u64>, Option<f64>) + Send + Sync);

/// A smoothed bytes-per-second reading, one window at a time.
struct Meter {
    window_start: Instant,
    window_bytes: u64,
    speed: Option<f64>,
}

impl Meter {
    fn new(received: u64) -> Self {
        Self {
            window_start: Instant::now(),
            window_bytes: received,
            speed: None,
        }
    }

    /// The speed, recomputed once a window has passed since the last one.
    fn note(&mut self, received: u64) -> Option<f64> {
        let elapsed = self.window_start.elapsed();
        if elapsed < METER_WINDOW {
            return self.speed;
        }
        let moved = received.saturating_sub(self.window_bytes);
        let instant = moved as f64 / elapsed.as_secs_f64().max(1e-6);
        // Half the old reading: a burst neither spikes the bar nor freezes it.
        self.speed = Some(match self.speed {
            Some(previous) => (previous + instant) / 2.0,
            None => instant,
        });
        self.window_start = Instant::now();
        self.window_bytes = received;
        self.speed
    }
}

/// One attempt at `url`, resuming only what the sidecar can vouch for.
pub(crate) async fn attempt(
    client: &Client,
    url: &str,
    source: u32,
    dest: &Path,
    flags: &Flags,
    note: Note<'_>,
) -> Step {
    let partial = partial_of(dest);
    let held = tokio::fs::metadata(&partial)
        .await
        .map(|meta| meta.len())
        .unwrap_or(0);

    let mut request = client.get(url);
    let mut asked_range = false;
    // A blind Range appends to another revision's bytes; If-Range asks.
    if held > 0 {
        let validator =
            Sidecar::load(dest).and_then(|sidecar| sidecar.validator().map(str::to_string));
        match validator {
            Some(validator) => {
                asked_range = true;
                request = request
                    .header(reqwest::header::RANGE, format!("bytes={held}-"))
                    .header(reqwest::header::IF_RANGE, validator);
            }
            None => discard(dest),
        }
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => return Step::Transient(format!("connect: {error}")),
    };

    let (append, total) = match response.status() {
        StatusCode::PARTIAL_CONTENT if asked_range => (
            true,
            content_range_total(header(&response, &reqwest::header::CONTENT_RANGE).as_deref()),
        ),
        // The resource moved, or the mirror ignores ranges: start over.
        StatusCode::OK | StatusCode::PARTIAL_CONTENT => (false, response.content_length()),
        StatusCode::RANGE_NOT_SATISFIABLE => {
            // `bytes */total` names the whole resource; a partial that
            // reaches it is finished, not stale.
            let whole =
                content_range_total(header(&response, &reqwest::header::CONTENT_RANGE).as_deref());
            if held > 0 && whole == Some(held) {
                return Step::Complete;
            }
            if held == 0 {
                return Step::Permanent("range refused with nothing held".into());
            }
            discard(dest);
            return Step::Transient("partial refused; restarting".into());
        }
        status if retryable(status) => return Step::Transient(format!("server said {status}")),
        status => return Step::Permanent(format!("server said {status}")),
    };

    // Written before the body: a crash mid-stream still resumes safely.
    let sidecar = Sidecar {
        etag: header(&response, &reqwest::header::ETAG),
        last_modified: header(&response, &reqwest::header::LAST_MODIFIED),
        source,
        total,
    };
    let _ = sidecar.save(dest);

    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).write(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    let mut file = match options.open(&partial).await {
        Ok(file) => file,
        Err(error) => return Step::Transient(format!("open partial: {error}")),
    };

    let mut received = if append { held } else { 0 };
    note(received, total, None);
    let mut meter = Meter::new(received);
    let mut last_emit = Instant::now();
    let mut stream = response.bytes_stream();
    loop {
        // Between chunks only: cutting a read in half is what loses a body.
        if let Some(phase) = flags.stop() {
            let _ = file.flush().await;
            // Reported after the flush: what the snapshot says is what is held.
            note(received, total, meter.note(received));
            return Step::Stopped(phase);
        }
        let chunk = match stream.next().await {
            Some(Ok(chunk)) => chunk,
            Some(Err(error)) => {
                // What landed stays; the next attempt resumes from it.
                let _ = file.flush().await;
                return Step::Transient(format!("stream: {error}"));
            }
            None => break,
        };
        received += chunk.len() as u64;
        if let Err(error) = file.write_all(&chunk).await {
            let _ = file.flush().await;
            return Step::Transient(format!("write partial: {error}"));
        }
        if last_emit.elapsed() >= EMIT_EVERY {
            last_emit = Instant::now();
            note(received, total, meter.note(received));
        }
    }
    if let Err(error) = file.flush().await {
        return Step::Transient(format!("flush partial: {error}"));
    }
    if let Err(error) = file.sync_all().await {
        return Step::Transient(format!("sync partial: {error}"));
    }
    drop(file);
    note(received, total, meter.note(received));

    // A body that ends early is a broken connection, not a short file.
    if let Some(total) = total
        && received != total
    {
        return Step::Transient(format!("short body: {received} of {total}"));
    }
    Step::Complete
}

/// Verify the partial and adopt it as the destination, atomically.
pub(crate) async fn adopt(dest: &Path, check: &Option<Verify>) -> Result<u64, String> {
    let partial = partial_of(dest);
    let bytes = match tokio::fs::metadata(&partial).await {
        Ok(meta) => meta.len(),
        Err(error) => return Err(format!("stat partial: {error}")),
    };
    if let Err(message) = verify(check, &partial).await {
        discard(dest);
        return Err(message);
    }
    if let Err(error) = tokio::fs::rename(&partial, dest).await {
        discard(dest);
        return Err(format!("adopt: {error}"));
    }
    let _ = tokio::fs::remove_file(sidecar_of(dest)).await;
    Ok(bytes)
}

/// Run the verify hook, off the runtime: it may read the whole body.
pub(crate) async fn verify(check: &Option<Verify>, path: &Path) -> Result<(), String> {
    let Some(hook) = check else {
        return Ok(());
    };
    let hook = hook.clone();
    let path: PathBuf = path.to_path_buf();
    match tokio::task::spawn_blocking(move || hook(&path)).await {
        Ok(verdict) => verdict,
        Err(error) => Err(format!("verify worker: {error}")),
    }
}

/// A status worth asking the same mirror again.
fn retryable(status: StatusCode) -> bool {
    status.is_server_error()
        || status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
}

/// The total of a `Content-Range: bytes a-b/total` header.
pub(crate) fn content_range_total(value: Option<&str>) -> Option<u64> {
    value?
        .rsplit('/')
        .next()?
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|total| *total > 0)
}

fn header(response: &Response, name: &HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)?
        .to_str()
        .ok()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_totals_parse_and_reject_junk() {
        assert_eq!(content_range_total(Some("bytes 100-199/1234")), Some(1234));
        // The 416 form names the whole resource with no range in it.
        assert_eq!(content_range_total(Some("bytes */1234")), Some(1234));
        assert_eq!(content_range_total(Some("bytes 0-0/*")), None);
        assert_eq!(content_range_total(Some("bytes 0-0/0")), None);
        assert_eq!(content_range_total(Some("garbage")), None);
        assert_eq!(content_range_total(None), None);
    }

    #[test]
    fn only_a_stalled_server_or_a_busy_one_is_worth_reasking() {
        assert!(retryable(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(retryable(StatusCode::SERVICE_UNAVAILABLE));
        assert!(retryable(StatusCode::GATEWAY_TIMEOUT));
        assert!(retryable(StatusCode::REQUEST_TIMEOUT));
        assert!(retryable(StatusCode::TOO_MANY_REQUESTS));
        // A 404 is an answer, not an outage: waiting does not fix it.
        assert!(!retryable(StatusCode::NOT_FOUND));
        assert!(!retryable(StatusCode::FORBIDDEN));
        assert!(!retryable(StatusCode::MOVED_PERMANENTLY));
    }

    #[test]
    fn the_meter_needs_a_window_before_it_reads() {
        let mut meter = Meter::new(0);
        assert_eq!(meter.note(1_000_000), None);
        // Backdate the window instead of sleeping through one.
        meter.window_start = Instant::now() - Duration::from_secs(2);
        meter.window_bytes = 0;
        let speed = meter.note(2_000_000).unwrap();
        assert!((900_000.0..1_100_000.0).contains(&speed), "{speed}");
        // An idle window halves toward zero; it does not hold the burst.
        meter.window_start = Instant::now() - Duration::from_secs(2);
        let smoothed = meter.note(2_000_000).unwrap();
        assert!(smoothed < speed, "{smoothed} must settle under {speed}");
    }
}
