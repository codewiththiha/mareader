//! A book's sound and video: the system's player, and its files as clips.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use mareader_audio::{Clip, Fetch};
use mareader_epub::Files;
use mareader_session::Document;

use crate::host::Shared;

/// Copies older than this are cleared when another clip plays.
const KEEP: Duration = Duration::from_secs(24 * 60 * 60);
/// Bytes of a clip that name its copy, with its path and length.
const NAMED_BY: usize = 64 << 10;

/// Copies `path` of `doc` out and hands it to the host, off this thread.
///
/// False when the thread could not start; a host that refuses the file says
/// so by returning false, which nobody here is left to hear.
pub fn play(host: &Shared, doc: Arc<Document>, path: String) -> bool {
    let host = Arc::clone(host);
    let spawned = std::thread::Builder::new()
        .name("mareader-play".into())
        .spawn(move || {
            if let Some(file) = copy_out(host.cache_dir().as_deref(), &doc, &path) {
                host.open_external(&file);
            }
        });
    spawned.is_ok()
}

/// Reads sound from `files`, keeping the last file for the next run.
pub fn fetch(files: Files) -> Fetch {
    let last: Mutex<Option<(String, Clip)>> = Mutex::new(None);
    Arc::new(move |path: &str| {
        let mut last = last.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, clip)) = last.as_ref().filter(|(p, _)| p == path) {
            return Some(clip.clone());
        }
        let media = files.media(path)?;
        let clip = Clip::File {
            data: Arc::new(media.data),
            extension: media.extension,
        };
        *last = Some((path.to_owned(), clip.clone()));
        Some(clip)
    })
}

/// The clip copied out, named by its declared type, never the book's name.
///
/// Copies go where the host says files that can be built again go, and in a
/// temporary folder when the host has nowhere.
fn copy_out(cache: Option<&Path>, doc: &Document, path: &str) -> Option<PathBuf> {
    let media = doc.media(path)?;
    let base = cache
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let folder = base.join("mareader-media");
    std::fs::create_dir_all(&folder).ok()?;
    prune(&folder);
    let file = folder.join(name(path, &media.data, media.extension));
    write_once(&file, &media.data)?;
    // Players load subtitles named after the clip, as `<clip>.en.vtt`.
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let mut taken = Vec::new();
    for sub in doc.subtitles(path) {
        let named = subtitle_name(stem, sub.language.as_deref(), sub.extension);
        if !taken.contains(&named) {
            let _ = write_once(&folder.join(&named), &sub.data);
            taken.push(named);
        }
    }
    Some(file)
}

/// Writes `data` to `file` unless a copy of its size is there already.
fn write_once(file: &Path, data: &[u8]) -> Option<()> {
    let fresh = std::fs::metadata(file).is_ok_and(|m| m.len() == data.len() as u64);
    if !fresh {
        std::fs::write(file, data).ok()?;
    }
    Some(())
}

/// `<stem>.<language>.<extension>`, the language kept to tag characters.
fn subtitle_name(stem: &str, language: Option<&str>, extension: &str) -> String {
    let tag: String = (language.unwrap_or_default().chars())
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(16)
        .collect();
    match tag.is_empty() {
        true => format!("{stem}.{extension}"),
        false => format!("{stem}.{tag}.{extension}"),
    }
}

/// `<hash>.<extension>`: the same clip keeps one name, others never share it.
fn name(path: &str, data: &[u8], extension: &str) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    data.len().hash(&mut hasher);
    data.get(..NAMED_BY).unwrap_or(data).hash(&mut hasher);
    format!("{:016x}.{extension}", hasher.finish())
}

/// Removes copies left from earlier days; ones in use may stay.
fn prune(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten().take(256) {
        let modified = entry.metadata().and_then(|m| m.modified());
        let old = modified.is_ok_and(|m| now.duration_since(m).is_ok_and(|age| age > KEEP));
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mareader_reflow::Typography;
    use mareader_session::Document;

    use super::{name, play, subtitle_name};
    use crate::host::{Fake, Shared};
    use crate::testing::waited;

    /// The epub crate's media book: a clip with a poster, and a song.
    const MEDIA: &[u8] = include_bytes!("../../epub/tests/books/media.epub");
    /// The video that book names, as its own trigger plays it.
    const CLIP: &str = "/OEBPS/clip.mp4";

    /// A directory that goes when the test does.
    fn dir() -> Result<tempfile::TempDir, String> {
        tempfile::tempdir().map_err(|e| e.to_string())
    }

    /// This job's answer is the host's, not an event: a thread copies the clip
    /// out and hands the copy over, so the test waits on the host instead.
    #[test]
    fn a_clip_leaves_on_its_thread_and_reaches_the_host() -> Result<(), String> {
        let dir = dir()?;
        let host = Arc::new(Fake::under(dir.path()));
        let machine: Shared = host.clone();
        let book = Document::open(Arc::new(MEDIA.to_vec()), &Typography::default());
        let doc = Arc::new(book.map_err(|e| e.to_string())?);
        let clip = doc.media(CLIP).ok_or("the book names no clip")?;

        assert!(
            play(&machine, Arc::clone(&doc), CLIP.to_owned()),
            "a thread"
        );
        assert!(waited(|| !host.opened().is_empty()), "the copy arrived");

        let opened = host.opened().first().cloned().ok_or("nothing opened")?;
        let cache = dir.path().join("cache").join("mareader-media");
        assert_eq!(opened.parent(), Some(cache.as_path()), "{opened:?}");
        assert_eq!(opened.extension().and_then(|e| e.to_str()), Some("mp4"));
        let bytes = std::fs::read(&opened).map_err(|e| e.to_string())?;
        assert_eq!(bytes, clip.data, "the copy is the clip's own bytes");
        Ok(())
    }

    #[test]
    fn copies_are_named_by_clip_and_type() {
        let a = name("/OPS/a.mp4", b"clip", "mp4");
        assert_eq!(a, name("/OPS/a.mp4", b"clip", "mp4"));
        assert_ne!(a, name("/OPS/b.mp4", b"clip", "mp4"));
        assert_ne!(a, name("/OPS/a.mp4", b"clap", "mp4"));
        assert!(a.ends_with(".mp4") && !a.contains('/'), "{a}");
    }

    #[test]
    fn subtitles_are_named_after_their_clip() {
        assert_eq!(
            subtitle_name("ab12", Some("en-GB"), "vtt"),
            "ab12.en-GB.vtt"
        );
        assert_eq!(subtitle_name("ab12", None, "srt"), "ab12.srt");
        assert_eq!(subtitle_name("ab12", Some("../x/y"), "srt"), "ab12.xy.srt");
    }
}
