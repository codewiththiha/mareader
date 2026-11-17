# Packaging and opening files from the system

Mareader opens a PDF from the system file manager (Finder, Explorer, a
Linux file manager) the way mareader did: as the registered PDF viewer,
reusing the window that is already running.

## Installers

[cargo-packager](https://github.com/crabnebula-dev/cargo-packager) reads
`[package.metadata.packager]` in `apps/mareader/Cargo.toml`:

```sh
cargo install cargo-packager --locked
cd apps/mareader && cargo packager --release
```

Each platform gets its default formats: `.app` and `.dmg` on macOS, NSIS
and MSI on Windows, `.deb`, AppImage and pacman on Linux. Each one
registers `.pdf` (`application/pdf`) with Mareader as a viewer, as in
mareader's `tauri.conf.json`. EPUB joins the list when it can be read (M6).

The icons in `apps/mareader/icons` come from `logo.svg`: an onyx
squircle lit warmly from above, holding an ivory open book whose page
tops draw an M, with gilt edges and a gold ribbon. The `.icns` keeps
Apple's 824-on-1024 grid with a soft shadow under the tile; the PNGs and
`.ico` crop to the tile, as Windows and Linux draw icons edge to edge.
`python3 tools/icons.py` renders every size from the SVG, each drawn on
its own so the small ones stay sharp (it needs `cairosvg` and Pillow).

**Windows reads the icon from the `.exe`.** Explorer, the Start menu and
desktop shortcuts, a pinned taskbar button and Installed apps all take it
from the executable's resources; the window's icon only exists while it
runs. Up to v0.0.3 the exe carried none, so everything but the running
window showed Windows' generic icon, and NSIS shortcuts, which point at
the exe, did too. `apps/mareader/build.rs` now turns `icon.ico` into a
resource file in plain Rust, every size in it, and the MSVC linker builds
it in; no resource compiler is involved. The NSIS setup program shows the
icon too (`installer-icon`). `tools/exe-icons.ps1` fails when an exe has
no icon, run by CI on each Windows build and on the installed copy.

## How a file arrives

| Where | How it arrives | Code |
| --- | --- | --- |
| Command line, Windows, Linux | the path is an argument | `main.rs` |
| macOS Finder and Dock | an "open documents" Apple event | `open_files.rs` |
| Any, while Mareader runs | the new launch forwards it | `single_instance.rs` |

Every route posts to one `Inbox`. The window reads it each frame and
opens the newest path in the focused pane, as mareader's `queue_pending`
did. A file given at launch takes the place of resuming the last book.

### macOS

AppKit delivers Finder's files as a `kAEOpenDocuments` Apple event, not
as arguments. winit installs its own `NSApplicationDelegate`, so
Mareader can't add `application:openURLs:` to it. Instead, `open_files.rs`
watches for `NSApplicationWillFinishLaunchingNotification` and then
installs its own handler with `NSAppleEventManager`. That is the moment
Apple documents for replacing AppKit's default handlers, and it comes
before the launch's own event, so a double-clicked file opens on the
first frame. With the app running, macOS sends the event to it directly.

This is the workspace's second `unsafe` module, approved by the owner and
listed in `xtask/src/unsafety.rs`. The `open-files` Cargo feature (on by
default) turns it off; without it, only the command line opens files.

### One window

On Windows and Linux, opening a file starts a new process. As mareader's
single-instance plugin did, the first launch listens on loopback (port 0)
and writes the port and a random token to `instance` in eframe's data
folder. A later launch sends the token and its absolute paths, waits for
`ok`, and exits; the first window opens the file and comes forward.

If the recorded reader is gone, the connection is refused at once. If the
port belongs to something else now, there is no `ok` within half a
second. Either way, the new launch runs as the primary and records itself.

## Checking it on a Mac

1. `cd apps/mareader && cargo packager --release --formats app`.
2. Move `Mareader.app` to `/Applications` and open it once.
3. In Finder, choose a PDF › Get Info › Open with: Mareader, then
   double-click the PDF. It opens on the first frame.
4. With Mareader running, double-click another PDF. It replaces the book
   in the focused pane, in the same window.
5. `open -a Mareader book.pdf` does the same from a terminal.

The owner ran these on a Mac on 2026-10-17; all passed.

## The Apple Intelligence helper

On a Mac, Explain can use the on-device model through `FMBridge`, a small
Swift program from [fm-bridge](https://github.com/codewiththiha/fm-bridge).
Only macOS 26 with Xcode 26 can build it, so it is not part of
`cargo build`:

```sh
SIGN_IDENTITY="Developer ID Application: …" tools/fm-helper.sh
cd apps/mareader && cargo packager --release
```

- **Which version.** The script builds the fm-bridge commit that
  `Cargo.lock` pins, so the helper and the crate speak the same protocol.
  Without `SIGN_IDENTITY` it signs ad hoc, enough for this Mac only.
- **Where it goes.** The script writes `apps/mareader/helper/FMBridge`
  (ignored by git). The packager's `resources` entry copies it to
  `Mareader.app/Contents/Resources/`, where the app looks for it. Without
  the file, packaging still works and Settings › AI says Apple
  Intelligence is not available.
- **While developing.** Debug builds also honour
  `FM_BRIDGE_BIN=/path/to/FMBridge`, else the helper the script wrote in
  `apps/mareader/helper/`, so `cargo run` can use it without packaging.
  When none is found, Settings › AI says so under Apple Intelligence.
- **Other platforms.** The helper is never built or shipped; the AI tab
  hides Apple Intelligence and offers the servers instead.

## Installers from CI

`.github/workflows/installers.yml` builds every installer on its own
platform, then installs it, launches it with a book, and removes it:

| Runner | Installers | Checked |
| --- | --- | --- |
| `ubuntu-22.04` (x86_64) | `.deb`, AppImage, pacman (`.tar.gz` and `PKGBUILD`) | the `.deb`, `PKGBUILD` and binary match x86_64; the desktop entry's four types and `%F`; `apt` installs it with its dependencies; every linked library resolves; both the `.deb` and the AppImage launch; `apt` removes it |
| `ubuntu-22.04-arm` (ARM64) | `.deb`, AppImage, pacman (`.tar.gz` and `PKGBUILD`) | the same checks for `aarch64` / `arm64`; `tools/merge-pkgbuild.py` combines both Linux runners' `PKGBUILD` files into one multi-arch `PKGBUILD` on release |
| `macos-26` (Apple Silicon) | `.app` in a `.dmg` | `Info.plist` lists the four types and macOS 11; the signature verifies; the binary is arm64 and links only system libraries; the Apple Intelligence helper is inside; the `.dmg`'s checksums hold, an ad hoc build's `.dmg` is unsigned, and it mounts with the signed app and an Applications link; it launches |
| `macos-26-intel` | `.app` in a `.dmg` | the same, x86_64, without the helper |
| `windows-latest` (x64) | NSIS `-setup.exe`, MSI | each installs silently, verifies x64 PE target and icon resource, registers `.pdf`, `.epub`, `.txt` and `.md`, launches and uninstalls silently |
| `windows-11-arm` (ARM64) | NSIS `-setup.exe` | installs silently, verifies ARM64 PE target and icon resource, registers the four types, launches and uninstalls silently (WiX 3.11 in `cargo-packager 0.11.8` does not build ARM64 `.msi`) |

"Launches" means the app is still running 12 s after opening an EPUB, on
the runner's software GPU (lavapipe under Xvfb, WARP) or its Metal device.
The run's summary lists every check, and each platform's report (the
checks, logs, a screenshot where the platform allows one, and the
installers' sizes and SHA-256) is a `checks-<platform>` artifact. The
installers are `installers-<platform>` artifacts.

How to run it:

- **On demand:** `python3 tools/ci.py installers` dispatches the workflow
  on a pushed commit, waits for all six platforms, and prints their
  checks (`--download` also fetches the installers).
- **A packaging change:** push it to a `release/<slug>` branch, which
  builds and checks the installers without a tag.
- **A release:** push a `v<version>` tag that matches `Cargo.toml`. The
  same checks run, then a draft GitHub release collects the installers
  with `SHA256SUMS`, for the owner to publish. Its notes come from
  `docs/releases/v<version>.md` when that file exists (follow the structured
  `## Summary` format in `docs/release-notes.md` so the website and in-app
  updater can parse highlights); otherwise GitHub lists the changes.
- **On your own machine:** after `cargo packager --release`,
  `tools/check-installers.sh target/release` runs the same checks for
  your platform (it installs and removes the app). Without a GPU, a
  launch that dies is a warning; `SMOKE=required` makes it a failure.

What each platform registers: `.pdf`, `.epub`, `.txt` and `.text`, `.md`,
`.markdown` and `.mdown`, as a viewer. Text and Markdown open through the
EPUB path, and mareader registered the same list. On Windows, a user's own
default app choice still wins; Mareader appears under "Open with".

Signing:

- **macOS** is signed ad hoc (`signing-identity = "-"`). A downloaded
  copy asks once: open it, then choose Open Anyway in System Settings ›
  Privacy & Security. Unsigned, Apple Silicon would call it damaged. The
  `.dmg` around it is left unsigned (`tools/unsign-dmg.py` strips the ad
  hoc signature cargo-packager adds): macOS checks a signed disk image as
  it opens, and refuses an ad hoc one before the app is reached. With
  the repository secrets `APPLE_SIGNING_IDENTITY`, `APPLE_CERTIFICATE`
  (base64 `.p12`) and `APPLE_CERTIFICATE_PASSWORD`, CI signs with that
  Developer ID instead, and `APPLE_ID`, `APPLE_PASSWORD` (an app-specific
  password) and `APPLE_TEAM_ID` add notarization.
- **Windows** installers are unsigned, so SmartScreen warns on first run
  until a code-signing certificate is added.
- **Linux** packages are not signed; `SHA256SUMS` in the release lets you
  check a download.

The Linux `.deb` declares what Mareader loads at run time, which `dpkg`
can't see in the binary: fontconfig, xkbcommon (with its X11 part), and
Vulkan or EGL. The pacman package's `PKGBUILD` names the Arch packages
for the same (with `libglvnd` for EGL). The AppImage expects them from the
system. It is built
on Ubuntu 22.04, so it needs glibc 2.35 or newer.
