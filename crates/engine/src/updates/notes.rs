//! SemVer comparison and structured `## Summary` release-notes parsing.

use serde::{Deserialize, Serialize};

use crate::updates::github::validate_https_url;

/// Concise, user-facing highlights parsed from a release's `## Summary` block.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseSummary {
    pub added: Vec<String>,
    pub fixes: Vec<String>,
}

impl ReleaseSummary {
    /// Whether neither `New` nor `Fixes` carries any items.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.fixes.is_empty()
    }
}

/// Parsed semantic version (`major.minor.patch`, optional pre-release tag).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    /// Parses `0.0.5`, `v0.0.5`, or `v0.0.5-rc.1` (ignoring `+build`).
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let rest = trimmed
            .strip_prefix('v')
            .or_else(|| trimmed.strip_prefix('V'))
            .unwrap_or(trimmed);
        let without_build = rest.split('+').next().unwrap_or(rest);
        let (core, pre) = match without_build.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_owned())),
            _ => (without_build, None),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Returns `true` only when `candidate` is strictly newer than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (Version::parse(candidate), Version::parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

const START_MARKER: &str = "<!-- release-summary:start -->";
const END_MARKER: &str = "<!-- release-summary:end -->";
const MAX_ITEMS: usize = 6;
const MAX_ITEM_CHARS: usize = 160;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bucket {
    None,
    New,
    Fixes,
}

/// Extracts `## Summary` (`### New` / `### Fixes`) from release Markdown.
///
/// Prefers content inside `<!-- release-summary:start/end -->` markers, then a
/// top-level `## Summary` section, and falls back to legacy `## What's new` /
/// `## Fixes` headings for pre-standard releases.
pub fn parse_summary(body: &str) -> ReleaseSummary {
    if let Some(slice) = between_markers(body) {
        let parsed = parse_structured(slice);
        if !parsed.is_empty() {
            return parsed;
        }
    }
    if let Some(slice) = section_under(body, "summary") {
        let parsed = parse_structured(slice);
        if !parsed.is_empty() {
            return parsed;
        }
    }
    parse_legacy(body)
}

/// Extracts an optional `<!-- release-url: https://... -->` override if valid.
pub fn parse_release_url(body: &str) -> Option<String> {
    for line in body.lines().take(40) {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("<!--") else {
            continue;
        };
        let Some(inner) = rest.strip_suffix("-->") else {
            continue;
        };
        let inner = inner.trim();
        let Some(url) = inner.strip_prefix("release-url:") else {
            continue;
        };
        let url = url.trim();
        if validate_https_url(url) {
            return Some(url.to_owned());
        }
    }
    None
}

/// Default release notes used by `--mock-update` when no file is provided.
pub fn default_mock_notes(version: &str) -> String {
    format!(
        "# Mareader {version}\n\n\
         <!-- release-summary:start -->\n\
         ## Summary\n\n\
         ### New\n\
         - Built-in update notifications and Settings › Updates controls.\n\
         - Silent background self-updates on Windows and Linux.\n\n\
         ### Fixes\n\
         - Preserved toolbar icon tints during rapid hover and click animations.\n\
         - Restored dark ink contrast in Dim mode.\n\
         <!-- release-summary:end -->\n"
    )
}

fn between_markers(body: &str) -> Option<&str> {
    let start = body.find(START_MARKER)?.checked_add(START_MARKER.len())?;
    let after = body.get(start..)?;
    let end = after.find(END_MARKER)?;
    after.get(..end)
}

fn section_under<'a>(body: &'a str, wanted_h2: &str) -> Option<&'a str> {
    let mut offset = 0usize;
    let mut start = None;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            let clean = heading.trim().to_ascii_lowercase();
            if start.is_some() {
                return body.get(start?..offset);
            }
            if clean == wanted_h2 {
                start = Some(offset.saturating_add(line.len()));
            }
        }
        offset = offset.saturating_add(line.len());
    }
    start.and_then(|s| body.get(s..))
}

fn parse_structured(slice: &str) -> ReleaseSummary {
    let mut out = ReleaseSummary::default();
    let mut bucket = Bucket::None;
    for raw in slice.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("<!--") {
            continue;
        }
        if let Some(h3) = line.strip_prefix("### ") {
            bucket = classify_heading(h3);
            continue;
        }
        if line.starts_with("## ") {
            if !line.trim().eq_ignore_ascii_case("## Summary") {
                break;
            }
            continue;
        }
        if let Some(item) = extract_bullet(line) {
            push_item(&mut out, bucket, item);
        }
    }
    out
}

fn parse_legacy(body: &str) -> ReleaseSummary {
    let mut out = ReleaseSummary::default();
    let mut bucket = Bucket::None;
    for raw in body.lines() {
        let line = raw.trim();
        if let Some(h2) = line.strip_prefix("## ") {
            bucket = classify_heading(h2);
            continue;
        }
        if let Some(h3) = line.strip_prefix("### ") {
            if bucket == Bucket::New && out.added.len() < 5 {
                let clean = clean_text(h3);
                if !clean.is_empty() {
                    out.added.push(clean);
                }
            }
            continue;
        }
        if let Some(item) = extract_bullet(line) {
            push_item(&mut out, bucket, item);
        }
    }
    out
}

fn classify_heading(raw: &str) -> Bucket {
    let lower = raw.trim().to_ascii_lowercase();
    if matches!(lower.as_str(), "new" | "what's new" | "whats new" | "added") {
        Bucket::New
    } else if matches!(lower.as_str(), "fixes" | "fixed" | "bug fixes" | "bugfixes") {
        Bucket::Fixes
    } else {
        Bucket::None
    }
}

fn extract_bullet(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))?;
    let clean = clean_text(rest);
    if clean.is_empty() { None } else { Some(clean) }
}

fn clean_text(raw: &str) -> String {
    let stripped = raw.replace("**", "").replace('`', "");
    let collapsed = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (i, ch) in collapsed.chars().enumerate() {
        if i >= MAX_ITEM_CHARS {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn push_item(out: &mut ReleaseSummary, bucket: Bucket, item: String) {
    let list = match bucket {
        Bucket::New => &mut out.added,
        Bucket::Fixes => &mut out.fixes,
        Bucket::None => return,
    };
    if list.len() < MAX_ITEMS {
        list.push(item);
    }
}
