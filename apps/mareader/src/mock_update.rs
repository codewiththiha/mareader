//! CLI flag and environment parsing for `--mock-update*` local update tests.

use std::ffi::OsString;
use std::path::PathBuf;

use mareader_engine::updates::{MockMode, MockUpdate, UpdatePlatform};

/// Parsed `--mock-update*` developer CLI options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MockUpdateArgs {
    pub version: Option<String>,
    pub file: Option<PathBuf>,
    pub platform: Option<UpdatePlatform>,
    pub mode: Option<MockMode>,
}

/// Resolves CLI or `MAREADER_MOCK_UPDATE` env parameters into a [`MockUpdate`].
pub fn resolve(cli: Option<&MockUpdateArgs>) -> Result<Option<MockUpdate>, String> {
    let args = match cli {
        Some(args) => args.clone(),
        None => match std::env::var("MAREADER_MOCK_UPDATE") {
            Ok(raw) if !raw.trim().is_empty() => parse_value(&raw),
            _ => return Ok(None),
        },
    };
    let notes = match args.file {
        Some(path) => Some(
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?,
        ),
        None => None,
    };
    Ok(Some(MockUpdate {
        version: args.version.unwrap_or_else(|| "99.0.0".to_owned()),
        notes,
        url: None,
        platform: args.platform,
        mode: args.mode,
    }))
}

/// Parses one `--mock-update*` flag; returns `true` if handled.
pub fn parse_flag(
    slot: &mut Option<MockUpdateArgs>,
    text: &str,
    iter: &mut impl Iterator<Item = OsString>,
) -> Result<bool, String> {
    if text == "--mock-update" {
        slot.get_or_insert_with(MockUpdateArgs::default);
    } else if let Some(raw) = text.strip_prefix("--mock-update=") {
        let parsed = parse_value(raw);
        let dst = slot.get_or_insert_with(MockUpdateArgs::default);
        dst.version = parsed.version.or(dst.version.take());
        dst.mode = parsed.mode.or(dst.mode);
        dst.platform = parsed.platform.or(dst.platform);
    } else if text == "--mock-update-file" {
        let path = crate::control_server::parse_path_arg(iter, "--mock-update-file")?;
        slot.get_or_insert_with(MockUpdateArgs::default).file = Some(path);
    } else if let Some(raw) = text.strip_prefix("--mock-update-file=") {
        slot.get_or_insert_with(MockUpdateArgs::default).file = Some(PathBuf::from(raw));
    } else if text == "--mock-update-platform" {
        let raw = next_str(iter, "--mock-update-platform")?;
        let p = UpdatePlatform::parse(&raw)
            .ok_or_else(|| format!("invalid --mock-update-platform {raw:?}"))?;
        slot.get_or_insert_with(MockUpdateArgs::default).platform = Some(p);
    } else if let Some(raw) = text.strip_prefix("--mock-update-platform=") {
        let p = UpdatePlatform::parse(raw)
            .ok_or_else(|| format!("invalid --mock-update-platform {raw:?}"))?;
        slot.get_or_insert_with(MockUpdateArgs::default).platform = Some(p);
    } else if text == "--mock-update-mode" {
        let raw = next_str(iter, "--mock-update-mode")?;
        let m =
            MockMode::parse(&raw).ok_or_else(|| format!("invalid --mock-update-mode {raw:?}"))?;
        slot.get_or_insert_with(MockUpdateArgs::default).mode = Some(m);
    } else if let Some(raw) = text.strip_prefix("--mock-update-mode=") {
        let m =
            MockMode::parse(raw).ok_or_else(|| format!("invalid --mock-update-mode {raw:?}"))?;
        slot.get_or_insert_with(MockUpdateArgs::default).mode = Some(m);
    } else {
        return Ok(false);
    }
    Ok(true)
}

fn parse_value(raw: &str) -> MockUpdateArgs {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("true") || trimmed == "1" {
        return MockUpdateArgs::default();
    }
    if let Some(mode) = MockMode::parse(trimmed) {
        return MockUpdateArgs {
            mode: Some(mode),
            ..MockUpdateArgs::default()
        };
    }
    if let Some(platform) = UpdatePlatform::parse(trimmed) {
        return MockUpdateArgs {
            platform: Some(platform),
            ..MockUpdateArgs::default()
        };
    }
    MockUpdateArgs {
        version: Some(trimmed.to_owned()),
        ..MockUpdateArgs::default()
    }
}

fn next_str(iter: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<String, String> {
    let raw = iter
        .next()
        .ok_or_else(|| format!("{flag} requires a value"))?;
    raw.into_string()
        .map_err(|_| format!("{flag} value must be UTF-8"))
}
