//! Repository gates, run as `cargo xtask <command>`.

use std::path::PathBuf;
use std::process::ExitCode;

mod assets;
mod gates;
mod layers;
mod parity;
mod trapped;
mod unsafety;
mod wasm;

type Command = fn(&[String]) -> Result<(), String>;

const COMMANDS: &[(&str, &str, Command)] = &[
    (
        "layers",
        "Enforce the crate layering in docs/architecture.md",
        layers::run,
    ),
    (
        "parity",
        "Validate parity/mareader-features.toml; report progress (--list STATUS)",
        parity::run,
    ),
    (
        "assets",
        "Check every file in assets/ against ATTRIBUTION.toml",
        assets::run,
    ),
    (
        "unsafe",
        "Keep `unsafe` to the files AGENTS.md allows, each audited by the owner",
        unsafety::run,
    ),
    (
        "comments",
        "Comment rule on Rust files changed from main (tools/comment_check.py)",
        gates::comments,
    ),
    (
        "wasm",
        "Check L0-L5, the front end and the web app for wasm32-unknown-unknown",
        wasm::run,
    ),
    (
        "trapped",
        "Fail when a toolkit-free file hides in crates/ui-egui beyond the allowlist",
        trapped::run,
    ),
    (
        "ci",
        "fmt, clippy, tests, layers, wasm, trapped, parity, assets, unsafe; stops at the first failure",
        gates::ci,
    ),
];

/// The workspace root: xtask lives one level below it.
pub fn root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent().map(PathBuf::from).unwrap_or(here)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(name) = args.first() else {
        help();
        return ExitCode::FAILURE;
    };
    if matches!(name.as_str(), "help" | "-h" | "--help") {
        help();
        return ExitCode::SUCCESS;
    }
    let Some((_, _, command)) = COMMANDS.iter().find(|(n, _, _)| n == name) else {
        eprintln!("xtask: unknown command `{name}`\n");
        help();
        return ExitCode::FAILURE;
    };
    match command(args.get(1..).unwrap_or_default()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {name}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn help() {
    eprintln!("usage: cargo xtask <command>\n\ncommands:");
    for (name, about, _) in COMMANDS {
        eprintln!("  {name:<10} {about}");
    }
}
