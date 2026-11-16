# Release notes and in-app updates

Every tagged release (`v<version>`) reads its GitHub Release body from
`docs/releases/v<version>.md` (`.github/workflows/installers.yml`). That file
serves two audiences:

1. **Human readers on GitHub** — the full release note below the summary.
2. **Automated consumers** — the marketing site's release page and Mareader's
   in-app update modal both parse the top `## Summary` block directly from the
   GitHub Releases API `body`.

Because machines render `## Summary` straight to users, follow the structure
below on every release note you write or edit.

## Required `## Summary` format

Place a `## Summary` section near the top of `docs/releases/v<version>.md`,
wrapped in HTML comment markers so parsers find an unambiguous slice even if
later sections share headings:

```markdown
# Mareader 0.0.6

Short one-sentence overview for GitHub readers.

<!-- release-summary:start -->
## Summary

### New
- Built-in update checks with silent background updates on Windows and Linux.
- Reader-wide EPUB image theming across reflow and fixed-layout pages.

### Fixes
- Preserved icon tints during rapid hover and click animations.
- Restored dark ink contrast in Dim mode.
<!-- release-summary:end -->

## Details
...
```

### Strict rules for `## Summary`

- **Only two subsections are allowed:** `### New` and `### Fixes` (in that
  order). Do not invent extra subsections inside `## Summary` (no `Changed`,
  `Internal`, `Chore`, `Refactor`, or `Performance` headings).
- **Omit empty subsections.** If a release has only fixes, include only
  `### Fixes` and omit `### New`. If a release has only new features, include
  only `### New` and omit `### Fixes`. Never write `None`, `N/A`, or an empty
  heading.
- **Short, user-facing bullets only.** Use plain `- ` single-line bullets,
  1–5 items per subsection, each ≤ 120 characters, describing what changed for
  a reader — never internal refactors, crate names, file paths, PR/issue
  numbers, commit SHAs, or benchmark dumps. Put technical detail below
  `<!-- release-summary:end -->`.
- **Plain prose inside bullets.** Keep inline formatting minimal (no nested
  lists, tables, images, or fenced code blocks inside `## Summary`).
- **Optional download/page link override.** By default, Mareader links directly
  to the platform's GitHub Release asset (`browser_download_url`, falling back
  to the release's `html_url`). If a release should open a dedicated marketing
  or download page instead, place a single comment above `## Summary`:
  `<!-- release-url: https://mareader.app/download -->` (HTTPS only; untrusted
  schemes or domains are ignored).

### Examples

**Fixes-only release:**

```markdown
<!-- release-summary:start -->
## Summary

### Fixes
- Fixed EPUB thumbnails staying dark in Light mode.
- Fixed rapid toolbar clicks stalling hover icons.
<!-- release-summary:end -->
```

**Features-only release:**

```markdown
<!-- release-summary:start -->
## Summary

### New
- Added an in-app update prompt and Settings › Updates controls.
<!-- release-summary:end -->
```

Older releases (`v0.0.1`–`v0.0.5`) predate `## Summary`; the in-app parser
falls back gracefully to lead items under `## What's new` / `## Fixes` so
existing releases still render cleanly.

## How in-app updates work across platforms

Every release publishes installers alongside `SHA256SUMS` and GitHub's native
asset SHA-256 digests. Mareader compares SemVer numerically (`0.0.10 > 0.0.9`),
ignores drafts and pre-releases, and never installs a binary whose SHA-256
checksum cannot be verified over HTTPS.

| Platform | Default behavior | Paid cert required? | Notes |
| --- | --- | --- | --- |
| **macOS** | Startup in-app modal with **Download update** link | No (notify only) | Without an Apple Developer ID + notarization, replacing an ad-hoc `.app` breaks TCC permissions on every update and fails under App Translocation. Opening the GitHub `.dmg` / release link is the safe path. |
| **Windows** | Silent background download + NSIS `/S` update on exit (or **Update & Restart** modal when Auto-update is off) | No (with caveats) | Files fetched directly over HTTPS inside the app do not carry browser Mark-of-the-Web (`Zone.Identifier`), and per-user NSIS installs under `%LOCALAPPDATA%\Mareader` update without UAC. Caveats: first browser install hits SmartScreen, Windows 11 Smart App Control (if enforced) blocks any unsigned `.exe`, and per-machine MSI installs under `Program Files` fall back to the download modal. |
| **Linux** | Silent background download + atomic `rename(2)` over the AppImage (or **Update & Restart** modal when Auto-update is off) | No | AppImage (`APPIMAGE` env var in a user-writable folder) updates atomically on the same filesystem. Package-managed installs (`.deb`, pacman, `/usr/bin`) are not overwritten and fall back to the download modal. |

### User settings (Settings › Updates)

- **macOS:** *Notify about updates* (`updates.check_on_startup`, default on)
  plus a manual **Check for updates** button.
- **Windows & Linux:**
  - *Auto-update* (`updates.auto_update`, default on): when enabled on a
    writable install, downloads and stages updates silently without interrupting
    the user, then applies on exit.
  - *Check for updates automatically* (`updates.check_on_startup`, default on):
    when Auto-update is off (or the install is package-managed), checks on
    startup and opens the in-app modal whenever a newer release exists. When
    both are off, no startup request is made and users can still trigger a
    manual **Check for updates**.

## Testing updates locally from the terminal

Launch Mareader with mock flags to preview the startup update modal, Settings ›
Updates states, or a custom release note file without publishing a GitHub
Release (mock launches run isolated from any existing Mareader instance):

```sh
# Preview the startup modal on your current platform (default mock v99.0.0)
mareader --mock-update

# Preview a specific platform and mode (notify | install | staged)
mareader --mock-update=0.0.9 --mock-update-platform mac --mock-update-mode notify
mareader --mock-update=0.0.9 --mock-update-platform windows --mock-update-mode install
mareader --mock-update=0.0.9 --mock-update-platform linux --mock-update-mode staged

# Parse and preview a local release-notes draft
mareader --mock-update --mock-update-file docs/releases/v0.0.6.md

# Or via environment variable (accepts notify | install | staged | <version>)
MAREADER_MOCK_UPDATE=notify mareader
```
