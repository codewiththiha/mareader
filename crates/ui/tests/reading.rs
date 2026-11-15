//! Opening and scrolling PDFs, headless, with jobs run between frames.

use eframe::{App as _, Storage as _};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_ui::app::LAST_DOCUMENT_KEY;
use mareader_ui::command::Command;
use mareader_ui::documents::Placement;
use mareader_ui::{MareaderApp, OpenRequest};

use crate::common::reader_200 as reader;

fn textures(h: &Harness<'static, MareaderApp>) -> usize {
    h.ctx.tex_manager().read().num_allocated()
}

fn visible(h: &Harness<'static, MareaderApp>) -> std::ops::Range<usize> {
    let pane = h.state().active_pane();
    h.state().documents().visible(pane).unwrap_or(0..0)
}

#[test]
fn the_sample_opens_titled_with_its_first_page_drawn() {
    let h = reader(&[("pdf", "sample:500")]);
    assert_eq!(h.state().title(), "A Mareader Sample");
    let pane = h.state().active_pane();
    assert_eq!(h.state().documents().page_count(pane), Some(500));
    assert_eq!(visible(&h).start, 0);
    // What is on screen, plus one page of prefetch.
    let (docs, pages) = h.state().documents().stats();
    assert_eq!((docs, pages), (1, visible(&h).len() + 1));
}

#[test]
fn the_wheel_scrolls_and_only_nearby_pages_are_kept_drawn() {
    let mut h = reader(&[("pdf", "sample:10000")]);
    let pane = h.state().active_pane();
    assert_eq!(h.state().documents().page_count(pane), Some(10_000));
    h.hover_at(egui::pos2(450.0, 400.0));
    for _ in 0..20 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -2000.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    h.run();
    let shown = visible(&h);
    assert!(shown.start > 20, "scrolled to {shown:?}");
    // Ten thousand pages, yet work and textures track the viewport.
    let (_, pages) = h.state().documents().stats();
    assert!(pages <= 40, "{pages} textures");
}

#[test]
fn closing_the_last_pane_on_a_book_frees_its_textures() {
    let mut h = reader(&[("panes", "2")]);
    let baseline = textures(&h);
    let panes = h.state().panes();
    let (first, second) = (panes[0], panes[1]);
    h.state_mut().set_option("focus", "2").ok();
    h.state_mut().set_option("pdf", "sample:30").ok();
    h.run();
    assert!(textures(&h) > baseline, "pages were uploaded");
    h.state_mut().queue(Command::ClosePane(second));
    h.run();
    assert_eq!(h.state().active_pane(), first);
    // Nothing of it is left: no document, raster, text layer or job.
    let empty = mareader_ui::documents::Meter::default();
    assert_eq!(h.state().documents().meter(), empty);
    assert_eq!(textures(&h), baseline);
}

#[test]
fn a_broken_file_says_why_and_leaves_the_pane_empty() {
    let mut h = reader(&[]);
    let junk = OpenRequest::bytes(b"%PDF-1.7 nonsense".to_vec(), "Notes.pdf");
    h.state_mut().queue(Command::Open(junk));
    h.run();
    let toast = h.state().toast().map(|t| t.message.clone());
    let cut = "Couldn't open Notes: the PDF is cut short. \
               It may not have finished downloading; download it again.";
    assert_eq!(toast.as_deref(), Some(cut));
    assert_eq!(h.state().title(), "Mareader");
}

#[test]
fn a_damaged_file_and_a_web_page_are_told_apart() {
    let mut h = reader(&[]);
    let broken = b"%PDF-1.7\nnonsense\n%%EOF\n".to_vec();
    h.state_mut()
        .queue(Command::Open(OpenRequest::bytes(broken, "Deed.pdf")));
    h.run();
    let toast = h.state().toast().map(|t| t.message.clone());
    let said = toast.unwrap_or_default();
    assert!(
        said.starts_with("Couldn't open Deed: the PDF is damaged."),
        "{said}"
    );
    let page = b"<html><body>Sign in</body></html>".to_vec();
    h.state_mut()
        .queue(Command::Open(OpenRequest::bytes(page, "Bill.pdf")));
    h.run();
    let toast = h.state().toast().map(|t| t.message.clone());
    let said = toast.unwrap_or_default();
    assert!(
        said.starts_with("Couldn't open Bill: the file isn't a PDF."),
        "{said}"
    );
}

#[test]
fn a_cut_short_file_opens_with_a_warning() {
    let mut h = reader(&[]);
    let mut bytes = mareader_pdf::sample::pdf(3, "Partial");
    bytes.truncate(bytes.len() - "%%EOF\n".len());
    h.state_mut()
        .queue(Command::Open(OpenRequest::bytes(bytes, "Partial.pdf")));
    h.run();
    let toast = h.state().toast().cloned();
    let tone = toast.as_ref().map(|t| t.tone);
    assert_eq!(tone, Some(mareader_ui::toast::Tone::Warning));
    let said = toast.map(|t| t.message).unwrap_or_default();
    assert_eq!(said, "Partial looks cut short; some pages may be missing.");
    assert_ne!(h.state().title(), "Mareader", "it opened");
}

#[test]
fn an_epub_opens_titled_with_its_pages_drawn() {
    let h = reader(&[("epub", "sample:3")]);
    assert_eq!(h.state().title(), "A Mareader Book");
    let pane = h.state().active_pane();
    let pages = h.state().documents().page_count(pane);
    // Three chapters, each opening a page and running onto another.
    assert!(pages.is_some_and(|n| n >= 6), "{pages:?}");
    assert!(h.state().settings_scope().book());
    let (docs, drawn) = h.state().documents().stats();
    assert_eq!((docs, drawn), (1, visible(&h).len() + 1));
}

#[test]
fn a_broken_epub_says_why() {
    let mut h = reader(&[]);
    let book = OpenRequest::bytes(b"PK\x03\x04 not really".to_vec(), "Dune.epub");
    h.state_mut().queue(Command::Open(book));
    h.run_steps(3);
    let toast = h.state().toast().map(|t| t.message.clone());
    assert!(
        toast.as_ref().is_some_and(|t| t.contains("EPUB")),
        "{toast:?}"
    );
}

#[test]
fn bigger_type_sets_an_open_book_again_at_the_page_read() {
    let mut h = reader(&[("epub", "sample:6")]);
    let pane = h.state().active_pane();
    let view = mareader_ui::layout::ViewMode::Single;
    h.state_mut().queue(Command::View(view));
    // A single page takes the pane's size: the book is set again first.
    h.run();
    let before = h.state().documents().page_count(pane).unwrap_or(0);
    let read = before / 2;
    h.state_mut().queue(Command::GoTo { pane, page: read });
    h.run();
    let document = |h: &Harness<'static, MareaderApp>| {
        let reading = h.state().documents().reading(pane);
        reading.map(|r| r.document)
    };
    let place = document(&h).and_then(|d| d.anchor(read));
    assert!(place.is_some(), "an EPUB page has a place");
    h.state_mut().set_option("text_size", "26").ok();
    h.run();
    let after = h.state().documents().page_count(pane).unwrap_or(0);
    assert!(after > before, "{after} > {before}");
    // The page read now holds the old place: it starts at or before it.
    let current = h.state().documents().current(pane).unwrap_or(0);
    let book = document(&h);
    let start = book.as_ref().and_then(|d| d.anchor(current));
    let next = book.as_ref().and_then(|d| d.anchor(current + 1));
    assert!(start.is_some() && start <= place, "{start:?} {place:?}");
    assert!(next.is_none() || next > place, "{next:?} {place:?}");
    // The old book is closed, its pages with it.
    let (docs, _) = h.state().documents().stats();
    assert_eq!(docs, 1);
}

#[test]
fn a_book_s_vertical_strip_streams_one_column_edge_to_edge() {
    use mareader_ui::zoom::{Step, ZoomCommand};
    let options = [
        ("epub", "sample:6"),
        ("book_layout", "true"),
        ("column", "left"),
    ];
    let mut h = reader(&options);
    let pane = h.state().active_pane();
    let vertical = Some(mareader_ui::layout::ViewMode::Vertical);
    assert_eq!(h.state().documents().mode(pane), vertical);
    for _ in 0..8 {
        h.state_mut()
            .queue(Command::Zoom(ZoomCommand::Step(Step::Out)));
    }
    h.run();
    // Each page's column, and where its band ends, on screen.
    let columns = |h: &Harness<'static, MareaderApp>| {
        let reading = h.state().documents().reading(pane);
        let book = reading.map(|r| r.document);
        let bands = book.as_ref().and_then(|d| d.bands()).unwrap_or(&[]);
        let sheets = h.state().documents().sheets(pane);
        let cut = |s: &mareader_ui::selection::Sheet| {
            let b = bands.get(s.page)?;
            let left = s.rect.left() + b.x * s.scale;
            let top = s.rect.top() + b.y * s.scale;
            Some((left, top, top + b.height * s.scale))
        };
        sheets.iter().filter_map(cut).collect::<Vec<_>>()
    };
    let cuts = columns(&h);
    assert!(cuts.len() >= 3, "{cuts:?}");
    for pair in cuts.windows(2) {
        let [(left, _, bottom), (next_left, top, _)] = pair else {
            continue;
        };
        // No gaps (sheets round alone), one column though book layout shifts.
        assert!((bottom - top).abs() < 2.0, "{pair:?}");
        assert!((left - next_left).abs() < 1.5, "{pair:?}");
    }
    let pane_left = h
        .state()
        .pane_rects()
        .first()
        .map_or(0.0, |(_, r)| r.left());
    let left = cuts.first().map_or(0.0, |c| c.0);
    assert!((left - pane_left - 16.0).abs() < 1.5, "{left} {pane_left}");
    h.state_mut().set_option("column", "right").ok();
    h.run();
    let right = columns(&h).first().map_or(0.0, |c| c.0);
    assert!(right > left + 50.0, "{right} > {left}");
}

#[test]
fn the_fonts_tab_steps_the_size_and_switches_justification() {
    let mut h = reader(&[("epub", "sample:2")]);
    h.state_mut().set_option("settings", "fonts").ok();
    h.run_steps(3);
    assert_eq!(h.state().settings().text.font_size, 17.0);
    h.get_by_role_and_label(Role::Button, "More text size")
        .click();
    h.run_steps(2);
    // A step is five percent of the standard size.
    assert_eq!(h.state().settings().text.size_pct(), 105.0);
    let own = "Override book font";
    h.get_by_role_and_label(Role::CheckBox, own).scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, own).click();
    h.run();
    assert!(h.state().settings().text.override_font);
    let justify = "Full justification";
    h.get_by_role_and_label(Role::CheckBox, justify)
        .scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, justify).click();
    h.run();
    assert!(h.state().settings().text.justify);
    let book = "Book layout";
    h.get_by_role_and_label(Role::CheckBox, book).scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, book).click();
    h.run();
    assert!(h.state().settings().text.book_layout);
    // The body font, and the book's own families behind a disclosure.
    let families = "Book's own families";
    h.get_by_label(families).scroll_to_me();
    h.run();
    h.get_by_label(families).click();
    h.run();
    for slot in [
        "Body font",
        "Serif font",
        "Sans-serif font",
        "Monospace font",
    ] {
        assert!(h.query_by_label(slot).is_some(), "{slot}");
    }
}

fn sample(pages: usize) -> OpenRequest {
    let pdf = mareader_pdf::sample::pdf(pages, "A Mareader Sample");
    OpenRequest::bytes(pdf, "sample.pdf")
}

#[test]
fn open_beside_splits_and_opens_in_the_new_pane() {
    let mut h = reader(&[]);
    let first = h.state().active_pane();
    let request = OpenRequest {
        placement: Placement::Beside,
        ..sample(5)
    };
    h.state_mut().queue(Command::Open(request));
    h.run();
    let panes = h.state().panes();
    assert_eq!(panes.len(), 2);
    let new = h.state().active_pane();
    assert_ne!(new, first);
    let docs = h.state().documents();
    assert_eq!(docs.page_count(new), Some(5));
    assert_eq!(docs.page_count(first), None, "the first pane stays empty");
}

/// A file as the desktop hands it over: a path, read by a worker.
#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

#[test]
fn a_dropped_file_opens_in_the_pane_under_the_pointer() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("mareader-drop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("Dropped Notes.pdf");
    let pdf = mareader_pdf::sample::pdf(3, "untitled");
    std::fs::write(&path, pdf).map_err(|e| e.to_string())?;
    let mut h = reader(&[("panes", "2")]);
    let right = h.state().panes()[1];
    h.hover_at(egui::pos2(700.0, 350.0));
    h.run_steps(2);
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(path)));
    h.run();
    assert_eq!(h.state().active_pane(), right);
    assert_eq!(h.state().title(), "Dropped Notes");
    std::fs::remove_dir_all(dir).map_err(|e| e.to_string())
}

#[test]
fn an_empty_pane_offers_the_open_dialog() {
    let h = reader(&[]);
    let _ = h.get_by_label("Open…");
}

fn zoom(h: &Harness<'static, MareaderApp>) -> mareader_ui::zoom::ZoomState {
    let pane = h.state().active_pane();
    let state = h.state().documents().zoom_state(pane);
    state.unwrap_or(mareader_ui::zoom::ZoomState {
        zoom: 0.0,
        fit: None,
    })
}

fn current(h: &Harness<'static, MareaderApp>) -> usize {
    let pane = h.state().active_pane();
    h.state().documents().current(pane).unwrap_or(usize::MAX)
}

/// Opens 60 pages and wheels down to somewhere in the middle.
fn reading_page_twenty() -> Harness<'static, MareaderApp> {
    let mut h = reader(&[("pdf", "sample:60")]);
    h.hover_at(egui::pos2(450.0, 400.0));
    for _ in 0..12 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -1000.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    h.run();
    h
}

#[test]
fn zoom_keys_step_the_ladder_and_keep_the_page_in_view() {
    use mareader_ui::zoom::{Fit, MAX};
    let mut h = reading_page_twenty();
    let before = zoom(&h);
    assert_eq!(before.fit, Some(Fit::Width));
    let page = current(&h);
    assert!(page > 5, "reading page {page}");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Equals);
    h.run();
    let after = zoom(&h);
    assert_eq!(after.fit, None);
    assert_eq!(
        Some(after.zoom),
        mareader_ui::zoom::next(before.zoom, mareader_ui::zoom::Step::In)
    );
    assert_eq!(current(&h), page, "the page under the centre stays");
    // Bare minus steps back down; Cmd+0 returns to fit width.
    h.key_press(egui::Key::Minus);
    h.run();
    assert!(zoom(&h).zoom < after.zoom);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num0);
    h.run();
    assert_eq!(zoom(&h), before);
    assert_eq!(current(&h), page);
    // Chained presses stop at the top of the ladder.
    for _ in 0..20 {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Plus);
    }
    h.run();
    assert_eq!(zoom(&h).zoom, MAX);
    assert_eq!(current(&h), page);
}

#[test]
fn a_pinch_zooms_about_the_pointer() {
    let mut h = reading_page_twenty();
    let before = zoom(&h).zoom;
    let page = current(&h);
    h.hover_at(egui::pos2(450.0, 400.0));
    h.event(egui::Event::Zoom(1.5));
    h.run();
    let after = zoom(&h);
    assert_eq!(after.fit, None);
    assert!((after.zoom - before * 1.5).abs() < 1e-3, "{after:?}");
    assert_eq!(current(&h), page);
}

#[test]
fn the_menu_fits_the_page() {
    use mareader_ui::zoom::Fit;
    let mut h = reader(&[("pdf", "sample:5")]);
    h.state_mut().queue(Command::ShowViewMenu);
    h.run();
    h.get_by_label("Fit page").click();
    h.run();
    let z = zoom(&h);
    assert_eq!(z.fit, Some(Fit::Page));
    // A letter page 700 high, less margins, fits by its height.
    let want = (700.0 - 32.0) / 792.0;
    assert!((z.zoom - want).abs() < 0.01, "{z:?} want {want}");
    let _ = h.get_by_label(&z.percent());
}

#[test]
fn a_resize_refits_and_keeps_the_page() {
    let mut h = reading_page_twenty();
    let page = current(&h);
    h.set_size(egui::vec2(600.0, 700.0));
    h.run();
    let z = zoom(&h);
    // No page margin by default: the page meets the pane's sides.
    assert!((z.zoom - 600.0 / 612.0).abs() < 0.01, "{z:?}");
    assert_eq!(current(&h), page);
}

#[derive(Default)]
struct Memory(std::collections::HashMap<String, String>);

impl eframe::Storage for Memory {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }

    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }

    fn flush(&mut self) {}
}

/// A sample PDF on disk, deleted when dropped.
struct File(std::path::PathBuf);

impl File {
    fn sample(pages: usize, tag: &str) -> Result<Self, String> {
        let name = format!("mareader-{tag}-{}.pdf", std::process::id());
        let path = std::env::temp_dir().join(name);
        let pdf = mareader_pdf::sample::pdf(pages, "Read Last Time");
        std::fs::write(&path, pdf).map_err(|e| e.to_string())?;
        Ok(Self(path))
    }
}

impl File {
    fn book(chapters: usize, tag: &str) -> Result<Self, String> {
        let name = format!("mareader-{tag}-{}.epub", std::process::id());
        let path = std::env::temp_dir().join(name);
        let epub = mareader_epub::sample::epub(chapters, "Read Last Time");
        std::fs::write(&path, epub).map_err(|e| e.to_string())?;
        Ok(Self(path))
    }
}

impl Drop for File {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}

/// The first page's words in the active pane's book.
fn first_page(h: &Harness<'static, MareaderApp>) -> String {
    let pane = h.state().active_pane();
    let doc = h.state().documents().reading(pane).map(|r| r.document);
    let layer = doc.and_then(|d| d.text_layer(0).ok());
    layer.map_or(String::new(), |l| l.text.text(0..l.text.len()))
}

#[test]
fn a_note_saved_elsewhere_reloads_where_it_is_read() -> Result<(), String> {
    let name = format!("mareader-live-{}.md", std::process::id());
    let file = File(std::env::temp_dir().join(name));
    std::fs::write(&file.0, "# Trip\n\nThe old words.\n").map_err(|e| e.to_string())?;
    let mut app = MareaderApp::default();
    app.queue(Command::Open(OpenRequest::path(file.0.clone())));
    let mut h = launched(app);
    assert!(first_page(&h).contains("old words"), "{}", first_page(&h));
    let pane = h.state().active_pane();
    let before = h.state().documents().reading(pane).map(|r| r.doc);
    // Saved at once: the watch saw the file as it opened, not a second on.
    std::thread::sleep(std::time::Duration::from_millis(150));
    let saved = "# Trip\n\nThe new words, saved in another app.\n";
    std::fs::write(&file.0, saved).map_err(|e| e.to_string())?;
    settle(&mut h, |h| first_page(h).contains("new words"));
    assert!(first_page(&h).contains("new words"), "{}", first_page(&h));
    let after = h.state().documents().reading(pane).map(|r| r.doc);
    assert_ne!(before, after);
    assert_eq!(h.state().documents().stats().0, 1, "the old one closed");
    Ok(())
}

/// Runs frames in real time, for the file watch, until `done` or 6 s.
fn settle(
    h: &mut Harness<'static, MareaderApp>,
    done: impl Fn(&Harness<'static, MareaderApp>) -> bool,
) {
    for _ in 0..60 {
        if done(h) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        h.run_steps(2);
    }
}

#[test]
fn a_pdf_saved_elsewhere_reloads_on_its_page_and_waits_out_a_bad_save() -> Result<(), String> {
    let file = File::sample(6, "live-pdf")?;
    let mut app = MareaderApp::default();
    app.queue(Command::Open(OpenRequest::path(file.0.clone())));
    let mut h = launched(app);
    let pane = h.state().active_pane();
    h.state_mut().queue(Command::GoTo { pane, page: 3 });
    h.run_steps(30);
    assert_eq!(h.state().documents().current(pane), Some(3));
    let doc =
        |h: &Harness<'static, MareaderApp>| h.state().documents().reading(pane).map(|r| r.doc);
    let first = doc(&h);
    // Half written: it can't open, so the old one stays.
    std::fs::write(&file.0, b"%PDF-1.7 half").map_err(|e| e.to_string())?;
    settle(&mut h, |_| false);
    assert_eq!(doc(&h), first, "the old one stays");
    assert_eq!(h.state().title(), "Read Last Time");
    let pdf = mareader_pdf::sample::pdf(6, "Saved Again");
    std::fs::write(&file.0, pdf).map_err(|e| e.to_string())?;
    settle(&mut h, |h| h.state().title() == "Saved Again");
    assert_eq!(h.state().title(), "Saved Again");
    assert_ne!(doc(&h), first);
    assert_eq!(h.state().documents().current(pane), Some(3), "on its page");
    Ok(())
}

fn launched(mut app: MareaderApp) -> Harness<'static, MareaderApp> {
    app.set_option("workers", "0").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

#[test]
fn a_book_keeps_its_own_design_over_the_setting() -> Result<(), String> {
    use mareader_library::Design;
    use mareader_reflow::Design as Set;
    let file = File::book(2, "design")?;
    let mut app = MareaderApp::default();
    app.queue(Command::Open(OpenRequest::path(file.0.clone())));
    let mut h = launched(app);
    let pane = h.state().active_pane();
    let design = |h: &Harness<'static, MareaderApp>| {
        h.state().documents().typography(pane).map(|t| t.design)
    };
    assert_eq!(design(&h), Some(Set::Auto));
    let Some((book, None)) = h.state().settings_scope().book_design else {
        return Err("the open book follows the setting".into());
    };
    h.state_mut().set_option("settings", "fonts").ok();
    h.run_steps(3);
    assert!(h.query_by_label("This book").is_some());
    let choose = mareader_ui::library::Action::Design(book, Some(Design::Off));
    h.state_mut().queue(Command::Library(choose));
    h.run();
    let own = h.state().library().book(book).and_then(|b| b.design.value);
    assert_eq!(own, Some(Design::Off));
    assert_eq!(design(&h), Some(Set::Off));
    // The setting moves on; the book keeps its own choice.
    h.state_mut().set_option("design", "on").ok();
    h.run();
    assert_eq!(
        h.state().settings().text.book_design,
        mareader_settings::BookDesign::On
    );
    assert_eq!(design(&h), Some(Set::Off));
    let follow = mareader_ui::library::Action::Design(book, None);
    h.state_mut().queue(Command::Library(follow));
    h.run();
    assert_eq!(design(&h), Some(Set::On));
    Ok(())
}

#[test]
fn the_book_read_at_exit_reopens_on_its_page() -> Result<(), String> {
    let file = File::sample(30, "resume")?;
    let mut app = MareaderApp::default();
    let request = OpenRequest {
        page: 12,
        ..OpenRequest::path(file.0.clone())
    };
    app.queue(Command::Open(request));
    let mut h = launched(app);
    let mut disk = Memory::default();
    h.state_mut().save(&mut disk);
    // The next launch, from what was saved.
    let mut again = MareaderApp::from_storage(Some(&disk));
    assert!(
        again.resume(),
        "saved {:?}",
        disk.get_string(LAST_DOCUMENT_KEY)
    );
    let h = launched(again);
    assert_eq!(h.state().title(), "Read Last Time");
    let pane = h.state().active_pane();
    assert_eq!(h.state().documents().current(pane), Some(12));
    // A file gone since then leaves the window empty, without a fuss.
    drop(file);
    let mut moved = MareaderApp::from_storage(Some(&disk));
    assert!(!moved.resume());
    Ok(())
}

#[test]
fn a_book_read_at_exit_reopens_at_its_place_in_new_type() -> Result<(), String> {
    let file = File::book(6, "resume-book")?;
    let mut app = MareaderApp::default();
    let request = OpenRequest {
        page: 8,
        ..OpenRequest::path(file.0.clone())
    };
    app.queue(Command::Open(request));
    let mut h = launched(app);
    let pane = h.state().active_pane();
    assert_eq!(h.state().documents().current(pane), Some(8));
    let reading = h.state().documents().reading(pane);
    let place = reading.and_then(|r| r.document.anchor(8));
    assert!(place.is_some(), "a book's page has a place");
    let mut disk = Memory::default();
    h.state_mut().save(&mut disk);
    let saved = disk.get_string(LAST_DOCUMENT_KEY).unwrap_or_default();
    assert!(saved.contains("\"place\""), "{saved}");
    // Next launch in bigger type: the page differs, the place doesn't.
    let mut again = MareaderApp::from_storage(Some(&disk));
    again.settings_mut().text.font_size = 28.0;
    assert!(again.resume());
    let h = launched(again);
    let pane = h.state().active_pane();
    let current = h.state().documents().current(pane).unwrap_or(0);
    assert!(current > 8, "{current}");
    let book = h.state().documents().reading(pane).map(|r| r.document);
    let start = book.as_ref().and_then(|d| d.anchor(current));
    let next = book.as_ref().and_then(|d| d.anchor(current + 1));
    assert!(start.is_some() && start <= place, "{start:?} {place:?}");
    assert!(next.is_none() || next > place, "{next:?} {place:?}");
    Ok(())
}

#[test]
fn a_dropped_book_or_an_empty_window_leaves_nothing_to_reopen() {
    let mut h = reader(&[]);
    let mut disk = Memory::default();
    h.state_mut().save(&mut disk);
    assert_eq!(disk.get_string(LAST_DOCUMENT_KEY).as_deref(), Some("null"));
    // Bytes, such as a browser's drop, have no file to come back to.
    h.state_mut().queue(Command::Open(sample(3)));
    h.run();
    assert_eq!(h.state().last_document(), None);
    let mut app = MareaderApp::from_storage(Some(&disk));
    assert!(!app.resume());
}

#[test]
fn one_file_in_two_panes_is_read_once_and_drawn_once() -> Result<(), String> {
    let file = File::sample(20, "twice")?;
    let mut h = reader(&[("panes", "2")]);
    let panes = h.state().panes();
    for pane in &panes {
        let request = OpenRequest {
            pane: Some(*pane),
            ..OpenRequest::path(file.0.clone())
        };
        h.state_mut().queue(Command::Open(request));
        h.run();
    }
    let meter = h.state().documents().meter();
    assert_eq!(meter.documents, 1, "one parse");
    // Panes of one size want the same rasters, so they share them.
    let shown = h.state().documents().visible(panes[0]).unwrap_or(0..0);
    assert_eq!(meter.pages, shown.len() + 1, "one set of pages");
    h.state_mut().queue(Command::ClosePane(panes[1]));
    h.run();
    assert_eq!(h.state().documents().meter().documents, 1, "still read");
    Ok(())
}

fn open_appearance(h: &mut Harness<'static, MareaderApp>) {
    h.hover_at(egui::pos2(450.0, 20.0));
    h.run_steps(2);
    h.get_by_label("Appearance").click();
    h.run_steps(2);
}

#[test]
fn column_width_widens_a_book_s_pages_and_shows_only_for_books() {
    let mut h = reader(&[("epub", "sample:6")]);
    let pane = h.state().active_pane();
    let measure = |h: &Harness<'static, MareaderApp>| {
        let reading = h.state().documents().reading(pane);
        let width = reading
            .as_ref()
            .and_then(|r| r.document.page_sizes().first().map(|s| s.width));
        (
            width.unwrap_or(0.0),
            reading.map_or(0, |r| r.document.page_count()),
        )
    };
    let (width, pages) = measure(&h);
    h.state_mut().set_option("settings", "fonts").ok();
    h.run_steps(3);
    let slider = h.get_by_role_and_label(Role::Slider, "Column width");
    slider.scroll_to_me();
    h.run();
    h.state_mut().set_option("column_width", "140").ok();
    h.run();
    let (wider, fewer) = measure(&h);
    assert!(wider > width * 1.3, "{wider} vs {width}");
    assert!(fewer < pages, "{fewer} vs {pages}");
    // A PDF's pages are its own: no dial, and no Fonts tab.
    let mut h = reader(&[("pdf", "sample:3")]);
    h.state_mut().set_option("settings", "fonts").ok();
    h.run_steps(3);
    assert!(h.query_by_label("Column width").is_none());
    assert!(h.query_by_label("Fonts").is_none());
}

#[test]
fn a_books_page_and_column_rows_follow_its_view() {
    let mut h = reader(&[("epub", "sample:2"), ("settings", "fonts")]);
    h.run_steps(3);
    // The vertical stream keeps a column of its own.
    assert!(shows(&h, "Column width") && !shows(&h, "Longest line"));
    let single = mareader_ui::layout::ViewMode::Single;
    h.state_mut().queue(Command::View(single));
    h.run();
    assert!(shows(&h, "Longest line") && !shows(&h, "Column width"));
    // A PDF has neither, nor a book's overrides.
    let mut h = reader(&[("pdf", "sample:3"), ("settings", "fonts")]);
    h.run_steps(3);
    for label in ["Longest line", "Column width", "Override book font"] {
        assert!(!shows(&h, label), "{label}");
    }
}

#[test]
fn the_appearance_popover_offers_a_books_text_dials() {
    let switch = "Override book layout";
    let mut h = reader(&[("epub", "sample:2")]);
    h.state_mut().queue(Command::ShowAppearance);
    h.run_steps(3);
    h.get_by_role_and_label(Role::CheckBox, switch)
        .scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, switch).click();
    h.run();
    assert!(h.state().settings().text.override_layout);
    // A PDF's page has no book type to dial.
    let mut h = reader(&[("pdf", "sample:3")]);
    h.state_mut().queue(Command::ShowAppearance);
    h.run_steps(3);
    assert!(!shows(&h, switch));
}

/// Whether the open settings window shows a row labelled `label`.
fn shows(h: &Harness<'static, MareaderApp>, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

#[test]
fn layout_rows_follow_the_focused_document() {
    let mut h = reader(&[("pdf", "sample:3"), ("settings", "layout")]);
    h.run_steps(3);
    for label in [
        "No gap",
        "Scale my zoom with the pane",
        "Page shadow",
        "Page margin",
    ] {
        assert!(shows(&h, label), "a PDF has {label}");
    }
    // A book's vertical stream: no gaps, shadows or odd page sizes.
    let mut h = reader(&[("epub", "sample:2"), ("settings", "layout")]);
    h.run_steps(3);
    for label in ["No gap", "Scale my zoom with the pane", "Page shadow"] {
        assert!(!shows(&h, label), "a book's stream has no {label}");
    }
    assert!(shows(&h, "Page margin"));
    // Its single pages have gaps and shadows again.
    let single = mareader_ui::layout::ViewMode::Single;
    h.state_mut().queue(Command::View(single));
    h.run();
    assert!(shows(&h, "No gap") && shows(&h, "Page shadow"));
    assert!(!shows(&h, "Scale my zoom with the pane"));
}

#[test]
fn pane_text_shows_only_while_split_with_a_book() {
    let label = "Independent text for each pane";
    let options = [("panes", "2"), ("focus", "2"), ("pdf", "sample:3")];
    let mut h = reader(&options);
    h.state_mut().set_option("settings", "panes").ok();
    h.run_steps(3);
    assert!(!shows(&h, label), "PDFs alone have no text settings");
    // A book in the other pane brings it back; opens land where focus is.
    h.state_mut().set_option("focus", "1").ok();
    h.state_mut().set_option("epub", "sample:2").ok();
    h.run();
    h.state_mut().set_option("focus", "2").ok();
    h.run();
    assert!(!h.state().settings_scope().book());
    assert!(shows(&h, label));
    let mut h = reader(&[("epub", "sample:2"), ("settings", "panes")]);
    h.run_steps(3);
    assert!(!shows(&h, label), "one pane has nothing to keep apart");
}

#[test]
fn the_fonts_tab_switches_a_book_s_pictures_to_the_theme() {
    let mut h = reader(&[("epub", "sample:2")]);
    h.state_mut().set_option("settings", "fonts").ok();
    h.run_steps(3);
    let switch = "Theme pictures";
    h.get_by_role_and_label(Role::CheckBox, switch)
        .scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, switch).click();
    h.run_steps(2);
    assert!(h.state().settings().text.theme_pictures);
}

#[test]
fn the_fonts_tab_switches_a_book_s_emoji_to_the_theme() {
    let mut h = reader(&[("epub", "sample:2")]);
    h.state_mut().set_option("settings", "fonts").ok();
    h.run_steps(3);
    let switch = "Theme emoji";
    h.get_by_role_and_label(Role::CheckBox, switch)
        .scroll_to_me();
    h.run();
    assert!(!h.state().settings().text.theme_emojis);
    h.get_by_role_and_label(Role::CheckBox, switch).click();
    h.run_steps(2);
    assert!(h.state().settings().text.theme_emojis);
}

#[test]
fn a_book_offers_the_ink_dial_in_the_popover_and_the_fonts_tab() {
    let mut h = reader(&[("epub", "sample:2")]);
    open_appearance(&mut h);
    assert!(
        h.query_all_by_label("Text ink").next().is_some(),
        "the popover's dial"
    );
    h.key_press(egui::Key::Escape);
    h.run();
    h.state_mut().set_option("settings", "fonts").ok();
    h.run_steps(3);
    let less = "Less text ink";
    h.get_by_role_and_label(Role::Button, less).scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::Button, less).click();
    h.run_steps(2);
    assert_eq!(h.state().settings().text.ink_contrast, 95.0);
    // A PDF's ink is its own: no dial.
    let mut h = reader(&[("pdf", "sample:3")]);
    open_appearance(&mut h);
    assert!(h.query_all_by_label("Strength").next().is_some());
    assert!(h.query_all_by_label("Text ink").next().is_none());
}

fn press(h: &mut Harness<'static, MareaderApp>, pos: egui::Pos2) {
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run();
}

#[test]
fn a_press_on_a_pane_page_focuses_that_pane() {
    let mut h = reader(&[("panes", "2"), ("focus", "2"), ("pdf", "sample:5")]);
    h.state_mut().set_option("focus", "1").ok();
    h.state_mut().set_option("pdf", "sample:5").ok();
    h.run();
    let rects = h.state().pane_rects().to_vec();
    assert_eq!(rects.len(), 2, "{rects:?}");
    let (first, (second, right)) = (rects[0].0, rects[1]);
    assert_eq!(h.state().active_pane(), first);
    // Mid-pane is on the page, whose own widgets take the press too.
    press(&mut h, right.center());
    assert_eq!(h.state().active_pane(), second);
}

fn raven() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    dir.join("../epub/tests/books/the-raven-gutenberg-17192.epub")
}

#[test]
fn a_book_set_anew_keeps_the_line_read_on_top() {
    let mut h = reader(&[]);
    let pane = h.state().active_pane();
    let open = OpenRequest {
        pane: Some(pane),
        ..OpenRequest::path(raven())
    };
    h.state_mut().queue(Command::Open(open));
    h.run();
    let book =
        |h: &Harness<'static, MareaderApp>| h.state().documents().reading(pane).expect("a book");
    // Into the poem, to a line partway down a page, not a picture.
    h.hover_at(egui::pos2(450.0, 400.0));
    let mut before = None;
    for _ in 0..80 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -170.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run();
        let read = h.state().documents().place_read(pane);
        let page = read.and_then(|at| book(&h).document.locate(at));
        // A re-set must not return to the page's start.
        let start = page.and_then(|p| book(&h).document.anchor(p));
        if read.is_some() && start < read {
            before = read;
            break;
        }
    }
    let before = before.expect("a line mid-page");
    let was = book(&h).doc;
    h.state_mut().set_option("text_size", "28").ok();
    h.run();
    assert_ne!(book(&h).doc, was, "set anew");
    let now = book(&h).document.locate(before).expect("on a page");
    let shown = h.state().documents().visible(pane).unwrap_or(0..0);
    assert!(shown.contains(&now), "{now} in {shown:?}");
    // Landed on that line: it stays the place read, without drift.
    assert_eq!(h.state().documents().place_read(pane), Some(before));
    h.state_mut().set_option("text_size", "17").ok();
    h.run();
    assert_eq!(
        h.state().documents().place_read(pane),
        Some(before),
        "and back"
    );
}

#[test]
fn independent_text_sets_each_pane_in_its_own_type() {
    let mut h = reader(&[("panes", "2"), ("independent_text", "true")]);
    let panes = h.state().panes();
    let (a, b) = (panes[0], panes[1]);
    for pane in [a, b] {
        let open = OpenRequest {
            pane: Some(pane),
            ..OpenRequest::path(raven())
        };
        h.state_mut().queue(Command::Open(open));
        h.run();
    }
    let doc =
        |h: &Harness<'static, MareaderApp>, p| h.state().documents().reading(p).map(|r| r.doc);
    let pages = |h: &Harness<'static, MareaderApp>, p| h.state().documents().page_count(p);
    assert!(doc(&h, a).is_some());
    assert_eq!(doc(&h, a), doc(&h, b), "one file, one type: one parse");
    let (before, size) = (pages(&h, a), h.state().settings().text.font_size);
    // The Fonts settings change only the focused pane.
    h.state_mut().set_option("focus", "2").ok();
    h.state_mut().set_option("text_size", "28").ok();
    h.run();
    assert_ne!(doc(&h, a), doc(&h, b));
    assert_eq!(pages(&h, a), before);
    assert!(pages(&h, b) > before, "{:?} > {before:?}", pages(&h, b));
    assert_eq!(h.state().text(a).font_size, size);
    assert_eq!(h.state().text(b).font_size, 28.0);
    // Column Width is a Fonts setting, so it is the pane's own too.
    h.state_mut().set_option("column_width", "140").ok();
    h.run();
    assert_eq!(h.state().text(a).column_width_pct, 100.0);
    assert_eq!(h.state().text(b).column_width_pct, 140.0);
    // Off, the focused pane's text is everyone's, and they share again.
    h.state_mut().set_option("independent_text", "false").ok();
    h.run();
    assert_eq!(h.state().settings().text.font_size, 28.0);
    assert_eq!(doc(&h, a), doc(&h, b));
    assert_eq!(h.state().documents().stats().0, 1, "the other type closed");
}

#[test]
fn an_empty_pane_offers_the_books_read_lately() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../epub/tests/books");
    let raven = dir.join("the-raven-gutenberg-17192.epub");
    let raven = raven.canonicalize().unwrap_or(raven);
    let path = raven.to_str().unwrap_or_default();
    let mut h = reader(&[("epub", path), ("panes", "2")]);
    let title = h.state().library().recent()[0].title.value.clone();
    let reopen = format!("Reopen {title}");
    h.get_by_label(&reopen).click();
    h.run();
    assert!(h.query_by_label(&reopen).is_none(), "the pane took it");
    assert_eq!(h.state().panes().len(), 2);
}
