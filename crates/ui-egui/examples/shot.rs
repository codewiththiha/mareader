//! `shot OUT.png [key=value ...]`: renders the app headlessly to look at.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use mareader_cefr::{Part, State};
use mareader_engine::command::Command;
use mareader_engine::dict::Download;
use mareader_engine::pane::{Arrow, ViewMode};
use mareader_engine::pane::{Fit, Step, ZoomCommand};
use mareader_settings::Provider;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ai::Job;
use mareader_ui_egui::ui_command::{Sent, UiCommand};

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let out = args.next().ok_or("usage: shot OUT.png [key=value ...]")?;
    let mut app = MareaderApp::default();
    // The AI's folder, for `explain=live`: one per run, so keys are kept.
    let ai_root = std::env::temp_dir().join(format!("mareader-shot-ai-{}", std::process::id()));
    let mut hover = None;
    let mut size = egui::vec2(1200.0, 800.0);
    let mut zooms = Vec::new();
    let mut pane_views = Vec::new();
    let mut deferred = Vec::new();
    let mut queued = Vec::new();
    let mut scroll = 0.0;
    let mut drag = None;
    let mut carry = None;
    let mut nudge = None;
    let mut words = false;
    let mut sounds = false;
    let mut searched = false;
    let mut tabs = 0;
    let mut right = None;
    let mut wait = 0;
    let mut wait_for_opening = true;
    let mut pause_read_aloud = false;
    let mut read_at = None;
    let mut read_target = None;
    let mut library_after_read_aloud = false;
    let mut library_route_frames = 8;
    let mut live_ai = false;
    let mut assistant_sample = false;
    let mut ai_key = None;
    let mut scripted = false;
    let mut clicks = Vec::new();
    let mut thumb_menu = None;
    let mut table_question = false;
    for kv in args {
        let (k, v) = kv
            .split_once('=')
            .ok_or(format!("expected key=value, got {kv}"))?;
        match k {
            "hover" => {
                let (x, y) = v.split_once(',').ok_or("hover=x,y")?;
                hover = Some(egui::pos2(
                    x.parse().map_err(|_| "hover x")?,
                    y.parse().map_err(|_| "hover y")?,
                ));
            }
            // A press, move and release once open: `drag=x1,y1,x2,y2`.
            "drag" => {
                let n: Vec<f32> = v.split(',').filter_map(|n| n.parse().ok()).collect();
                let [x1, y1, x2, y2] = n.as_slice() else {
                    return Err("drag=x1,y1,x2,y2".into());
                };
                drag = Some((egui::pos2(*x1, *y1), egui::pos2(*x2, *y2)));
            }
            // A drag held, not let go: `carry=x1,y1,x2,y2`.
            "carry" => {
                let n: Vec<f32> = v.split(',').filter_map(|n| n.parse().ok()).collect();
                let [x1, y1, x2, y2] = n.as_slice() else {
                    return Err("carry=x1,y1,x2,y2".into());
                };
                carry = Some((egui::pos2(*x1, *y1), egui::pos2(*x2, *y2)));
            }
            // The read-aloud pin, once open: `pin=page:char`.
            "pin" => {
                let (page, char) = v.split_once(':').ok_or("pin=page:char")?;
                let spot = mareader_speech::Spot {
                    page: page.parse().map_err(|_| "pin page")?,
                    char: char.parse().map_err(|_| "pin char")?,
                };
                let pane = app.active_pane();
                queued.push(Sent::Engine(Command::PlaceReadMarker { pane, spot }));
            }
            // The pin carried by this much, held: `nudge=dx,dy`.
            "nudge" => {
                let (x, y) = v.split_once(',').ok_or("nudge=dx,dy")?;
                let x: f32 = x.parse().map_err(|_| "nudge dx")?;
                nudge = Some(egui::vec2(x, y.parse().map_err(|_| "nudge dy")?));
            }
            // Applied once the document is open: `zoom=in,in,page`.
            "zoom" => {
                for z in v.split(',') {
                    zooms.push(match z {
                        "in" => ZoomCommand::Step(Step::In),
                        "out" => ZoomCommand::Step(Step::Out),
                        "width" => ZoomCommand::Fit(Fit::Width),
                        "page" => ZoomCommand::Fit(Fit::Page),
                        _ => return Err(format!("zoom: in, out, width or page, not {z}")),
                    });
                }
            }
            // Points to wheel the document down once open.
            "scroll" => scroll = v.parse().map_err(|_| "scroll=points")?,
            // Also once open: `view=spread arrows=right,right`.
            "view" => queued.push(Sent::Engine(Command::View(match v {
                "single" => ViewMode::Single,
                "spread" => ViewMode::Spread,
                "vertical" => ViewMode::Vertical,
                "horizontal" => ViewMode::Horizontal,
                _ => {
                    return Err(format!(
                        "view: single, spread, vertical or horizontal, not {v}"
                    ));
                }
            }))),
            "pane_view" => {
                let mut fields = v.split(':');
                let pane: u32 = fields
                    .next()
                    .ok_or("pane_view=pane:mode:fit")?
                    .parse()
                    .map_err(|_| "pane_view: pane number")?;
                if pane == 0 {
                    return Err("pane_view: pane numbers start at 1".into());
                }
                let mode = match fields.next().ok_or("pane_view=pane:mode:fit")? {
                    "single" => ViewMode::Single,
                    "spread" => ViewMode::Spread,
                    "vertical" => ViewMode::Vertical,
                    "horizontal" => ViewMode::Horizontal,
                    _ => {
                        return Err(
                            "pane_view: mode is single, spread, vertical or horizontal".into()
                        );
                    }
                };
                let fit = match fields.next().ok_or("pane_view=pane:mode:fit")? {
                    "width" => Fit::Width,
                    "page" => Fit::Page,
                    _ => return Err("pane_view: fit is width or page".into()),
                };
                if fields.next().is_some() {
                    return Err("pane_view=pane:mode:fit".into());
                }
                pane_views.push((pane, mode, fit));
            }
            "arrows" => {
                for a in v.split(',') {
                    queued.push(Sent::Engine(Command::Arrow(match a {
                        "left" => Arrow::Left,
                        "right" => Arrow::Right,
                        "up" => Arrow::Up,
                        "down" => Arrow::Down,
                        _ => return Err(format!("arrows: left, right, up or down, not {a}")),
                    })));
                }
            }
            // 1-based, like the page entry: `goto=12`.
            "goto" => {
                let page: usize = v.parse().map_err(|_| "goto=page")?;
                queued.push(Sent::Engine(Command::GoTo {
                    pane: app.active_pane(),
                    page: page.saturating_sub(1),
                }));
            }
            // The find bar, once open: `find=true`.
            "find" if v == "true" => queued.push(Sent::Ui(UiCommand::ShowFind)),
            // Searched, on the first hit from the page read: `find=fox`.
            "find" => {
                app.set_option(k, v)?;
                queued.push(Sent::Ui(UiCommand::ShowFind));
                searched = true;
            }
            // A book from the web, fetched first: `epub=https://…/a.epub`.
            "epub" | "pdf" if v.starts_with("https://") => {
                let path = download(v)?;
                app.set_option(k, &path.to_string_lossy())?;
            }
            // The real word levels, fetched into a temporary folder.
            "cefr" if v == "download" => {
                let dir = std::env::temp_dir().join("mareader-shot-cefr");
                app.set_option(k, &dir.to_string_lossy())?;
                words = true;
            }
            // The real pronunciation pack, fetched: `pronunciation=download`.
            "pronunciation" if v == "download" => {
                let dir = voice_models(false)?;
                app.set_option("voice_models", &dir.to_string_lossy())?;
                sounds = true;
            }
            // Voice models none or downloaded: `voice_models=ready`.
            "voice_models" if v == "absent" || v == "ready" => {
                let dir = voice_models(v == "ready")?;
                app.set_option(k, &dir.to_string_lossy())?;
            }
            // The test packs, converted: `dictionaries=fixtures`.
            "dictionaries" if v == "fixtures" => {
                let dir = fixture_packs()?;
                app.set_option(k, &dir.to_string_lossy())?;
            }
            // A real English-to-French pack for lookup shots.
            "dictionaries" if v == "download" => {
                let dir = download_dictionary()?;
                app.set_option(k, &dir.to_string_lossy())?;
            }
            // Tab presses once open, so the focus ring shows: `tab=3`.
            "tab" => tabs = v.parse().map_err(|_| "tab=count")?,
            // A right-click once open, so its menu shows: `right=x,y`.
            "right" => {
                let (x, y) = v.split_once(',').ok_or("right=x,y")?;
                right = Some(egui::pos2(
                    x.parse().map_err(|_| "right x")?,
                    y.parse().map_err(|_| "right y")?,
                ));
            }
            // Frames to run before the shot, at 60 a second: `wait=90`.
            "wait" => wait = v.parse().map_err(|_| "wait=frames")?,
            "wait_for_opening" => match v {
                "true" => wait_for_opening = true,
                "false" => wait_for_opening = false,
                _ => return Err("wait_for_opening=true or false".into()),
            },
            "pause_read_aloud" => pause_read_aloud = v == "true",
            // Starts reading at a phrase on a 1-based page: `read_at=14:word`.
            "read_at" => {
                let (page, phrase) = v.split_once(':').ok_or("read_at=page:phrase")?;
                let page: usize = page.parse().map_err(|_| "read_at page")?;
                if page == 0 || phrase.is_empty() {
                    return Err("read_at needs a 1-based page and phrase".into());
                }
                read_at = Some((page - 1, phrase.replace('_', " ")));
            }
            // Pressed by its name, `_` for a space: `click=All_voices`.
            "click" => clicks.push(v.replace('_', " ")),
            "thumb_menu" => {
                let range = match v.split_once('-') {
                    Some((a, b)) => (
                        a.parse::<usize>().map_err(|_| "thumb_menu=page")?,
                        b.parse::<usize>().map_err(|_| "thumb_menu=start-end")?,
                    ),
                    None => {
                        let page = v.parse::<usize>().map_err(|_| "thumb_menu=page")?;
                        (page, page)
                    }
                };
                thumb_menu = Some(range);
            }
            "table_question" => {
                if v == "book" {
                    let path = std::env::temp_dir().join("mareader-shot-question.epub");
                    let bytes = mareader_epub::sample::epub(8, "Sample");
                    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                    app.set_option("epub", &path.to_string_lossy())?;
                }
                table_question = v == "true" || v == "book";
            }
            // A scripted book's scripts run first: `scripted=true`.
            "scripted" => scripted = v == "true",
            "read_aloud" if v == "true" => queued.push(Sent::Engine(Command::ToggleReadAloud)),
            // Live explanations read their key from the Actions environment.
            "explain" if v == "live" => {
                let key = std::env::var("OPENAI_API_KEY")
                    .map_err(|_| "explain=live needs OPENAI_API_KEY")?;
                if key.trim().is_empty() {
                    return Err("explain=live needs OPENAI_API_KEY".into());
                }
                app.set_option("provider", "openai")?;
                app.set_option("ai_dir", &ai_root.to_string_lossy())?;
                live_ai = true;
                ai_key = Some(key);
            }
            // The assistant-generated answer for this sample word.
            "explain" if v == "assistant" => {
                app.set_option(k, v)?;
                assistant_sample = true;
            }
            // Leaves the reader after playback starts, for the Library route.
            "library_after_read_aloud" => match v {
                "true" => library_after_read_aloud = true,
                _ => return Err("library_after_read_aloud=true".into()),
            },
            // Frames after a route switch: `library_after_read_aloud_frames=2`.
            "library_after_read_aloud_frames" => {
                library_route_frames = v
                    .parse()
                    .map_err(|_| "library_after_read_aloud_frames=frames")?;
                if library_route_frames == 0 {
                    return Err("library_after_read_aloud_frames must be positive".into());
                }
            }
            // Clicks on its scripted pages: `script=#answer,li.right`.
            "script" => {
                app.set_option(k, v)?;
                scripted = true;
            }
            "size" => {
                let (w, h) = v.split_once('x').ok_or("size=WxH")?;
                size = egui::vec2(
                    w.parse().map_err(|_| "width")?,
                    h.parse().map_err(|_| "height")?,
                );
            }
            "selection" | "gloss" | "lookup" | "note" | "noted" | "highlight" | "bookmark"
            | "pick" | "page_ai" | "word_window" => {
                deferred.push((k.to_owned(), v.to_owned()));
            }
            _ => app.set_option(k, v)?,
        }
    }
    let mut harness = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .with_max_steps(64)
        .build_eframe(|_| app);
    harness.run_steps(4);
    if let Some(key) = ai_key {
        harness
            .state_mut()
            .queue(Sent::Engine(Command::Ai(Job::AddKey(
                Provider::OpenAi,
                key,
            ))));
        for _ in 0..20 {
            harness.run_steps(1);
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    if words {
        harness
            .state_mut()
            .queue(Sent::Engine(Command::Vocab(Download::Start)));
        wait_for_words(&mut harness)?;
    }
    if sounds {
        harness
            .state_mut()
            .queue(Sent::Engine(Command::Pronunciation(Download::Start)));
        wait_for_sounds(&mut harness)?;
    }
    if wait_for_opening {
        wait_for_books(&mut harness);
    }
    let mut library_shown = false;
    for c in queued {
        let mut from_text = false;
        let c = match c {
            Sent::Engine(Command::ToggleReadAloud) => match read_at.take() {
                Some((page, phrase)) => {
                    from_text = true;
                    let from = read_from_text(harness.state(), page, &phrase)?;
                    read_target = from.at;
                    let pane = harness.state().active_pane();
                    Sent::Engine(Command::ReadAloud { pane, from })
                }
                None => Sent::Engine(Command::ToggleReadAloud),
            },
            other => other,
        };
        harness.state_mut().queue(c);
        if from_text && library_after_read_aloud {
            harness.state_mut().queue(Sent::Ui(UiCommand::ShowLibrary));
            harness.run_steps(library_route_frames);
            library_shown = true;
        } else {
            harness.run_steps(8);
        }
    }
    if library_after_read_aloud && !library_shown {
        harness.state_mut().queue(Sent::Ui(UiCommand::ShowLibrary));
        harness.run_steps(library_route_frames);
    }
    if let Some(target) = read_target {
        for _ in 0..64 {
            if read_at_visible(&harness, target) {
                break;
            }
            let after = harness
                .state()
                .read_aloud()
                .reading_spot()
                .is_some_and(|(page, chars)| {
                    page > target.page
                        || (page == target.page
                            && chars.is_some_and(|range| range.start > target.char))
                });
            if after && let Some(previous) = harness.query_by_label("Previous sentence") {
                previous.click();
            }
            harness.run_steps(1);
        }
        if !read_at_visible(&harness, target) {
            return Err(format!(
                "read_at: target {target:?}, current {:?}",
                harness.state().read_aloud().reading_spot()
            ));
        }
    }
    for (pane, mode, fit) in pane_views.iter().copied() {
        harness.state_mut().set_option("focus", &pane.to_string())?;
        harness.state_mut().queue(Sent::Engine(Command::View(mode)));
        harness.run_steps(8);
        harness
            .state_mut()
            .queue(Sent::Engine(Command::Zoom(ZoomCommand::Fit(fit))));
        harness.run_steps(16);
    }
    if !pane_views.is_empty() {
        harness.state_mut().set_option("focus", "1")?;
    }
    if pause_read_aloud {
        let pause = harness
            .query_by_label("Pause")
            .ok_or("no read-aloud player to pause")?;
        pause.click();
        harness.run_steps(8);
    }
    // The hit is revealed, gliding there when it's pages away.
    if searched {
        harness.run_steps(60);
    }
    for z in zooms {
        harness.state_mut().queue(Sent::Engine(Command::Zoom(z)));
        harness.run_steps(16);
    }
    if scroll != 0.0 {
        harness.hover_at((size / 2.0).to_pos2());
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -scroll),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(40);
    }
    let lookup = deferred.iter().any(|(key, _)| key == "lookup");
    let frames = deferred.len().saturating_mul(8);
    for (key, value) in deferred {
        harness.state_mut().set_option(&key, &value)?;
    }
    harness.run_steps(frames);
    if live_ai {
        wait_for_ai_answer(&mut harness)?;
    }
    if assistant_sample {
        wait_for_assistant_answer(&mut harness)?;
    }
    if lookup {
        wait_for_dictionary_answer(&mut harness)?;
    }
    if let Some(by) = nudge {
        let pin = harness.query_by_label(mareader_ui_egui::read_aloud::marker::LABEL);
        let from = pin.ok_or("no pin to nudge")?.rect().center();
        carry = Some((from, from + by));
    }
    let held = carry.map(|c| (c, false));
    if let Some(((from, to), release)) = drag.map(|d| (d, true)).or(held) {
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        harness.hover_at(from);
        harness.event(press(from, true));
        harness.run_steps(2);
        for k in 1..=8 {
            harness.hover_at(from + (to - from) * (k as f32 / 8.0));
            harness.run_steps(1);
        }
        if release {
            harness.event(press(to, false));
        }
        // A held carry rests, so the grid's rests take, opening included.
        harness.run_steps(if release { 4 } else { 50 });
    }
    if let Some((start, end)) = thumb_menu {
        for page in start..=end {
            let label = format!("Page {page}");
            harness
                .get_by_label(&label)
                .click_modifiers(egui::Modifiers::COMMAND);
            harness.run_steps(2);
        }
        let target = format!("Page {start}");
        harness.get_by_label(&target).click_secondary();
        harness.run_steps(4);
    }
    if table_question {
        let pane = harness.state().active_pane();
        let reading = harness
            .state()
            .documents()
            .reading(pane)
            .ok_or("table question needs a book")?;
        let at = (0..reading.document.page_count())
            .find_map(|page| {
                let layer = reading.document.text_layer(page).ok()?;
                layer.tables.first().map(|_| page)
            })
            .ok_or("no table in the book")?;
        let mut settings = harness.state().settings().clone();
        settings.read_aloud.skips.tables = mareader_settings::Skip {
            on: true,
            ask: true,
        };
        harness.state_mut().set_settings(settings);
        let from = mareader_engine::read_aloud::ReadFrom {
            anchor: None,
            at: Some(mareader_speech::Spot {
                page: at.saturating_sub(1),
                char: 0,
            }),
            until: None,
        };
        harness
            .state_mut()
            .queue(Sent::Engine(Command::ReadAloud { pane, from }));
        let until = Instant::now() + Duration::from_secs(30);
        while harness.query_by_label("Read the table").is_none() && Instant::now() < until {
            harness.run_steps(3);
            std::thread::sleep(Duration::from_millis(10));
        }
        if harness.query_by_label("Read the table").is_none() {
            return Err("table question did not appear".into());
        }
        harness.run_steps(30);
    }
    if let Some(pos) = right {
        harness.hover_at(pos);
        harness.run_steps(1);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run_steps(1);
        }
        harness.run_steps(4);
    }
    if scripted {
        wait_for_scripts(&mut harness)?;
    }
    harness.run_steps(wait);
    for label in &clicks {
        let node = (harness.query_by_label(label)).ok_or(format!("nothing named {label}"))?;
        node.click();
        harness.run_steps(8);
    }
    for _ in 0..tabs {
        harness.key_press(egui::Key::Tab);
        harness.run_steps(4);
    }
    // Long enough for a tooltip to appear.
    if let Some(p) = hover {
        harness.hover_at(p);
        harness.run_steps(45);
    }
    let image = harness.render()?;
    image.save(&out).map_err(|e| e.to_string())?;
    println!("{out}");
    Ok(())
}

/// Finds a phrase on a page and makes it the starting point for reading.
fn read_from_text(
    app: &MareaderApp,
    page: usize,
    phrase: &str,
) -> Result<mareader_engine::read_aloud::ReadFrom, String> {
    let pane = app.active_pane();
    let reading = app
        .documents()
        .reading(pane)
        .ok_or("read_at needs an open document")?;
    let layer = reading
        .document
        .text_layer(page)
        .map_err(|_| format!("read_at: no text on page {}", page + 1))?;
    let chars: Vec<_> = layer
        .text
        .chars()
        .iter()
        .enumerate()
        .filter_map(|(i, c)| (!c.ch.is_whitespace()).then_some((c.ch.to_ascii_lowercase(), i)))
        .collect();
    let phrase: Vec<_> = phrase
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if phrase.is_empty() {
        return Err("read_at needs a phrase with letters".into());
    }
    let start = chars
        .windows(phrase.len())
        .position(|window| window.iter().map(|(c, _)| *c).eq(phrase.iter().copied()))
        .ok_or(format!("read_at: phrase not found on page {}", page + 1))?;
    let char = chars[start].1;
    Ok(mareader_engine::read_aloud::ReadFrom {
        anchor: None,
        at: Some(mareader_speech::Spot { page, char }),
        until: None,
    })
}

/// Whether the player's current phrase covers the requested reading spot.
fn read_at_visible(harness: &Harness<'_, MareaderApp>, target: mareader_speech::Spot) -> bool {
    harness
        .state()
        .read_aloud()
        .reading_spot()
        .is_some_and(|(page, chars)| {
            page == target.page && chars.is_some_and(|range| range.contains(&target.char))
        })
}

/// `url` saved under the temporary folder, by its last part.
fn download(url: &str) -> Result<std::path::PathBuf, String> {
    let name = url.rsplit('/').next().filter(|n| !n.is_empty());
    let dir = std::env::temp_dir().join("mareader-shot-books");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name.unwrap_or("book"));
    let ok = std::process::Command::new("curl")
        .args(["-sfL", "--retry", "2", "-o"])
        .arg(&path)
        .arg(url)
        .status()
        .map_err(|e| format!("curl: {e}"))?
        .success();
    match ok {
        true => Ok(path),
        false => Err(format!("couldn't fetch {url}")),
    }
}

/// Downloads and converts the pinned English-to-French pack.
fn download_dictionary() -> Result<std::path::PathBuf, String> {
    let pack = mareader_dict::PACKS
        .iter()
        .find(|pack| pack.id == "wiktionary-en-fr")
        .ok_or("no English-to-French pack")?;
    let dir = std::env::temp_dir().join(format!("mareader-shot-dict-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let db = mareader_dict::db_file(&dir, pack);
    let mut failures = Vec::new();
    for url in pack.urls() {
        let result = download(&url).and_then(|parquet| {
            mareader_dict::build(&parquet, &db)
                .map(|_| ())
                .map_err(|e| format!("dictionary convert: {e}"))
        });
        match result {
            Ok(()) => return Ok(dir),
            Err(error) => failures.push(error),
        }
    }
    Err(format!("dictionary pack: {}", failures.join("; ")))
}

/// The Burmese and French test packs, built into a temporary folder.
fn fixture_packs() -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join("mareader-shot-packs");
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dict/tests/fixtures");
    for (file, id) in [
        ("en-my.parquet", "mcfnlp-en-my"),
        ("en-fr.parquet", "wiktionary-en-fr"),
    ] {
        let pack = (mareader_dict::PACKS.iter())
            .find(|p| p.id == id)
            .ok_or(format!("no pack {id}"))?;
        let db = mareader_dict::db_file(&dir, pack);
        std::fs::create_dir_all(db.parent().unwrap_or(&dir)).map_err(|e| e.to_string())?;
        mareader_dict::build(&fixtures.join(file), &db).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

/// Waits until the open word card has a completed hosted answer.
fn wait_for_ai_answer(harness: &mut Harness<'_, MareaderApp>) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        harness.run_steps(1);
        let by = {
            let study = harness.state().study();
            study
                .card_answer()
                .is_some()
                .then(|| study.card_by().unwrap_or_default().to_owned())
        };
        if let Some(by) = by {
            if !by.starts_with("OpenAI") {
                return Err("AI explanation: expected an OpenAI answer".into());
            }
            harness.run_steps(4);
            return Ok(());
        }
        if Instant::now() > until {
            return Err("AI explanation: timed out waiting for OpenAI".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Waits until the sample word has its complete assistant-generated answer.
fn wait_for_assistant_answer(harness: &mut Harness<'_, MareaderApp>) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        harness.run_steps(1);
        let by = {
            let study = harness.state().study();
            study
                .card_answer()
                .is_some()
                .then(|| study.card_by().unwrap_or_default().to_owned())
        };
        if let Some(by) = by {
            if by != "Assistant-generated sample" {
                return Err("AI sample: unexpected answer source".into());
            }
            harness.run_steps(4);
            return Ok(());
        }
        if Instant::now() > until {
            return Err("AI sample: timed out waiting for its answer".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Waits until the open dictionary card has a real entry.
fn wait_for_dictionary_answer(harness: &mut Harness<'_, MareaderApp>) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        harness.run_steps(1);
        if let Some(answers) = harness.state().dict().card_answers() {
            if answers.is_empty() {
                return Err("dictionary lookup: no entry found".into());
            }
            harness.run_steps(4);
            return Ok(());
        }
        if Instant::now() > until {
            return Err("dictionary lookup: timed out".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Steps until the scripts have answered every click, for a minute at most.
fn wait_for_scripts(harness: &mut Harness<'_, MareaderApp>) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(60);
    let mut calm = 0;
    // Settled for a second: clicks sent were taken and answered.
    while calm < 10 {
        harness.run_steps(1);
        calm = if harness.state().scripts_settled() {
            calm + 1
        } else {
            0
        };
        if Instant::now() > until {
            let state = harness.state().scripts_state();
            return Err(format!("scripts: timed out ({state})"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// Frames in real time until every book asked for is open, or 30 s.
fn wait_for_books(harness: &mut Harness<'_, MareaderApp>) {
    let start = Instant::now();
    while harness.state().documents().opening() && start.elapsed() < Duration::from_secs(30) {
        harness.run_steps(1);
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Steps until the pronunciation pack is in, for two minutes at most.
fn wait_for_sounds(harness: &mut Harness<'_, MareaderApp>) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(120);
    while !harness.state().sounds().ready() {
        if let mareader_speech::Stage::Failed(why) = harness.state().sounds().stage() {
            return Err(format!("pronunciation: {why}"));
        }
        if Instant::now() > until {
            return Err("pronunciation: timed out".into());
        }
        harness.run_steps(1);
        std::thread::sleep(Duration::from_millis(100));
    }
    harness.run_steps(4);
    Ok(())
}

/// Steps until the word levels are in, for two minutes at most.
fn wait_for_words(harness: &mut Harness<'_, MareaderApp>) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        harness.run_steps(1);
        match harness.state().vocab().state(Part::Dataset) {
            State::Ready => break,
            State::Failed(why) => return Err(format!("word levels: {why}")),
            _ if Instant::now() > until => return Err("word levels: timed out".into()),
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    // Pages ask for their levels, then repaint shaded.
    harness.run_steps(12);
    Ok(())
}

/// A fresh models folder; if `ready`, sparse files of the listed sizes.
fn voice_models(ready: bool) -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join("mareader-shot-voice-models");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if !ready {
        return Ok(dir);
    }
    for model in mareader_speech::MODELS {
        let extra = model.voices.iter().take(4).flat_map(|v| v.files());
        // The best build and the smallest, so one is left to download.
        let builds = [model.builds.first(), model.builds.last()];
        let builds = builds.into_iter().flatten().flat_map(|b| b.files());
        for f in model.core().into_iter().chain(builds).chain(extra) {
            let path = dir.join(model.id).join(f.path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
            file.set_len(f.bytes).map_err(|e| e.to_string())?;
        }
    }
    Ok(dir)
}
