//! `memory BOOK [ROUNDS] | --library N | --seeded N | --markdown N`.

// Linux reads `/proc/self/status`; macOS, its footprint. See `docs/memory.md`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use egui_kittest::Harness;
use mareader_engine::documents::OpenRequest;
use mareader_engine::library::{Action, SearchAction};
use mareader_engine::{command::Command, host::Machine};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::pages::{CAP_BYTES, REST_BYTES};
use mareader_ui_egui::ui_command::{Sent, UiCommand};
use mareader_workspace::{PaneId, SplitAxis};

type App = Harness<'static, MareaderApp>;

const MIB: f64 = 1024.0 * 1024.0;
/// The library's cover textures, as `covers.rs` holds them.
const COVER_BUDGET: usize = 64 << 20;
/// How far the later rounds' floor may rise over the earlier rounds' top.
const ROUND_SLACK: f64 = 16.0;
/// What each book added to the library may hold, file or not.
const PER_BOOK_KIB: f64 = 16.0;
/// What scrolling a big library may add at its peak, covers included.
const SCROLL_PEAK: f64 = 256.0;
/// Laid blocks a resting book keeps, as `flow.rs` holds them.
const REST_LAID: usize = 4 << 20;

/// Resident MiB and their peak since the last reset.
fn resident() -> (f64, f64) {
    #[cfg(target_os = "macos")]
    {
        // The footprint's peak can't be reset, so each row's is its own.
        let now = footprint().unwrap_or(0.0);
        (now, now)
    }
    #[cfg(not(target_os = "macos"))]
    (
        status("VmRSS:").unwrap_or(0.0),
        status("VmHWM:").unwrap_or(0.0),
    )
}

/// What Activity Monitor shows, in MiB, from the `footprint` tool.
#[cfg(target_os = "macos")]
fn footprint() -> Option<f64> {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("footprint")
        .arg(&pid)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut words = text.split_whitespace().skip_while(|w| *w != "Footprint:");
    let n: f64 = words.nth(1)?.parse().ok()?;
    match words.next()? {
        "KB" => Some(n / 1024.0),
        "MB" => Some(n),
        "GB" => Some(n * 1024.0),
        _ => None,
    }
}

/// A `/proc/self/status` field, in MiB.
#[cfg(not(target_os = "macos"))]
fn status(field: &str) -> Option<f64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = text.lines().find(|l| l.starts_with(field))?;
    let kib: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib / 1024.0)
}

/// Forgets the peak so far, so the next one is the next step's own.
fn reset_peak() {
    std::fs::write("/proc/self/clear_refs", "5").ok();
}

/// One row's numbers, kept for the budgets.
#[derive(Clone, Copy, Debug, Default)]
struct Row {
    rss: f64,
    peak: f64,
    documents: usize,
    pages: usize,
    page_bytes: usize,
    texts: usize,
    jobs: usize,
    searches: usize,
    cover_bytes: usize,
    /// Set books alive, their laid blocks and decoded pictures.
    flows: usize,
    laid: usize,
    decoded: usize,
}

/// Every row so far, and the budgets checked against them.
#[derive(Default)]
struct Audit {
    rows: Vec<(String, Row)>,
    budgets: Vec<(String, String, bool)>,
}

impl Audit {
    fn header() {
        println!(
            "| Step | RSS MiB | Peak MiB | Docs | Pages (MiB) | Text layers | Jobs | Searches \
             | Covers MiB | egui textures | Flows (laid, decoded MiB) |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|---|");
    }

    fn row(&mut self, h: &mut App, step: &str) -> Row {
        h.render().ok();
        let app = h.state();
        let m = app.documents().meter();
        let held = mareader_reflow::held();
        let (rss, peak) = resident();
        let row = Row {
            rss,
            peak,
            documents: m.documents,
            pages: m.pages,
            page_bytes: m.page_bytes,
            texts: m.texts,
            jobs: m.jobs,
            searches: app.searches(),
            cover_bytes: app.library_state().covers().bytes(),
            flows: held.flows,
            laid: held.laid,
            decoded: held.decoded,
        };
        let textures = h.ctx.tex_manager().read().num_allocated();
        let peak = row.peak;
        println!(
            "| {step} | {:.0} | {peak:.0} | {} | {} ({:.0}) | {} | {} | {} | {:.1} | {textures} \
             | {} ({:.1}, {:.1}) |",
            row.rss,
            row.documents,
            row.pages,
            row.page_bytes as f64 / MIB,
            row.texts,
            row.jobs,
            row.searches,
            row.cover_bytes as f64 / MIB,
            row.flows,
            row.laid as f64 / MIB,
            row.decoded as f64 / MIB,
        );
        reset_peak();
        self.rows.push((step.to_owned(), row));
        row
    }

    fn check(&mut self, budget: &str, value: String, held: bool) {
        self.budgets.push((budget.to_owned(), value, held));
    }

    /// The caps every row must keep, whatever the workload.
    fn caps(&mut self) {
        let pages = self.rows.iter().map(|(_, r)| r.page_bytes).max();
        let covers = self.rows.iter().map(|(_, r)| r.cover_bytes).max();
        let (pages, covers) = (pages.unwrap_or(0), covers.unwrap_or(0));
        let value = format!("{:.0} MiB at most", pages as f64 / MIB);
        self.check(
            "Page rasters within their 256 MiB cap",
            value,
            pages <= CAP_BYTES,
        );
        let value = format!("{:.1} MiB at most", covers as f64 / MIB);
        let held = covers <= COVER_BUDGET;
        self.check("Cover textures within their 64 MiB budget", value, held);
    }

    /// Prints the budgets; true when every one held.
    fn report(mut self) -> bool {
        self.caps();
        println!();
        println!("| Budget | Measured | Held |");
        println!("|---|---|---|");
        for (budget, value, held) in &self.budgets {
            let word = if *held { "yes" } else { "**no**" };
            println!("| {budget} | {value} | {word} |");
        }
        self.budgets.iter().all(|(_, _, held)| *held)
    }
}

fn harness(app: MareaderApp) -> App {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .wgpu()
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn open(h: &mut App, pane: PaneId, path: &Path) {
    let request = OpenRequest {
        pane: Some(pane),
        ..OpenRequest::path(path.to_path_buf())
    };
    h.state_mut().queue(Sent::Engine(Command::Open(request)));
    h.run();
}

/// Every page in turn, each landing at once and drawn.
fn read_through(h: &mut App, pane: PaneId) {
    let count = h.state().documents().page_count(pane).unwrap_or(0);
    for page in 0..count {
        h.state_mut()
            .queue(Sent::Engine(Command::GoTo { pane, page }));
        h.run();
        h.render().ok();
    }
}

/// The find bar's search over every page, answered on this thread.
fn search(h: &mut App, query: &str) {
    h.state_mut().set_option("find", query).ok();
    h.state_mut().queue(Sent::Ui(UiCommand::ShowFind));
    h.run_steps(30);
}

fn close(h: &mut App, pane: PaneId) {
    h.state_mut().queue(Sent::Engine(Command::ClosePane(pane)));
    h.run();
}

/// Nothing the app counts is left of a closed book.
fn emptied(r: &Row) -> bool {
    r.documents + r.pages + r.texts + r.jobs + r.searches + r.flows == 0
}

/// A book set again in new text sizes, then in new window sizes.
fn retype(h: &mut App, audit: &mut Audit) {
    // A PDF has nothing to set again.
    if mareader_reflow::held().flows == 0 {
        return;
    }
    let was = h.state().settings().text.font_size;
    for size in [14.0, 18.0, 22.0, 26.0, 30.0, was] {
        let mut s = h.state().settings().clone();
        s.text.font_size = size;
        h.state_mut()
            .queue(Sent::Engine(Command::SetSettings(Box::new(s))));
        h.run();
        h.render().ok();
    }
    let sized = audit.row(h, "set again in six text sizes");
    for (w, ht) in [
        (1000.0, 700.0),
        (1400.0, 900.0),
        (1100.0, 760.0),
        (1200.0, 800.0),
    ] {
        h.set_size(egui::vec2(w, ht));
        h.run();
        h.render().ok();
    }
    audit.row(h, "and in four window sizes");
    settle(h);
    let rest = audit.row(h, "a second later");
    // Old sets die with their last pane; only the one shown is kept.
    let value = format!("{} after sizes, {} at rest", sized.flows, rest.flows);
    let held = sized.flows <= 1 && rest.flows <= 1;
    audit.check("Re-set books keep only the set shown", value, held);
}

/// Four quiet seconds, past the three after which caches rest, then drawn.
fn rest(h: &mut App) {
    h.run_steps(240);
    settle(h);
    // Frames here outrun the clock; the freed heap goes back a second on.
    std::thread::sleep(Duration::from_millis(1500));
}

/// A second's frames, drawn, so the GPU's deferred frees can land.
fn settle(h: &mut App) {
    for _ in 0..6 {
        h.run_steps(10);
        h.render().ok();
    }
}

/// A leak raises every later round; memory kept for reuse only spikes.
fn floor_rise(rounds: &[Row]) -> f64 {
    let (early, late) = rounds.split_at(rounds.len() / 2);
    let top = early.iter().map(|r| r.rss).fold(f64::MIN, f64::max);
    let floor = late.iter().map(|r| r.rss).fold(f64::MAX, f64::min);
    floor - top
}

/// Reads, searches and closes `path` in a new pane, `rounds` times.
fn rounds(h: &mut App, audit: &mut Audit, path: &Path, rounds: usize) {
    let mut kept = Vec::new();
    for round in 2..=rounds.max(2) {
        h.state_mut()
            .queue(Sent::Engine(Command::Split(SplitAxis::Horizontal)));
        h.run();
        let pane = h.state().active_pane();
        open(h, pane, path);
        read_through(h, pane);
        search(h, "the");
        close(h, pane);
        kept.push(audit.row(h, &format!("round {round}: read, searched, closed")));
    }
    if kept.len() >= 2 {
        let rise = floor_rise(&kept);
        let value = format!("{rise:+.0} MiB over rounds 2 to {}", rounds.max(2));
        let held = rise <= ROUND_SLACK;
        audit.check("Later rounds' floor stays level (no leak)", value, held);
    }
    let all = kept.iter().all(emptied);
    audit.check("Every round closes to nothing", "see rows".into(), all);
}

fn book(path: &Path, count: usize) -> Result<bool, String> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0")?;
    app.set_option("panes", "2")?;
    let mut s = app.settings().clone();
    // Jumps land at once: the read-through is about pages, not glides.
    s.motion.scroll_jumps = false;
    app.queue(Sent::Engine(Command::SetSettings(Box::new(s))));
    let mut h = harness(app);
    let mut audit = Audit::default();
    Audit::header();
    let idle = audit.row(&mut h, "idle, two empty panes");
    let panes = h.state().panes();
    let (left, right) = (panes[0], panes[1]);
    open(&mut h, right, path);
    let first = audit.row(&mut h, "opened in one pane");
    read_through(&mut h, right);
    audit.row(&mut h, "every page read");
    search(&mut h, "the");
    audit.row(&mut h, "searched for \"the\"");
    rest(&mut h);
    let rested = audit.row(&mut h, "left alone four seconds");
    let value = format!(
        "{:.0} MiB of pages, {:.1} MiB laid",
        rested.page_bytes as f64 / MIB,
        rested.laid as f64 / MIB
    );
    let held = rested.page_bytes <= REST_BYTES && rested.laid <= REST_LAID;
    audit.check("At rest, only the pages in view stay", value, held);
    retype(&mut h, &mut audit);
    // The page grid follows the pane read, picks and all.
    h.state_mut().set_option("sidebar", "thumbnails").ok();
    h.run();
    audit.row(&mut h, "thumbnails shown");
    close(&mut h, right);
    let closed = audit.row(&mut h, "its pane closed");
    settle(&mut h);
    audit.row(&mut h, "a second later");
    h.state_mut().set_option("sidebar", "closed").ok();
    let value = format!(
        "{} documents, {} searches",
        closed.documents, closed.searches
    );
    audit.check(
        "A closed book leaves nothing counted",
        value,
        emptied(&closed),
    );
    rounds(&mut h, &mut audit, path, count);
    // The same book twice: the second pane should cost nearly nothing.
    h.state_mut().queue(Sent::Engine(Command::FocusPane(left)));
    h.state_mut()
        .queue(Sent::Engine(Command::Split(SplitAxis::Horizontal)));
    h.run();
    let panes = h.state().panes();
    open(&mut h, panes[0], path);
    let once = audit.row(&mut h, "opened again");
    open(&mut h, panes[1], path);
    let twice = audit.row(&mut h, "and in the second pane");
    let shared = twice.documents == once.documents && twice.pages <= once.pages + 1;
    // A second parse would cost about what the first open did.
    let (grew, cost) = (twice.rss - once.rss, first.rss - idle.rss);
    let value = format!("{grew:+.0} MiB of the first open's {cost:.0}");
    let held = shared && grew <= cost / 4.0;
    audit.check(
        "A second pane shares the book (under a quarter)",
        value,
        held,
    );
    Ok(audit.report())
}

/// Writes books `from..to`, an hour old so no scan waits for them.
fn write_books(dir: &Path, from: usize, to: usize) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let old = SystemTime::now() - Duration::from_secs(3_600);
    for n in from..to {
        let path = dir.join(format!("book-{n:05}.pdf"));
        let pdf = mareader_pdf::sample::pdf(1, &format!("Book {n}"));
        std::fs::write(&path, pdf).map_err(|e| e.to_string())?;
        let file = std::fs::File::options().write(true).open(&path);
        file.and_then(|f| f.set_modified(old))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Wheels down the page until `last` shows, or `bound` turns.
fn scroll_to(h: &mut App, last: Option<&str>, bound: usize) -> usize {
    use egui_kittest::kittest::Queryable as _;
    h.hover_at(egui::pos2(600.0, 450.0));
    for turn in 0..bound {
        if last.is_some_and(|l| h.query_all_by_label(l).next().is_some()) {
            return turn;
        }
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(2);
        // Like a real frame, a render submits the uploads queued so far.
        if turn % 4 == 3 {
            h.render().ok();
        }
    }
    bound
}

/// The title the page labels the book from `file` with.
fn title_of(h: &App, file: &str) -> String {
    let lib = h.state().library();
    let mut books = lib.books();
    let hit = books.find(|b| b.path().is_some_and(|p| p.ends_with(file)));
    hit.map(|b| b.title.value.clone()).unwrap_or_default()
}

/// Covers are left out: they keep to a budget of their own.
fn per_book(audit: &mut Audit, before: &Row, after: &Row, count: usize) {
    let covers = (after.cover_bytes as f64 - before.cover_bytes as f64) / MIB;
    let kib = (after.rss - before.rss - covers) * 1024.0 / count.max(1) as f64;
    let value = format!("{kib:.1} KiB a book, {count} more");
    let held = kib <= PER_BOOK_KIB;
    audit.check("Each book's share of the library", value, held);
}

fn scroll_peak(audit: &mut Audit, view: &str, rest: &Row, done: &Row) {
    let rise = done.peak - rest.rss;
    let value = format!("+{rise:.0} MiB at its peak");
    let budget = format!("Scrolling the {view} to the end, covers and all");
    audit.check(&budget, value, rise <= SCROLL_PEAK);
}

/// Searches the page, then lets it go.
fn library_search(h: &mut App, audit: &mut Audit, text: &str) {
    let query = SearchAction::Text(text.to_owned());
    h.state_mut()
        .queue(Sent::Engine(Command::Library(Action::Search(query))));
    h.run_steps(4);
    audit.row(h, &format!("searched \"{text}\""));
    let clear = Sent::Engine(Command::Library(Action::Search(SearchAction::Clear)));
    h.state_mut().queue(clear);
    h.run_steps(4);
}

/// Reads `root` in place, waiting until the library holds `total`.
fn add_folder(h: &mut App, root: &Path, total: usize) {
    let opts = mareader_library::Options {
        min_size: 0,
        ..mareader_library::Options::default()
    };
    let root = root.to_path_buf();
    let add = Sent::Engine(Command::Library(Action::ImportFolder { root, opts }));
    h.state_mut().queue(add);
    for _ in 0..200 {
        h.run_steps(2);
        if h.state().library().len() >= total {
            break;
        }
    }
}

fn library(count: usize) -> Result<bool, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    // By the third folder, the store cache and allocator are paid for.
    let (a, b) = (count / 3, count * 2 / 3);
    let names = ["One", "Two", "Three"].map(|n| dir.path().join(n));
    let [one, two, three] = &names;
    write_books(one, 0, a)?;
    write_books(two, a, b)?;
    write_books(three, b, count)?;
    let mut app = MareaderApp::wired(Machine::library(dir.path().join("library")));
    app.set_option("workers", "0")?;
    app.set_option("library", "show")?;
    let mut h = harness(app);
    let mut audit = Audit::default();
    Audit::header();
    audit.row(&mut h, "library page, empty");
    add_folder(&mut h, one, a);
    audit.row(&mut h, &format!("{a} books read in place"));
    add_folder(&mut h, two, b);
    let second = audit.row(&mut h, &format!("{} more, from a second folder", b - a));
    add_folder(&mut h, three, count);
    let found = h.state().library().len();
    let more = count - b;
    let added = audit.row(&mut h, &format!("{more} more, from a third"));
    let value = format!("{found} of {count}");
    audit.check("Every book is found", value, found == count);
    per_book(&mut audit, &second, &added, more);
    let last = title_of(&h, &format!("book-{:05}.pdf", count.saturating_sub(1)));
    if last.is_empty() {
        return Err("the last book has no title".into());
    }
    let turns = scroll_to(&mut h, Some(&last), 4_000);
    let grid = audit.row(&mut h, &format!("grid scrolled to the end ({turns} turns)"));
    scroll_peak(&mut audit, "grid", &added, &grid);
    library_search(&mut h, &mut audit, "Book 12");
    h.state_mut().set_option("layout", "list")?;
    h.run_steps(4);
    let rest = audit.row(&mut h, "list shown");
    let turns = scroll_to(&mut h, Some(&last), 8_000);
    let list = audit.row(&mut h, &format!("list scrolled to the end ({turns} turns)"));
    scroll_peak(&mut audit, "list", &rest, &list);
    h.state_mut().queue(Sent::Ui(UiCommand::ShowLibrary));
    h.run_steps(4);
    audit.row(&mut h, "back to reading");
    Ok(audit.report())
}

fn seeded(count: usize) -> Result<bool, String> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0")?;
    let mut h = harness(app);
    let mut audit = Audit::default();
    Audit::header();
    audit.row(&mut h, "reader, no library");
    let half = count / 2;
    h.state_mut().set_option("books", &half.to_string())?;
    h.state_mut().set_option("library", "show")?;
    h.run_steps(4);
    let before = audit.row(&mut h, &format!("{half} books, page shown"));
    h.state_mut()
        .set_option("books", &(count - half).to_string())?;
    h.run_steps(4);
    let after = audit.row(&mut h, &format!("{} more", count - half));
    per_book(&mut audit, &before, &after, count - half);
    // Its titles repeat, so the end is a bound: about a row a turn.
    let turns = scroll_to(&mut h, None, count / 6 + 10);
    audit.row(&mut h, &format!("grid scrolled ({turns} turns)"));
    library_search(&mut h, &mut audit, "Raven");
    Ok(audit.report())
}

fn count(arg: Option<String>) -> Result<usize, String> {
    let text = arg.ok_or("give a count of books")?;
    text.parse().map_err(|_| format!("not a count: {text}"))
}

/// Wheels `turns` times one way, drawing as frames do.
fn wheel(h: &mut App, turns: usize, down: bool) {
    h.hover_at(egui::pos2(900.0, 450.0));
    let dy = if down { -300.0 } else { 300.0 };
    for turn in 0..turns {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, dy),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(3);
        if turn % 2 == 1 {
            h.render().ok();
        }
    }
}

/// A six-page README of emoji, scrolled down and up `passes` times.
fn markdown(passes: usize) -> Result<bool, String> {
    let dir = std::env::temp_dir().join("mareader-memory-md");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("emoji.md");
    let text = mareader_epub::sample::markdown(6, true);
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    let mut app = MareaderApp::default();
    // Real workers, as the app has: scrolling races their renders.
    app.set_option("workers", "4")?;
    app.set_option("panes", "2")?;
    // Its headings, emoji and all, drawn as egui text.
    app.set_option("sidebar", "outline")?;
    let mut h = harness(app);
    let mut audit = Audit::default();
    Audit::header();
    audit.row(&mut h, "idle, two empty panes");
    let right = h.state().panes()[1];
    let request = OpenRequest {
        pane: Some(right),
        ..OpenRequest::path(path.clone())
    };
    h.state_mut().queue(Sent::Engine(Command::Open(request)));
    // Workers open it while the loading mark spins: step, don't run.
    for _ in 0..600 {
        if h.state().documents().page_count(right).is_some() {
            break;
        }
        h.run_steps(5);
        std::thread::sleep(Duration::from_millis(10));
    }
    settle(&mut h);
    audit.row(&mut h, "opened in one pane");
    let mut kept = Vec::new();
    for pass in 1..=passes.max(2) {
        wheel(&mut h, 40, true);
        wheel(&mut h, 40, false);
        settle(&mut h);
        kept.push(audit.row(&mut h, &format!("pass {pass}: scrolled down and up")));
    }
    let rise = floor_rise(&kept);
    let value = format!("{rise:+.0} MiB over {} passes", kept.len());
    audit.check(
        "Scrolling's floor stays level (no leak)",
        value,
        rise <= ROUND_SLACK,
    );
    close(&mut h, right);
    settle(&mut h);
    let closed = audit.row(&mut h, "its pane closed, a second later");
    let value = format!("{} documents, {} jobs", closed.documents, closed.jobs);
    audit.check(
        "A closed book leaves nothing counted",
        value,
        emptied(&closed),
    );
    Ok(audit.report())
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .ok_or("usage: memory BOOK [ROUNDS] | --library N | --seeded N | --markdown N")?;
    let held = match first.as_str() {
        "--markdown" => markdown(count(args.next())?)?,
        "--library" => library(count(args.next())?)?,
        "--seeded" => seeded(count(args.next())?)?,
        _ => {
            let rounds = args.next().and_then(|n| n.parse().ok()).unwrap_or(4);
            book(&PathBuf::from(first), rounds)?
        }
    };
    if !held {
        std::process::exit(1);
    }
    Ok(())
}
