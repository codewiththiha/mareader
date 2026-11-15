//! Keyboard shortcuts. One table drives the handler, tooltips and cheat sheet.

use egui::{Context, Key, KeyboardShortcut, ModifierNames, Modifiers};

use crate::platform::Platform;
use crate::ui_command::Allows;
use crate::ui_command::{Commands, Sent, UiCommand, WindowCommand};
use mareader_engine::command::Command;
use mareader_engine::documents::Placement;
use mareader_engine::pane::{Arrow, ViewMode};
use mareader_engine::pane::{Fit, Step, ZoomCommand};
use mareader_engine::route::Route;

pub struct Binding {
    pub label: &'static str,
    pub shortcut: KeyboardShortcut,
    pub command: Sent,
}

/// Every bound shortcut; features add theirs here as they land.
pub fn bindings(platform: Platform) -> [Binding; 39] {
    // Each desktop's own full-screen key.
    let fullscreen = match platform {
        Platform::Mac => KeyboardShortcut::new(Modifiers::CTRL | Modifiers::MAC_CMD, Key::F),
        Platform::Windows | Platform::Gnome => KeyboardShortcut::new(Modifiers::NONE, Key::F11),
    };
    [
        Binding {
            label: "Open a file",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::O),
            command: Sent::Ui(UiCommand::ShowOpenDialog(Placement::Into)),
        },
        zoom("Zoom in", Modifiers::COMMAND, Key::Plus, ZOOM_IN),
        zoom("Zoom in", Modifiers::COMMAND, Key::Equals, ZOOM_IN),
        zoom("Zoom out", Modifiers::COMMAND, Key::Minus, ZOOM_OUT),
        zoom("Fit width", Modifiers::COMMAND, Key::Num0, FIT_WIDTH),
        // mareader's bare keys too, while nothing takes typing.
        zoom("Zoom in", Modifiers::NONE, Key::Plus, ZOOM_IN),
        zoom("Zoom in", Modifiers::NONE, Key::Equals, ZOOM_IN),
        zoom("Zoom out", Modifiers::NONE, Key::Minus, ZOOM_OUT),
        view("Single page", Key::Num1, ViewMode::Single),
        view("Vertical scroll", Key::Num2, ViewMode::Vertical),
        // Arrows and vim's keys; the first of each names it.
        arrow("Previous page", Key::ArrowLeft, Arrow::Left),
        arrow("Next page", Key::ArrowRight, Arrow::Right),
        arrow(UP, Key::ArrowUp, Arrow::Up),
        arrow(DOWN, Key::ArrowDown, Arrow::Down),
        arrow("Previous page", Key::H, Arrow::Left),
        arrow("Next page", Key::L, Arrow::Right),
        arrow(UP, Key::K, Arrow::Up),
        arrow(DOWN, Key::J, Arrow::Down),
        // Shift+Space first: a bare pattern ignores Shift.
        page("Back a screen", Modifiers::NONE, Key::PageUp, false),
        page("Forward a screen", Modifiers::NONE, Key::PageDown, true),
        page("Back a screen", Modifiers::SHIFT, Key::Space, false),
        page("Forward a screen", Modifiers::NONE, Key::Space, true),
        Binding {
            label: "Auto-scroll",
            shortcut: KeyboardShortcut::new(Modifiers::SHIFT, Key::A),
            command: Sent::Engine(Command::AutoScroll),
        },
        Binding {
            label: "First page",
            shortcut: KeyboardShortcut::new(Modifiers::NONE, Key::Home),
            command: Sent::Engine(Command::FirstPage),
        },
        Binding {
            label: "Last page",
            shortcut: KeyboardShortcut::new(Modifiers::NONE, Key::End),
            command: Sent::Engine(Command::LastPage),
        },
        Binding {
            label: "Settings",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma),
            command: Sent::Ui(UiCommand::ShowSettings),
        },
        Binding {
            label: "Keyboard shortcuts",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Slash),
            command: Sent::Ui(UiCommand::ShowShortcuts),
        },
        Binding {
            label: "Full screen",
            shortcut: fullscreen,
            command: Sent::Ui(UiCommand::Window(WindowCommand::ToggleFullscreen)),
        },
        // Before Find, whose pattern would take Shift too.
        Binding {
            label: "Distraction-free",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::F),
            command: Sent::Ui(UiCommand::Chromeless),
        },
        // After full screen, which macOS puts on Ctrl+Cmd+F.
        Binding {
            label: "Find",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::F),
            command: Sent::Ui(UiCommand::ShowFind),
        },
        // Shift first again: the bare key would take Shift+F3 too.
        step("Previous match", Modifiers::SHIFT, Key::F3, false),
        step("Next match", Modifiers::NONE, Key::F3, true),
        step(
            "Previous match",
            Modifiers::COMMAND | Modifiers::SHIFT,
            Key::G,
            false,
        ),
        step("Next match", Modifiers::COMMAND, Key::G, true),
        // Before Bookmark, whose pattern would take Shift too.
        Binding {
            label: "Dictionary",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::D),
            command: Sent::Ui(UiCommand::ShowDictionary),
        },
        Binding {
            label: "Bookmark this page",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::D),
            command: Sent::Engine(Command::Bookmark(None)),
        },
        // Where browsers put their own read-aloud.
        Binding {
            label: "Read aloud, or pause",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::U),
            command: Sent::Engine(Command::ToggleReadAloud),
        },
        Binding {
            label: "Previous pane",
            shortcut: KeyboardShortcut::new(Modifiers::SHIFT, Key::F6),
            command: Sent::Engine(Command::CyclePane { forward: false }),
        },
        Binding {
            label: "Next pane",
            shortcut: KeyboardShortcut::new(Modifiers::NONE, Key::F6),
            command: Sent::Engine(Command::CyclePane { forward: true }),
        },
    ]
}

/// Along a column the up and down arrows scroll; elsewhere they turn.
const UP: &str = "Scroll up, or previous page";
const DOWN: &str = "Scroll down, or next page";

const ZOOM_IN: ZoomCommand = ZoomCommand::Step(Step::In);
const ZOOM_OUT: ZoomCommand = ZoomCommand::Step(Step::Out);
const FIT_WIDTH: ZoomCommand = ZoomCommand::Fit(Fit::Width);

fn zoom(label: &'static str, modifiers: Modifiers, key: Key, z: ZoomCommand) -> Binding {
    Binding {
        label,
        shortcut: KeyboardShortcut::new(modifiers, key),
        command: Sent::Engine(Command::Zoom(z)),
    }
}

fn view(label: &'static str, key: Key, mode: ViewMode) -> Binding {
    Binding {
        label,
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, key),
        command: Sent::Engine(Command::View(mode)),
    }
}

fn arrow(label: &'static str, key: Key, arrow: Arrow) -> Binding {
    Binding {
        label,
        shortcut: KeyboardShortcut::new(Modifiers::NONE, key),
        command: Sent::Engine(Command::Arrow(arrow)),
    }
}

fn step(label: &'static str, modifiers: Modifiers, key: Key, forward: bool) -> Binding {
    Binding {
        label,
        shortcut: KeyboardShortcut::new(modifiers, key),
        command: Sent::Ui(UiCommand::FindStep { forward }),
    }
}

/// Page keys: nearly a screen along a strip, a turn when paged.
fn page(label: &'static str, modifiers: Modifiers, key: Key, forward: bool) -> Binding {
    Binding {
        label,
        shortcut: KeyboardShortcut::new(modifiers, key),
        command: Sent::Engine(Command::Page { forward }),
    }
}

/// Keys that a focused text field types or moves its cursor by.
fn types(shortcut: &KeyboardShortcut) -> bool {
    use Key::{
        A, ArrowDown, ArrowLeft, ArrowRight, ArrowUp, End, Equals, H, Home, J, K, L, Minus,
        PageDown, PageUp, Plus, Space,
    };
    let bare = shortcut.modifiers.is_none() || shortcut.modifiers == Modifiers::SHIFT;
    bare && matches!(
        shortcut.logical_key,
        Plus | Equals
            | A
            | Minus
            | ArrowLeft
            | ArrowRight
            | ArrowUp
            | ArrowDown
            | H
            | J
            | K
            | L
            | Space
            | PageUp
            | PageDown
            | Home
            | End
    )
}

/// Arrows move egui's focus and Space presses buttons, while one has it.
fn steers(b: &Binding) -> bool {
    matches!(b.command, Sent::Engine(Command::Arrow(_))) || b.shortcut.logical_key == Key::Space
}

/// Whether a key that sends `arrow` is still held down.
pub fn held(input: &egui::InputState, arrow: Arrow) -> bool {
    let keys = match arrow {
        Arrow::Left => [Key::ArrowLeft, Key::H],
        Arrow::Right => [Key::ArrowRight, Key::L],
        Arrow::Up => [Key::ArrowUp, Key::K],
        Arrow::Down => [Key::ArrowDown, Key::J],
    };
    keys.iter().any(|k| input.key_down(*k))
}

/// Keys handled elsewhere, listed so the reference is complete.
const LISTED: [(&str, &str); 4] = [
    ("Close a popover, window or sidebar", "Esc"),
    ("Move between controls", "Tab  Shift+Tab"),
    ("Press the focused control, link or mark", "Enter  Space"),
    ("Open the focused item's menu", "Shift+F10"),
];

/// Consumes this frame's shortcuts and queues their commands.
pub fn read(ctx: &Context, platform: Platform, route: Route, out: &mut Commands) {
    // An open picture takes the keys it knows; the rest wait.
    if crate::image_viewer::is_open(ctx) {
        return;
    }
    // Behind the cheat sheet only its own key, which shuts it, acts.
    let sheet = crate::shortcuts::is_open(ctx);
    // A focused button wants Space and the arrows, not every key.
    let typing = ctx.text_edit_focused();
    let focused = ctx.memory(|m| m.focused().is_some()) || egui::Popup::is_any_open(ctx);
    // Keys the route ignores stay unconsumed, for the page's own widgets.
    for b in bindings(platform)
        .into_iter()
        .filter(|b| route.allows(&b.command))
        .filter(|b| !sheet || b.command == Sent::Ui(UiCommand::ShowShortcuts))
    {
        if (typing && types(&b.shortcut)) || (focused && steers(&b)) {
            continue;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&b.shortcut)) {
            out.push(b.command);
        }
    }
}

/// Modifier words with key symbols: `Ctrl+,`, not `Ctrl+Comma`.
const WORDS: ModifierNames<'static> = ModifierNames {
    is_short: true,
    concat: "+",
    ..ModifierNames::NAMES
};

/// `⌘,` on macOS, `Ctrl+,` elsewhere.
pub fn text(platform: Platform, shortcut: &KeyboardShortcut) -> String {
    let text = match platform {
        Platform::Mac => shortcut.format(&ModifierNames::SYMBOLS, true),
        Platform::Windows | Platform::Gnome => shortcut.format(&WORDS, false),
    };
    // egui's arrow glyphs are media triangles, tiny beside the letters.
    let arrows = [('⏴', '←'), ('⏵', '→'), ('⏶', '↑'), ('⏷', '↓')];
    arrows
        .iter()
        .fold(text, |t, (from, to)| t.replace(*from, &to.to_string()))
}

/// The shortcut that sends `command`, for its tooltip or menu row.
pub fn hint(platform: Platform, command: &Sent) -> Option<String> {
    bindings(platform)
        .iter()
        .find(|b| b.command == *command)
        .map(|b| text(platform, &b.shortcut))
}

/// One action on the cheat sheet, with every key that sends it.
pub struct Entry {
    pub label: &'static str,
    pub keys: Vec<String>,
}

/// A named group of the cheat sheet.
pub struct Section {
    pub name: &'static str,
    pub entries: Vec<Entry>,
}

/// The cheat sheet's groups, in reading order.
const SECTIONS: [&str; 6] = [
    "Moving around",
    "View",
    "Find",
    "General",
    "Panes",
    CONTROLS,
];
const CONTROLS: &str = "Controls";

fn section_of(command: &Sent) -> &'static str {
    use mareader_engine::command::Command as I;
    match command {
        Sent::Engine(I::Arrow(_) | I::Page { .. } | I::FirstPage | I::LastPage) => "Moving around",
        Sent::Engine(I::Zoom(_) | I::View(_) | I::AutoScroll) => "View",
        Sent::Ui(UiCommand::Chromeless | UiCommand::Window(_)) => "View",
        Sent::Ui(UiCommand::ShowFind | UiCommand::FindStep { .. }) => "Find",
        Sent::Engine(I::CyclePane { .. }) => "Panes",
        _ => "General",
    }
}

/// The cheat sheet: each action once, under its group, with all its keys.
pub fn sheet(platform: Platform) -> Vec<Section> {
    let mut sections: Vec<Section> = SECTIONS
        .iter()
        .map(|&name| Section {
            name,
            entries: Vec::new(),
        })
        .collect();
    for b in bindings(platform) {
        // `=` is the unshifted `+`: the same key, listed once.
        if b.shortcut.logical_key == Key::Equals {
            continue;
        }
        let name = section_of(&b.command);
        let Some(section) = sections.iter_mut().find(|s| s.name == name) else {
            continue;
        };
        let keys = text(platform, &b.shortcut);
        match section.entries.iter_mut().find(|e| e.label == b.label) {
            Some(e) if !e.keys.contains(&keys) => e.keys.push(keys),
            Some(_) => {}
            None => section.entries.push(Entry {
                label: b.label,
                keys: vec![keys],
            }),
        }
    }
    if let Some(controls) = sections.iter_mut().find(|s| s.name == CONTROLS) {
        controls.entries = LISTED
            .iter()
            .map(|&(label, keys)| Entry {
                label,
                keys: keys.split("  ").map(str::to_owned).collect(),
            })
            .collect();
    }
    sections.retain(|s| !s.entries.is_empty());
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_read_in_each_desktops_words() {
        let settings = Sent::Ui(UiCommand::ShowSettings);
        assert_eq!(hint(Platform::Mac, &settings).as_deref(), Some("⌘,"));
        assert_eq!(
            hint(Platform::Windows, &settings).as_deref(),
            Some("Ctrl+,")
        );
        let full = Sent::Ui(UiCommand::Window(WindowCommand::ToggleFullscreen));
        assert_eq!(hint(Platform::Gnome, &full).as_deref(), Some("F11"));
        assert_eq!(hint(Platform::Mac, &full).as_deref(), Some("⌃⌘F"));
        assert_eq!(hint(Platform::Mac, &Sent::Ui(UiCommand::ShowLibrary)), None);
        let open = Sent::Ui(UiCommand::ShowOpenDialog(Placement::Into));
        assert_eq!(hint(Platform::Gnome, &open).as_deref(), Some("Ctrl+O"));
        let zoom_in = Sent::Engine(Command::Zoom(ZOOM_IN));
        assert_eq!(hint(Platform::Mac, &zoom_in).as_deref(), Some("⌘+"));
        let fit = Sent::Engine(Command::Zoom(FIT_WIDTH));
        assert_eq!(hint(Platform::Windows, &fit).as_deref(), Some("Ctrl+0"));
    }

    #[test]
    fn the_sheet_lists_each_action_once_with_all_its_keys() {
        let sheet = sheet(Platform::Mac);
        let entries = || sheet.iter().flat_map(|s| &s.entries);
        let zoom_in: Vec<_> = entries().filter(|e| e.label == "Zoom in").collect();
        assert_eq!(zoom_in.len(), 1);
        assert_eq!(zoom_in[0].keys, ["⌘+", "+"], "= is listed as +");
        let next = entries().find(|e| e.label == "Next page");
        assert!(next.is_some_and(|e| e.keys.len() == 2), "the arrow and L");
        let names: Vec<_> = sheet.iter().map(|s| s.name).collect();
        assert_eq!(names, SECTIONS);
        let previous = entries().find(|e| e.label == "Previous page");
        assert!(
            previous.is_some_and(|e| e.keys == ["←", "H"]),
            "real arrows"
        );
        let tab = entries().find(|e| e.label == "Move between controls");
        assert!(tab.is_some_and(|e| e.keys == ["Tab", "Shift+Tab"]));
        let own = hint(Platform::Gnome, &Sent::Ui(UiCommand::ShowShortcuts));
        assert_eq!(own.as_deref(), Some("Ctrl+/"));
        let dict = entries().find(|e| e.label == "Dictionary");
        assert!(dict.is_some_and(|e| e.keys == ["⇧⌘D"]), "listed once");
    }
}
