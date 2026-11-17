//! GitHub Releases JSON parsing, asset selection, and checksum extraction.

use serde::Deserialize;

use super::notes::{parse_release_url, parse_summary};
use super::{InstallKind, ReleaseInfo, UpdatePlatform};

/// Default GitHub Releases endpoint for Mareader.
pub const LATEST_API_URL: &str =
    "https://api.github.com/repos/codewiththiha/mareader2/releases/latest";

/// Fallback releases web page when a release carries no direct asset.
pub const RELEASES_PAGE_URL: &str = "https://github.com/codewiththiha/mareader2/releases/latest";

/// CPU architecture used for installer asset matching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    Aarch64,
    X86_64,
    Other,
}

impl Arch {
    pub const fn current() -> Self {
        if cfg!(target_arch = "aarch64") {
            Self::Aarch64
        } else if cfg!(target_arch = "x86_64") {
            Self::X86_64
        } else {
            Self::Other
        }
    }
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    digest: Option<String>,
}

/// Parsed release plus optional `SHA256SUMS` asset URL for checksum lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedRelease {
    pub info: ReleaseInfo,
    pub sums_url: Option<String>,
}

/// Parses a GitHub `releases/latest` JSON payload for this platform.
///
/// Returns `Ok(None)` for drafts or pre-releases.
pub fn parse_release_json(
    json: &str,
    platform: UpdatePlatform,
    arch: Arch,
    install: &InstallKind,
) -> Result<Option<ParsedRelease>, String> {
    let raw: ApiRelease =
        serde_json::from_str(json).map_err(|e| format!("Invalid release metadata: {e}"))?;
    if raw.draft || raw.prerelease {
        return Ok(None);
    }
    let version = raw
        .tag_name
        .trim()
        .strip_prefix('v')
        .or_else(|| raw.tag_name.trim().strip_prefix('V'))
        .unwrap_or(raw.tag_name.trim())
        .to_owned();
    let html_url = if validate_https_url(&raw.html_url) {
        raw.html_url.clone()
    } else {
        RELEASES_PAGE_URL.to_owned()
    };
    let body = raw.body.as_deref().unwrap_or("");
    let summary = parse_summary(body);
    let url_override = parse_release_url(body);
    let sums_url = raw
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS" && validate_https_url(&a.browser_download_url))
        .map(|a| a.browser_download_url.clone());

    let asset = pick_asset(&raw.assets, platform, arch, install);
    let (download_url, asset_url, asset_name, asset_size, sha256) = match asset {
        Some(a) if validate_https_url(&a.browser_download_url) => (
            url_override.unwrap_or_else(|| a.browser_download_url.clone()),
            Some(a.browser_download_url.clone()),
            Some(a.name.clone()),
            a.size,
            a.digest.as_deref().and_then(parse_sha256_digest),
        ),
        _ => (
            url_override.unwrap_or_else(|| html_url.clone()),
            None,
            None,
            None,
            None,
        ),
    };
    let info = ReleaseInfo {
        version,
        tag: raw.tag_name,
        html_url,
        download_url,
        asset_url,
        asset_name,
        asset_size,
        sha256,
        summary,
    };
    Ok(Some(ParsedRelease { info, sums_url }))
}

/// Looks up `asset_name`'s 64-hex SHA-256 checksum inside a `SHA256SUMS` file.
pub fn parse_sha256sums(body: &str, asset_name: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        let mut parts = trimmed.split_whitespace();
        let Some(hex) = parts.next() else { continue };
        let Some(name_raw) = parts.next() else {
            continue;
        };
        let name = name_raw.trim_start_matches('*');
        if (name == asset_name || name.ends_with(&format!("/{asset_name}"))) && is_sha256_hex(hex) {
            return Some(hex.to_ascii_lowercase());
        }
    }
    None
}

/// Validates a GitHub API asset `digest` string (`"sha256:<64-hex>"`).
pub fn parse_sha256_digest(raw: &str) -> Option<String> {
    let hex = raw.trim().strip_prefix("sha256:")?;
    if is_sha256_hex(hex) {
        Some(hex.to_ascii_lowercase())
    } else {
        None
    }
}

/// Returns `true` when `hex` is exactly 64 ASCII hexadecimal characters.
pub fn is_sha256_hex(hex: &str) -> bool {
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Accepts only HTTPS URLs pointing at Mareader or GitHub download hosts.
pub fn validate_https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    if rest.is_empty()
        || url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>'))
    {
        return false;
    }
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        host.as_str(),
        "github.com"
            | "api.github.com"
            | "objects.githubusercontent.com"
            | "release-assets.githubusercontent.com"
            | "mareader.app"
            | "www.mareader.app"
    )
}

fn pick_asset<'a>(
    assets: &'a [ApiAsset],
    platform: UpdatePlatform,
    arch: Arch,
    install: &InstallKind,
) -> Option<&'a ApiAsset> {
    let find_suffix = |suffix: &str| {
        let lower = suffix.to_ascii_lowercase();
        assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().ends_with(&lower))
    };
    match platform {
        UpdatePlatform::Mac => match arch {
            Arch::Aarch64 => find_suffix("_aarch64.dmg").or_else(|| find_suffix(".dmg")),
            Arch::X86_64 => find_suffix("_x64.dmg").or_else(|| find_suffix(".dmg")),
            Arch::Other => find_suffix(".dmg"),
        },
        UpdatePlatform::Windows => match (install, arch) {
            (InstallKind::WindowsNsis(_), Arch::Aarch64) => find_suffix("_arm64-setup.exe"),
            (InstallKind::WindowsNsis(_), Arch::X86_64) => find_suffix("_x64-setup.exe"),
            (InstallKind::WindowsNsis(_), Arch::Other) => None,
            (_, Arch::Aarch64) => find_suffix("_arm64-setup.exe")
                .or_else(|| find_suffix("_x64-setup.exe"))
                .or_else(|| find_suffix(".msi")),
            (_, Arch::X86_64 | Arch::Other) => {
                find_suffix("_x64-setup.exe").or_else(|| find_suffix(".msi"))
            }
        },
        UpdatePlatform::Linux => match (install, arch) {
            (InstallKind::LinuxAppImage(_), Arch::X86_64) => find_suffix("_x86_64.appimage"),
            (InstallKind::LinuxAppImage(_), Arch::Aarch64) => find_suffix("_aarch64.appimage"),
            (InstallKind::LinuxAppImage(_), Arch::Other) => None,
            (_, Arch::Aarch64) => {
                find_suffix("_aarch64.appimage").or_else(|| find_suffix("_arm64.deb"))
            }
            (_, Arch::X86_64 | Arch::Other) => {
                find_suffix("_x86_64.appimage").or_else(|| find_suffix("_amd64.deb"))
            }
        },
    }
}
