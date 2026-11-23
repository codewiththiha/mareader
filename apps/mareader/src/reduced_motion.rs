//! Whether the system asks apps for less motion, read once at launch.

// Each desktop's own tool answers: no `unsafe`. No answer, normal motion.

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Asks off the UI thread, then stores the answer and wakes a frame.
pub fn watch(flag: Arc<AtomicBool>, ctx: egui::Context) {
    let asked = std::thread::Builder::new()
        .name("reduced-motion".into())
        .spawn(move || {
            if ask().is_some_and(|reduced| reduced) {
                flag.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            }
        });
    if let Err(e) = asked {
        eprintln!("mareader: couldn't ask about reduced motion: {e}");
    }
}

/// The system's answer, or none when it can't be read.
fn ask() -> Option<bool> {
    let (program, args) = question()?;
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // No console window flashes up behind the app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let out = command.output().ok()?;
    out.status.success().then_some(())?;
    answer(&String::from_utf8_lossy(&out.stdout))
}

/// The command that reads this desktop's setting.
fn question() -> Option<(&'static str, &'static [&'static str])> {
    if cfg!(target_os = "macos") {
        Some((
            "defaults",
            &["read", "com.apple.universalaccess", "reduceMotion"],
        ))
    } else if cfg!(windows) {
        // Animation effects off also turns this off.
        const KEY: &str = r"HKCU\Control Panel\Desktop\WindowMetrics";
        Some(("reg", &["query", KEY, "/v", "MinAnimate"]))
    } else if cfg!(target_os = "linux") {
        const SCHEMA: &str = "org.gnome.desktop.interface";
        Some(("gsettings", &["get", SCHEMA, "enable-animations"]))
    } else {
        None
    }
}

/// Reads the tool's output for this desktop.
fn answer(out: &str) -> Option<bool> {
    let out = out.trim();
    if cfg!(target_os = "macos") {
        // `defaults` prints 1 when Reduce motion is on.
        Some(out == "1")
    } else if cfg!(windows) {
        // `MinAnimate    REG_SZ    0` when animations are off.
        let value = out.split_whitespace().last()?;
        Some(value == "0")
    } else {
        // GNOME prints whether animations are enabled.
        match out {
            "false" => Some(true),
            "true" => Some(false),
            _ => None,
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::answer;

    #[test]
    fn gnome_animations_off_means_reduced() {
        assert_eq!(answer("false\n"), Some(true));
        assert_eq!(answer("true\n"), Some(false));
        assert_eq!(answer("No such schema"), None);
    }
}
