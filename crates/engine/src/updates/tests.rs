#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;
use std::sync::Arc;

use mareader_settings::UpdateSettings;

use crate::host::Fake;
use crate::session::Waker;

use super::github::{
    Arch, parse_release_json, parse_sha256_digest, parse_sha256sums, validate_https_url,
};
use super::notes::{is_newer, parse_release_url, parse_summary};
use super::{
    Action, InstallKind, MockMode, MockUpdate, State, Status, UpdatePlatform, should_auto_check,
};

fn noop_waker() -> Waker {
    Arc::new(|| {})
}

#[test]
fn semver_compares_numerically_and_ignores_invalid() {
    assert!(is_newer("0.0.10", "0.0.9"));
    assert!(is_newer("v0.1.0", "0.0.99"));
    assert!(is_newer("0.0.6", "0.0.6-rc.1"));
    assert!(!is_newer("0.0.5", "0.0.5"));
    assert!(!is_newer("0.0.4", "0.0.5"));
    assert!(!is_newer("not-a-version", "0.0.5"));
}

#[test]
fn parses_structured_summary_and_omits_empty_sections() {
    let both = "\
# Mareader 0.0.6

<!-- release-url: https://mareader.app/download -->
<!-- release-summary:start -->
## Summary

### New
- In-app update modal and Settings › Updates controls.
- Silent updates on Windows and Linux.

### Fixes
- Preserved icon tints during rapid hover animations.
<!-- release-summary:end -->

## Technical notes
- Ignored detail line.
";
    let summary = parse_summary(both);
    assert_eq!(
        summary.added,
        vec![
            "In-app update modal and Settings › Updates controls.",
            "Silent updates on Windows and Linux."
        ]
    );
    assert_eq!(
        summary.fixes,
        vec!["Preserved icon tints during rapid hover animations."]
    );
    assert_eq!(
        parse_release_url(both).as_deref(),
        Some("https://mareader.app/download")
    );

    let fixes_only = "\
<!-- release-summary:start -->
## Summary

### Fixes
- Restored dark ink contrast in Dim mode.
<!-- release-summary:end -->
";
    let parsed = parse_summary(fixes_only);
    assert!(parsed.added.is_empty());
    assert_eq!(
        parsed.fixes,
        vec!["Restored dark ink contrast in Dim mode."]
    );

    let new_only = "\
## Summary

### New
- Added EPUB image theming.

## Details
- Internal details stay out.
";
    let parsed = parse_summary(new_only);
    assert_eq!(parsed.added, vec!["Added EPUB image theming."]);
    assert!(parsed.fixes.is_empty());
}

#[test]
fn parses_legacy_release_notes_as_fallback() {
    let legacy = "\
# Mareader 0.0.5

## What's new

### Side-by-side panes
Long explanation paragraph.

## Fixes

- Text selection works across split panes.
- Toolbar buttons keep their colour on hover.
";
    let summary = parse_summary(legacy);
    assert_eq!(summary.added, vec!["Side-by-side panes"]);
    assert_eq!(summary.fixes.len(), 2);
}

#[test]
fn rejects_untrusted_or_non_https_urls() {
    assert!(validate_https_url(
        "https://github.com/codewiththiha/mareader2/releases/tag/v0.0.6"
    ));
    assert!(validate_https_url("https://mareader.app/download"));
    assert!(!validate_https_url(
        "http://github.com/codewiththiha/mareader2"
    ));
    assert!(!validate_https_url("javascript:alert(1)"));
    assert!(!validate_https_url("https://evil.example.com/mareader.dmg"));
    assert_eq!(
        parse_release_url("<!-- release-url: https://evil.example.com/a -->"),
        None
    );
}

#[test]
fn parses_github_release_assets_and_checksums() {
    let hex = "a".repeat(64);
    let json = format!(
        r#"{{
  "tag_name": "v0.0.6",
  "html_url": "https://github.com/codewiththiha/mareader2/releases/tag/v0.0.6",
  "draft": false,
  "prerelease": false,
  "body": "<!-- release-summary:start -->\n## Summary\n### New\n- Auto update.\n<!-- release-summary:end -->",
  "assets": [
    {{
      "name": "Mareader_0.0.6_aarch64.dmg",
      "browser_download_url": "https://github.com/codewiththiha/mareader2/releases/download/v0.0.6/Mareader_0.0.6_aarch64.dmg",
      "size": 12000,
      "digest": "sha256:{hex}"
    }},
    {{
      "name": "mareader_0.0.6_x64-setup.exe",
      "browser_download_url": "https://github.com/codewiththiha/mareader2/releases/download/v0.0.6/mareader_0.0.6_x64-setup.exe",
      "size": 14000
    }},
    {{
      "name": "mareader_0.0.6_x86_64.AppImage",
      "browser_download_url": "https://github.com/codewiththiha/mareader2/releases/download/v0.0.6/mareader_0.0.6_x86_64.AppImage",
      "size": 15000
    }},
    {{
      "name": "SHA256SUMS",
      "browser_download_url": "https://github.com/codewiththiha/mareader2/releases/download/v0.0.6/SHA256SUMS"
    }}
  ]
}}"#
    );

    let mac = parse_release_json(
        &json,
        UpdatePlatform::Mac,
        Arch::Aarch64,
        &InstallKind::NotifyOnly,
    )
    .unwrap()
    .unwrap();
    assert_eq!(mac.info.version, "0.0.6");
    assert_eq!(
        mac.info.asset_name.as_deref(),
        Some("Mareader_0.0.6_aarch64.dmg")
    );
    assert_eq!(mac.info.sha256.as_deref(), Some(hex.as_str()));

    let win = parse_release_json(
        &json,
        UpdatePlatform::Windows,
        Arch::X86_64,
        &InstallKind::WindowsNsis(PathBuf::from("C:\\App\\mareader.exe")),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        win.info.asset_name.as_deref(),
        Some("mareader_0.0.6_x64-setup.exe")
    );
    assert!(win.sums_url.is_some());

    let linux = parse_release_json(
        &json,
        UpdatePlatform::Linux,
        Arch::X86_64,
        &InstallKind::LinuxAppImage(PathBuf::from("/tmp/Mareader.AppImage")),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        linux.info.asset_name.as_deref(),
        Some("mareader_0.0.6_x86_64.AppImage")
    );

    let sums = format!(
        "{hex}  mareader_0.0.6_x64-setup.exe\n{}  *mareader_0.0.6_x86_64.AppImage\n",
        "b".repeat(64)
    );
    assert_eq!(
        parse_sha256sums(&sums, "mareader_0.0.6_x64-setup.exe").as_deref(),
        Some(hex.as_str())
    );
    assert_eq!(
        parse_sha256sums(&sums, "mareader_0.0.6_x86_64.AppImage").as_deref(),
        Some("b".repeat(64).as_str())
    );
    assert_eq!(parse_sha256_digest(&format!("sha256:{hex}")), Some(hex));
    assert_eq!(parse_sha256_digest("sha256:short"), None);
}

#[test]
fn auto_check_matrix_respects_platform_and_settings() {
    let both_on = UpdateSettings {
        check_on_startup: true,
        auto_update: true,
    };
    let notify_only = UpdateSettings {
        check_on_startup: true,
        auto_update: false,
    };
    let auto_only = UpdateSettings {
        check_on_startup: false,
        auto_update: true,
    };
    let both_off = UpdateSettings {
        check_on_startup: false,
        auto_update: false,
    };

    assert!(should_auto_check(
        UpdatePlatform::Mac,
        &InstallKind::NotifyOnly,
        &both_on
    ));
    assert!(!should_auto_check(
        UpdatePlatform::Mac,
        &InstallKind::NotifyOnly,
        &auto_only
    ));

    let writable = InstallKind::LinuxAppImage(PathBuf::from("/home/u/Mareader.AppImage"));
    assert!(should_auto_check(
        UpdatePlatform::Linux,
        &writable,
        &auto_only
    ));
    assert!(should_auto_check(
        UpdatePlatform::Linux,
        &writable,
        &notify_only
    ));
    assert!(!should_auto_check(
        UpdatePlatform::Linux,
        &writable,
        &both_off
    ));

    assert!(should_auto_check(
        UpdatePlatform::Linux,
        &InstallKind::PackageManaged,
        &notify_only
    ));
    assert!(!should_auto_check(
        UpdatePlatform::Linux,
        &InstallKind::PackageManaged,
        &auto_only
    ));
}

#[test]
fn mock_modes_cover_notify_install_and_silent_staged() {
    let host = Fake::default();
    let waker = noop_waker();
    let settings = UpdateSettings::default();

    let mut state = State::new("0.0.5");
    state.run(
        Action::Mock(MockUpdate {
            version: "0.0.9".into(),
            notes: None,
            url: None,
            platform: Some(UpdatePlatform::Mac),
            mode: Some(MockMode::Notify),
        }),
        &settings,
        &host,
        &waker,
    );
    assert!(state.modal_open());
    assert!(!state.can_self_update());
    state.run(Action::OpenDownload, &settings, &host, &waker);
    assert!(!state.modal_open());
    assert_eq!(host.opened().len(), 1);

    state.run(
        Action::Mock(MockUpdate {
            version: "0.0.9".into(),
            notes: None,
            url: None,
            platform: Some(UpdatePlatform::Windows),
            mode: Some(MockMode::Install),
        }),
        &settings,
        &host,
        &waker,
    );
    assert!(state.modal_open());
    assert!(state.can_self_update());
    state.run(Action::Install, &settings, &host, &waker);
    assert!(matches!(
        state.status(),
        Status::ReadyToRestart { silent: false, .. }
    ));
    let out = state.run(Action::Restart, &settings, &host, &waker);
    assert!(!state.modal_open());
    assert!(out.toast.is_some());

    state.run(
        Action::Mock(MockUpdate {
            version: "0.0.9".into(),
            notes: None,
            url: None,
            platform: Some(UpdatePlatform::Linux),
            mode: Some(MockMode::Staged),
        }),
        &settings,
        &host,
        &waker,
    );
    assert!(!state.modal_open());
    assert!(matches!(
        state.status(),
        Status::ReadyToRestart { silent: true, .. }
    ));
}
