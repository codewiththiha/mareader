//! `cargo xtask ci` and `comments`: the gates a commit must pass.

use std::process::Command;

fn run_step(name: &str, program: &str, args: &[&str]) -> Result<(), String> {
    println!("== {name}");
    let status = Command::new(program)
        .args(args)
        .current_dir(crate::root())
        .status()
        .map_err(|e| format!("{name}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} failed"))
    }
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

/// The workspace sweep CI runs; locally prefer `-p <crate> -j 2` gates.
pub fn ci(_: &[String]) -> Result<(), String> {
    let cargo = cargo();
    run_step("fmt", &cargo, &["fmt", "--all", "--check"])?;
    run_step(
        "clippy",
        &cargo,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run_step("test", &cargo, &["test", "--workspace"])?;
    crate::layers::run(&[])?;
    crate::wasm::run(&[])?;
    crate::trapped::run(&[])?;
    crate::parity::run(&[])?;
    crate::assets::run(&[])?;
    crate::unsafety::run(&[])?;
    comments(&[])
}

/// `tools/comment_check.py` on the Rust files that differ from `base`.
pub fn comments(args: &[String]) -> Result<(), String> {
    let base = args.first().map_or("origin/main", String::as_str);
    let out = Command::new("git")
        .args([
            "diff",
            "--name-only",
            "--diff-filter=AM",
            base,
            "--",
            "*.rs",
        ])
        .current_dir(crate::root())
        .output()
        .map_err(|e| format!("git diff: {e}"))?;
    let changed: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    if !out.status.success() || changed.is_empty() {
        println!("comments: no changed Rust files against {base}");
        return Ok(());
    }
    let mut args = vec!["tools/comment_check.py".to_owned()];
    args.extend(changed);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run_step("comments", "python3", &args)
}
