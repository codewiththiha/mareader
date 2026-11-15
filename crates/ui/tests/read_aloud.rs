//! Reading aloud a book with no narration: a voice, a sentence at a time.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_speech::Spot;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::park::PEEK;
use mareader_ui::read_aloud::{ReadFrom, Span};

use crate::common::press;

/// A sample PDF, read by the engine that says nothing.
fn reader(pages: usize) -> Harness<'static, MareaderApp> {
    reader_sized(pages, egui::vec2(900.0, 700.0))
}

/// What the app holds in its library, for picking books.
fn library<'a>(h: &'a Harness<'static, MareaderApp>) -> &'a mareader_library::Library {
    h.state().library()
}

fn reader_sized(pages: usize, size: egui::Vec2) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("pdf", &format!("sample:{pages}")).ok();
    app.set_option("set", "read_aloud.engine:\"silent\"").ok();
    let mut h = Harness::builder()
        .with_size(size)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

/// Starts reading, waiting for the pages' thread to light a sentence.
fn start(h: &mut Harness<'static, MareaderApp>, from: ReadFrom) {
    let pane = h.state().active_pane();
    h.state_mut().queue(Command::ReadAloud { pane, from });
    step_until(h, |h| lit(h).is_some());
}

/// Steps until `done`, giving the pages' thread time between frames.
fn step_until(
    h: &mut Harness<'static, MareaderApp>,
    done: impl Fn(&Harness<'static, MareaderApp>) -> bool,
) {
    for _ in 0..600 {
        h.run_steps(1);
        if done(h) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// Where the sentence lit now starts and ends.
fn lit(h: &Harness<'static, MareaderApp>) -> Option<(Spot, Spot)> {
    let pane = h.state().active_pane();
    match h.state().read_aloud().lit(pane)?.span {
        Span::Chars { start, end } => Some((start, end)),
        Span::Anchors { .. } => None,
    }
}

#[test]
fn sentences_are_lit_one_after_another_and_pages_follow() {
    let mut h = reader(2);
    start(&mut h, ReadFrom::default());
    let first = lit(&h).expect("the first sentence is lit");
    assert_eq!(first.0.page, 0);
    assert!(first.0 < first.1);
    let _ = h.get_by_label("Pause");
    let _ = h.get_by_label("Next sentence");
    step_until(&mut h, |h| lit(h) != Some(first));
    let second = lit(&h).expect("the next sentence is lit");
    assert!(second.0 >= first.1, "{second:?} comes after {first:?}");
    // Read on to the second page; the pane turns with it.
    step_until(&mut h, |h| lit(h).is_none_or(|(s, _)| s.page > 0));
    assert_eq!(lit(&h).map(|(s, _)| s.page), Some(1));
    h.run_steps(5);
    let pane = h.state().active_pane();
    assert_eq!(h.state().documents().current(pane), Some(1));
}

#[test]
fn pause_holds_the_sentence_and_previous_steps_back() {
    let mut h = reader(1);
    start(&mut h, ReadFrom::default());
    let first = lit(&h);
    step_until(&mut h, |h| lit(h) != first);
    h.get_by_label("Pause").click();
    // The voice may move on in the frame the click lands.
    h.run_steps(2);
    let _ = h.get_by_label("Play");
    let before = lit(&h);
    h.run_steps(6);
    assert_eq!(lit(&h), before, "paused, the same sentence stays lit");
    h.get_by_label("Previous sentence").click();
    h.run_steps(1);
    let back = lit(&h);
    assert!(back < before, "{back:?} is before {before:?}");
    h.get_by_label("Play").click();
    step_until(&mut h, |h| lit(h) > back);
    assert!(lit(&h) > back, "playing goes on");
}

#[test]
fn reading_starts_at_the_character_asked_for() {
    let mut h = reader(2);
    let at = Spot { page: 1, char: 0 };
    let from = ReadFrom {
        anchor: None,
        at: Some(at),
        until: None,
    };
    start(&mut h, from);
    assert_eq!(lit(&h).map(|(s, _)| s), Some(at));
}

#[test]
fn a_selection_is_read_alone_and_again_from_its_start() {
    let mut h = reader(2);
    let (at, until) = (Spot { page: 0, char: 0 }, Spot { page: 0, char: 120 });
    let from = ReadFrom {
        anchor: None,
        at: Some(at),
        until: Some(until),
    };
    start(&mut h, from);
    let first = lit(&h);
    assert_eq!(first.map(|(s, _)| s), Some(at));
    // Each sentence lit ends by the selection's end, then reading stops.
    for _ in 0..600 {
        h.run_steps(1);
        let Some((_, end)) = lit(&h) else {
            break;
        };
        assert!(end <= until, "{end:?} is past {until:?}");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    h.run_steps(1);
    let _ = h.get_by_label("Selection read");
    // Play again reads the selection again, not the book.
    h.get_by_label("Play").click();
    step_until(&mut h, |h| lit(h).is_some());
    assert_eq!(lit(&h), first);
}

#[test]
fn the_selection_bar_reads_just_the_selection() {
    let mut h = reader(1);
    let pane = h.state().active_pane();
    let text = "The quick brown fox jumps over the lazy dog.".to_owned();
    h.state_mut().queue(Command::SelectText {
        pane,
        page: 0,
        text,
    });
    h.run();
    h.get_by_label("Read aloud").click();
    step_until(&mut h, |h| lit(h).is_some());
    assert!(h.state().documents().selection(pane).is_none());
    let (start, _) = lit(&h).expect("the selection is lit");
    assert!(
        start.char > 100,
        "{start:?} is the selection's, not the page's"
    );
}

#[test]
fn the_reading_ends_and_says_so() {
    let mut h = reader(1);
    start(&mut h, ReadFrom::default());
    step_until(&mut h, |h| lit(h).is_none());
    h.run_steps(1);
    let _ = h.get_by_label("The end");
    h.get_by_label("Stop reading aloud").click();
    h.run_steps(2);
    assert!(!h.state().read_aloud().active());
}

#[test]
fn a_sentence_below_the_fold_is_brought_into_view() {
    let mut h = reader_sized(1, egui::vec2(900.0, 360.0));
    start(&mut h, ReadFrom::default());
    step_until(&mut h, |h| lit(h).is_none_or(|(s, _)| s.char > 250));
    let (start, end) = lit(&h).expect("a sentence far down is lit");
    h.get_by_label("Pause").click();
    h.run_steps(40);
    let pane = h.state().active_pane();
    let rects = h.state().pane_rects().to_vec();
    let (_, shown) = rects
        .into_iter()
        .find(|(p, _)| *p == pane)
        .expect("the pane");
    let chars = start.char..end.char.max(start.char + 1);
    let at = h
        .state()
        .documents()
        .hit_box(pane, 0, chars)
        .expect("drawn");
    assert!(shown.contains_rect(at), "{at:?} shows within {shown:?}");
}

#[test]
fn the_player_shows_time_left_and_seeks_to_a_page() {
    let mut h = reader(3);
    // Wide enough for the time, which the narrowest leaves out.
    h.state_mut().settings_mut().read_aloud.player_width = Some(560.0);
    start(&mut h, ReadFrom::default());
    h.run_steps(2);
    let _ = h.get_by_label_contains("left");
    // The track's middle is the second of three pages.
    h.get_by_label("Reading position").click();
    step_until(&mut h, |h| lit(h).is_some_and(|(s, _)| s.page == 1));
    assert_eq!(lit(&h).map(|(s, _)| (s.page, s.char)), Some((1, 0)));
    h.run_steps(3);
    let pane = h.state().active_pane();
    assert_eq!(
        h.state().documents().current(pane),
        Some(1),
        "the pane turns"
    );
    // Seeking while paused stays paused.
    h.get_by_label("Pause").click();
    h.run_steps(2);
    h.get_by_label("Reading position").click();
    h.run_steps(3);
    assert!(!h.state().read_aloud().playing());
    let _ = h.get_by_label("Play");
}

/// The reader with settings open on the Read aloud tab.
fn voice_settings(pages: usize) -> Harness<'static, MareaderApp> {
    let mut h = reader(pages);
    let tab = mareader_ui::settings_window::Tab::ReadAloud;
    h.state_mut().queue(Command::OpenSettings(tab));
    h.run_steps(4);
    h
}

#[test]
fn the_tab_lists_the_engine_voices_and_picks_one() {
    let mut h = voice_settings(1);
    assert!(h.state().read_aloud().booth.is_open());
    // The list starts folded, its pick named on the fold.
    assert!(h.query_by_label("System default").is_none());
    press(&mut h, "Voices");
    h.run_steps(2);
    // English first: the French voice waits behind the language picker.
    let _ = h.get_by_label("System default");
    assert!(h.query_by_label("Hush").is_none());
    press(&mut h, "Quiet");
    h.run_steps(2);
    let picked = h.state().settings().read_aloud.voice().map(str::to_owned);
    assert_eq!(picked.as_deref(), Some("quiet-en"));
    press(&mut h, "System default");
    h.run_steps(2);
    assert_eq!(h.state().settings().read_aloud.voice(), None);
    // The list folds away, its pick still named.
    press(&mut h, "Voices");
    h.run_steps(2);
    assert!(h.query_by_label("Quiet").is_none());
    press(&mut h, "Voices");
    h.run_steps(2);
    let _ = h.get_by_label("Quiet");
}

#[test]
fn each_voice_says_a_line_in_itself() {
    let mut h = voice_settings(1);
    press(&mut h, "Voices");
    h.run_steps(2);
    press(&mut h, "Preview Quiet");
    h.run_steps(1);
    let booth = &h.state().read_aloud().booth;
    assert_eq!(booth.samples(), 1);
    assert_eq!(booth.trying_voice(), Some("quiet-en"));
    // Its picked voice stays as it was.
    assert_eq!(h.state().settings().read_aloud.voice(), None);
    // Once the line ends, its button offers it again.
    step_until(&mut h, |h| !h.state().read_aloud().booth.trying());
    h.run_steps(1);
    let _ = h.get_by_label("Preview Quiet");
}

#[test]
fn a_preview_says_a_line_and_the_book_waits() {
    let mut h = voice_settings(1);
    start(&mut h, ReadFrom::default());
    assert!(h.state().read_aloud().playing());
    press(&mut h, "Preview");
    h.run_steps(1);
    assert_eq!(h.state().read_aloud().booth.samples(), 1);
    assert!(!h.state().read_aloud().playing(), "the book pauses");
    // The silent voice ends its line within a few looks.
    step_until(&mut h, |h| !h.state().read_aloud().booth.trying());
    let _ = h.get_by_label("Preview");
    h.state_mut().queue(Command::CloseSettings);
    h.run_steps(2);
    assert!(
        !h.state().read_aloud().booth.is_open(),
        "closing lets it go"
    );
}

#[test]
fn the_shortcut_starts_reading_then_pauses_it() {
    let mut h = reader(3);
    let keys = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
    h.key_press_modifiers(keys, egui::Key::U);
    step_until(&mut h, |h| lit(h).is_some());
    assert!(h.state().read_aloud().playing());
    h.key_press_modifiers(keys, egui::Key::U);
    h.run_steps(1);
    assert!(!h.state().read_aloud().playing(), "the same keys pause");
    // A pause between silent sentences lights the next one, unsaid.
    step_until(&mut h, |h| lit(h).is_some());
    assert!(lit(&h).is_some(), "paused, it stays lit");
    assert!(!h.state().read_aloud().playing(), "and stays paused");
}

#[test]
fn a_book_set_in_new_type_reads_on_from_the_same_sentence() {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("epub", "sample:3").ok();
    app.set_option("set", "read_aloud.engine:\"silent\"").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    let pane = h.state().active_pane();
    let from = ReadFrom {
        anchor: None,
        at: Some(Spot { page: 1, char: 0 }),
        until: None,
    };
    start(&mut h, from);
    let first = lit(&h);
    step_until(&mut h, |h| lit(h) != first);
    h.get_by_label("Pause").click();
    h.run_steps(2);
    let said = |h: &Harness<'static, MareaderApp>| h.state().read_aloud().said().map(|(_, a)| a);
    let before = said(&h).expect("the sentence has a place");
    let doc =
        |h: &Harness<'static, MareaderApp>| h.state().documents().reading(pane).map(|r| r.doc);
    let was = doc(&h);
    // Bigger type sets the book anew, as another document.
    h.state_mut().set_option("text_size", "28").ok();
    step_until(&mut h, |h| doc(h) != was && lit(h).is_some());
    assert_ne!(doc(&h), was, "set anew");
    assert!(h.state().read_aloud().active(), "reading goes on");
    assert_eq!(said(&h), Some(before), "the same sentence");
    let _ = h.get_by_label("Play");
    // Lit, and shown, where the new pages have it.
    let reading = h.state().documents().reading(pane).expect("a book");
    let page = reading.document.locate(before);
    assert!(page.is_some(), "the new pages have it");
    assert_eq!(lit(&h).map(|(s, _)| s.page), page);
    let shown = h.state().documents().visible(pane).unwrap_or(0..0);
    assert!(
        page.is_some_and(|p| shown.contains(&p)),
        "{page:?} in {shown:?}"
    );
    // Playing goes on past it.
    h.get_by_label("Play").click();
    let now = lit(&h);
    step_until(&mut h, |h| lit(h) > now);
    assert!(lit(&h) > now, "playing goes on");
}

/// The first character of `word` on page 0, and where it sits on screen.
fn word(h: &Harness<'static, MareaderApp>, word: &str) -> (Spot, egui::Rect) {
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let layer = docs.reading(pane).map(|r| r.document.text_layer(0));
    let layer = layer.and_then(Result::ok).expect("page 0's text");
    let sheet = *docs
        .sheets(pane)
        .iter()
        .find(|s| s.page == 0)
        .expect("page 0");
    let letters: Vec<char> = layer.text.chars().iter().map(|c| c.ch).collect();
    let want: Vec<char> = word.chars().collect();
    let char = (letters.windows(want.len()))
        .position(|w| w == want.as_slice())
        .expect("the word is on the page");
    let bounds = layer.text.chars().get(char).expect("its letter").bounds;
    (Spot { page: 0, char }, sheet.to_screen(bounds))
}

fn pointer(h: &mut Harness<'static, MareaderApp>, pos: egui::Pos2, pressed: Option<bool>) {
    h.event(egui::Event::PointerMoved(pos));
    if let Some(pressed) = pressed {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(1);
}

fn marker(h: &Harness<'static, MareaderApp>) -> Option<Spot> {
    let pane = h.state().active_pane();
    h.state().documents().read_marker(pane)
}

#[test]
fn the_page_menu_pins_the_start_and_reads_once_it_rests() {
    let mut h = reader(1);
    let (quick, at) = word(&h, "quick");
    // Right-click inside the word; the pin goes to its start.
    let inside = at.center() + egui::vec2(at.width() * 2.0, 0.0);
    h.event(egui::Event::PointerMoved(inside));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: inside,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(3);
    h.get_by_label("Read aloud from “quick”").click();
    h.run_steps(3);
    assert_eq!(marker(&h), Some(quick), "pinned at the word's start");
    let _ = h.get_by_label("Reading starts here");
    h.run_steps(40);
    assert!(lit(&h).is_none(), "nothing reads before it rests");
    step_until(&mut h, |h| lit(h).is_some());
    assert_eq!(lit(&h).map(|(s, _)| s), Some(quick));
    assert_eq!(marker(&h), None, "the pin gives way to the reading");
    h.run_steps(30);
    assert!(h.query_by_label("Reading starts here").is_none());
}

#[test]
fn the_pin_is_dragged_to_a_word_and_reads_from_there_after_resting() {
    let mut h = reader(1);
    let pane = h.state().active_pane();
    let (quick, from) = word(&h, "quick");
    let (lazy, to) = word(&h, "lazy");
    h.state_mut()
        .queue(Command::PlaceReadMarker { pane, spot: quick });
    h.run_steps(20);
    let pin = h.get_by_label("Reading starts here").rect();
    let grab = egui::pos2(pin.center().x, pin.min.y + 14.0);
    pointer(&mut h, grab, Some(true));
    // Carried by the knob: the word it ends beside is the one taken.
    let left = |r: egui::Rect| egui::pos2(r.left(), r.center().y);
    let drop = left(to) + (grab - left(from));
    for k in 1..=12 {
        let p = grab + (drop - grab) * (k as f32 / 12.0);
        pointer(&mut h, p, None);
    }
    h.run_steps(5);
    assert_eq!(marker(&h), Some(lazy), "it snaps to the word's start");
    h.run_steps(100);
    assert!(lit(&h).is_none(), "nothing reads while it is held");
    pointer(&mut h, drop, Some(false));
    h.run_steps(30);
    assert!(lit(&h).is_none(), "it rests a moment first");
    step_until(&mut h, |h| lit(h).is_some());
    assert_eq!(lit(&h).map(|(s, _)| s), Some(lazy));
}

#[test]
fn escape_sends_the_pin_away_unread() {
    let mut h = reader(1);
    let pane = h.state().active_pane();
    let (quick, _) = word(&h, "quick");
    h.state_mut()
        .queue(Command::PlaceReadMarker { pane, spot: quick });
    h.run_steps(5);
    assert_eq!(marker(&h), Some(quick));
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!(marker(&h), None);
    h.run_steps(120);
    assert!(lit(&h).is_none(), "nothing reads");
    assert!(h.query_by_label("Reading starts here").is_none());
}

#[test]
fn the_right_clicked_word_is_lit_where_reading_would_start() {
    let mut h = reader(1);
    let pane = h.state().active_pane();
    let (quick, _) = word(&h, "quick");
    // The right edge of its last letter is still the word, not the next.
    let k = {
        let docs = h.state().documents();
        let layer = docs.reading(pane).map(|r| r.document.text_layer(0));
        let layer = layer.and_then(Result::ok).expect("page 0's text");
        let sheet = *docs
            .sheets(pane)
            .iter()
            .find(|s| s.page == 0)
            .expect("page 0");
        let bounds = layer
            .text
            .chars()
            .get(quick.char + 4)
            .expect("its k")
            .bounds;
        sheet.to_screen(bounds)
    };
    let at = egui::pos2(k.right() - 1.0, k.center().y);
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(3);
    let lit_word = h
        .state()
        .page_ai()
        .washed_word(pane)
        .expect("a word is lit");
    assert_eq!(lit_word.page, 0);
    assert!(
        (quick.char..quick.char + 5).contains(&lit_word.char),
        "{lit_word:?} is in “quick” at {quick:?}"
    );
    h.get_by_label("Read aloud from “quick”").click();
    h.run_steps(3);
    assert_eq!(marker(&h), Some(quick), "the pin takes the lit word");
}

fn right_click(h: &mut Harness<'static, MareaderApp>, at: egui::Pos2) {
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(12);
}

#[test]
fn the_menu_keeps_off_its_word_and_rides_along_with_it() {
    let mut h = reader(2);
    let pane = h.state().active_pane();
    let (_, at) = word(&h, "quick");
    right_click(&mut h, at.center());
    let menu = h.state().page_ai().menu_rect().expect("the menu shows");
    let (_, lit) = word(&h, "quick");
    let whole = lit.expand2(egui::vec2(lit.width() * 4.0, 0.0));
    assert!(!menu.intersects(lit), "{menu:?} covers the word at {lit:?}");
    assert!(menu.top() > whole.bottom() || menu.bottom() < whole.top());
    // Scrolled a little, the menu moves just as its word does.
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -30.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(20);
    let (_, moved) = word(&h, "quick");
    let dy = moved.top() - lit.top();
    assert!(dy < -5.0, "the page scrolled: {dy}");
    let now = h.state().page_ai().menu_rect().expect("the menu stays");
    assert!(
        (now.top() - menu.top() - dy).abs() < 1.5,
        "{now:?} after {menu:?}, {dy}"
    );
    assert_eq!(h.state().page_ai().washed(pane), Some(&[0][..]));
}

#[test]
fn off_the_text_it_reads_from_the_page_s_start() {
    let mut h = reader(1);
    let pane = h.state().active_pane();
    let sheet = *h.state().documents().sheets(pane).first().expect("a page");
    right_click(&mut h, sheet.shown.min + egui::vec2(14.0, 14.0));
    h.get_by_label("Read aloud from page start").click();
    h.run_steps(3);
    let first = marker(&h).expect("a pin");
    assert_eq!(first, Spot { page: 0, char: 0 }, "the page's first word");
}

#[test]
fn a_pin_off_screen_is_brought_into_view_before_it_reads() {
    let mut h = reader(3);
    let pane = h.state().active_pane();
    let spot = Spot { page: 2, char: 0 };
    h.state_mut().queue(Command::PlaceReadMarker { pane, spot });
    h.run_steps(40);
    assert!(lit(&h).is_none(), "it waits its rest first");
    let shown = |h: &Harness<'static, MareaderApp>| {
        let sheets = h.state().documents().sheets(pane);
        sheets
            .iter()
            .any(|s| s.page == 2 && s.shown.height() > 40.0)
    };
    assert!(shown(&h), "its page came into view");
    assert!(h.query_by_label("Reading starts here").is_some());
    step_until(&mut h, |h| lit(h).is_some());
    assert_eq!(lit(&h).map(|(s, _)| s), Some(spot));
}

fn carded(h: &Harness<'static, MareaderApp>) -> bool {
    h.state().read_aloud().carded()
}

#[test]
fn the_player_opens_into_a_card_when_the_page_stops_following() {
    let mut h = reader(3);
    start(&mut h, ReadFrom::default());
    h.run_steps(2);
    assert!(!carded(&h), "the words are in sight");
    let _ = h.get_by_label("Expand player");
    // Not followed, the words will drift away: the card shows them.
    h.state_mut().settings_mut().read_aloud.follow = false;
    h.run_steps(2);
    assert!(carded(&h));
    h.run_steps(30);
    let _ = h.get_by_label("Show where it's reading");
    let _ = h.get_by_label("Pause");
    h.get_by_label("Collapse player").click();
    h.run_steps(2);
    assert!(!carded(&h), "asked by hand, the pill stays");
    // Followed again, it stays a pill; asked, the card comes back.
    h.state_mut().settings_mut().read_aloud.follow = true;
    h.run_steps(2);
    assert!(!carded(&h));
    h.run_steps(30);
    h.get_by_label("Expand player").click();
    h.run_steps(2);
    assert!(carded(&h));
}

#[test]
fn a_scroll_hidden_highlight_makes_the_dictionary_yield_to_the_player() {
    use mareader_settings::Player;

    let mut h = reader(3);
    h.state_mut().settings_mut().read_aloud.player = Player::Full;
    start(&mut h, ReadFrom::default());
    h.get_by_label("Pause").click();
    h.run_steps(300);
    assert!(!carded(&h), "the highlight starts in view");

    let id = egui::Id::new("read-aloud-bar");
    let compact = h.ctx.memory(|m| m.area_rect(id)).expect("compact player");
    let room_top = h.ctx.content_rect().top() + mareader_ui::theme::TITLE_BAR_H + 12.0;
    let height = 420.0_f32.min(compact.top() - room_top - 16.0);
    assert!(height > 240.0, "not enough room for the test: {height}");
    let top = compact.top() - height - 4.0;
    let window = &mut h.state_mut().settings_mut().dictionary.window;
    window.open = true;
    window.minimized = false;
    window.rect = Some([100.0, top, 300.0, height]);
    h.run_steps(30);
    let home = h.state().dict().window_rect().expect("dictionary home");
    assert!(
        !home.intersects(compact),
        "the bar fits beside it: {home:?}"
    );

    let pane = h.state().active_pane();
    let pane_rect = h
        .state()
        .pane_rects()
        .iter()
        .find(|(p, _)| *p == pane)
        .map(|(_, r)| *r)
        .expect("reader pane");
    h.hover_at(egui::pos2(pane_rect.right() - 20.0, pane_rect.center().y));
    h.run_steps(1);
    for _ in 0..4 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -500.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(3);
    }
    h.run_steps(60);

    assert!(carded(&h), "the hidden highlight opens the full player");
    let full = h.ctx.memory(|m| m.area_rect(id)).expect("full player");
    assert!(
        home.intersects(full),
        "the full player needs room: {home:?} vs {full:?}"
    );
    let aside = h
        .state()
        .dict()
        .window_rect()
        .expect("dictionary steps clear");
    assert!(!aside.intersects(full), "{aside:?} vs {full:?}");
    assert!(
        aside.height() < home.height() || aside.top() < home.top() - 20.0,
        "the dictionary moves or shortens: {aside:?} vs {home:?}"
    );
    assert!(h.query_by_label("Collapse player").is_some());
    assert!(h.query_by_label("Show the read-aloud player").is_none());
}

#[test]
fn the_card_shows_over_the_library_and_leads_back_to_the_words() {
    use mareader_ui::route::Route;
    let mut h = reader(3);
    start(&mut h, ReadFrom::default());
    let first = lit(&h);
    h.state_mut().queue(Command::ShowLibrary);
    h.run_steps(2);
    assert_eq!(h.state().route(), Route::Library);
    assert!(carded(&h), "the library hides the words");
    step_until(&mut h, |h| lit(h) != first);
    assert!(h.state().read_aloud().playing(), "reading goes on");
    h.run_steps(30);
    h.get_by_label("Show where it's reading").click();
    h.run_steps(4);
    assert_eq!(h.state().route(), Route::Reader);
    assert!(!carded(&h), "back with the words, it folds away");
    let (pane, page, _) = h.state().read_aloud().spot().expect("still reading");
    assert_eq!(h.state().documents().current(pane), Some(page));
}

/// Another book, opened over the one in the active pane.
fn replace_book(h: &mut Harness<'static, MareaderApp>) {
    use mareader_ui::documents::OpenRequest;
    let pdf = mareader_pdf::sample::pdf(2, "Another Book");
    let request = OpenRequest {
        pane: Some(h.state().active_pane()),
        ..OpenRequest::bytes(pdf, "another.pdf")
    };
    h.state_mut().queue(Command::Open(request));
    h.run_steps(3);
}

/// A sample read from a file, so it can be opened again.
fn reader_from_file(dir: &std::path::Path) -> (Harness<'static, MareaderApp>, std::path::PathBuf) {
    use mareader_ui::documents::OpenRequest;
    let path = dir.join("Kept.pdf");
    std::fs::write(&path, mareader_pdf::sample::pdf(40, "Kept")).expect("a sample file");
    let mut h = reader(1);
    let request = OpenRequest {
        pane: Some(h.state().active_pane()),
        ..OpenRequest::path(path.clone())
    };
    h.state_mut().queue(Command::Open(request));
    let pane = h.state().active_pane();
    step_until(&mut h, |h| {
        h.state().documents().page_count(pane) == Some(40)
    });
    (h, path)
}

#[test]
fn a_book_replaced_in_its_pane_reads_on_unseen() {
    let mut h = reader(3);
    start(&mut h, ReadFrom::default());
    let pane = h.state().active_pane();
    let before = h.state().read_aloud().reading_spot();
    replace_book(&mut h);
    let aloud = h.state().read_aloud();
    assert!(aloud.away().is_some(), "its pane shows another book");
    assert!(aloud.lit(pane).is_none(), "nothing is lit in the new book");
    assert_eq!(
        aloud.origin().map(|o| o.name.as_str()),
        Some("A Mareader Sample")
    );
    step_until(&mut h, |h| h.state().read_aloud().reading_spot() != before);
    assert!(h.state().read_aloud().playing(), "it reads on");
    h.run_steps(2);
    assert!(carded(&h), "the card shows the words instead");
}

#[test]
fn the_card_opens_the_book_again_beside_and_reads_on_there() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let (mut h, _) = reader_from_file(dir.path());
    let first = h.state().active_pane();
    start(&mut h, ReadFrom::default());
    replace_book(&mut h);
    assert!(h.state().read_aloud().away().is_some());
    h.run_steps(30);
    h.get_by_label("Show where it's reading").click();
    step_until(&mut h, |h| h.state().read_aloud().home().is_some());
    let home = h.state().read_aloud().home().expect("found again");
    assert_ne!(home, first, "beside the book now read");
    assert_eq!(h.state().panes().len(), 2);
    assert!(h.state().read_aloud().lit(home).is_some(), "lit there");
    // It turns to the words read, and keeps up with them.
    let page = |h: &Harness<'static, MareaderApp>| {
        let spot = h.state().read_aloud().reading_spot();
        spot.map(|s| s.0)
    };
    step_until(&mut h, |h| h.state().documents().current(home) == page(h));
    assert_eq!(h.state().documents().current(home), page(&h));
}

#[test]
fn a_book_gone_from_disk_says_so_and_reading_goes_on() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let (mut h, path) = reader_from_file(dir.path());
    start(&mut h, ReadFrom::default());
    replace_book(&mut h);
    std::fs::remove_file(&path).expect("the file goes");
    h.run_steps(30);
    h.get_by_label("Show where it's reading").click();
    h.run_steps(3);
    let toast = h.state().toast().map(|t| t.message.clone());
    assert!(
        toast
            .as_ref()
            .is_some_and(|t| t.contains("moved or deleted")),
        "{toast:?}"
    );
    assert!(
        h.state().read_aloud().away().is_some(),
        "still reading unseen"
    );
    assert_eq!(h.state().panes().len(), 1);
}

#[test]
fn bar_only_stays_a_bar_until_asked_and_full_keeps_it_open() {
    use mareader_settings::Player;
    let mut h = reader(3);
    h.state_mut().settings_mut().read_aloud.player = Player::Bar;
    start(&mut h, ReadFrom::default());
    h.state_mut().settings_mut().read_aloud.follow = false;
    h.run_steps(2);
    assert!(!carded(&h), "bar only waits to be asked");
    h.get_by_label("Expand player").click();
    h.run_steps(30);
    assert!(carded(&h));
    let _ = h.get_by_label("Show where it's reading");
    // Switched to full, it stays open while it changes shape.
    h.state_mut().settings_mut().read_aloud.player = Player::Full;
    h.run_steps(30);
    assert!(carded(&h));
    let _ = h.get_by_label("Show where it's reading");
}

/// A sample EPUB read from a file, so it can be opened again.
fn epub_from_file(dir: &std::path::Path) -> (Harness<'static, MareaderApp>, std::path::PathBuf) {
    use mareader_ui::documents::OpenRequest;
    let path = dir.join("Kept.epub");
    let bytes = mareader_epub::sample::epub(8, "Kept");
    std::fs::write(&path, bytes).expect("a sample file");
    let mut h = reader(1);
    let request = OpenRequest {
        pane: Some(h.state().active_pane()),
        ..OpenRequest::path(path.clone())
    };
    h.state_mut().queue(Command::Open(request));
    let pane = h.state().active_pane();
    step_until(&mut h, |h| {
        let docs = h.state().documents();
        docs.path(pane).is_some_and(|p| p.ends_with("Kept.epub"))
            && docs.page_count(pane).is_some_and(|n| n > 4)
    });
    (h, path)
}

#[test]
fn a_reflowed_book_opened_again_is_lit_and_followed_there() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let (mut h, _) = epub_from_file(dir.path());
    let first = h.state().active_pane();
    let pages = h.state().documents().page_count(first).unwrap_or(0);
    let from = ReadFrom {
        anchor: None,
        at: Some(Spot {
            page: pages / 2,
            char: 0,
        }),
        until: None,
    };
    start(&mut h, from);
    replace_book(&mut h);
    assert!(h.state().read_aloud().away().is_some());
    h.run_steps(30);
    h.get_by_label("Show where it's reading").click();
    step_until(&mut h, |h| {
        let aloud = h.state().read_aloud();
        aloud.home().is_some_and(|p| aloud.lit(p).is_some())
    });
    let home = h.state().read_aloud().home().expect("found again");
    assert_ne!(home, first, "beside the book now read");
    // It stays home, not away again the next frame.
    h.run_steps(20);
    let aloud = h.state().read_aloud();
    assert!(aloud.away().is_none(), "home, not away");
    assert_eq!(aloud.home(), Some(home));
    assert!(aloud.lit(home).is_some(), "lit there");
    let page = |h: &Harness<'static, MareaderApp>| {
        let spot = h.state().read_aloud().reading_spot();
        spot.map(|s| s.0)
    };
    step_until(&mut h, |h| h.state().documents().current(home) == page(h));
    assert_eq!(h.state().documents().current(home), page(&h));
    assert!(page(&h).is_some_and(|p| p > 0), "turned to the words read");
}

#[test]
fn dragging_a_side_sets_the_player_width_for_good() {
    let mut h = reader(3);
    start(&mut h, ReadFrom::default());
    h.run_steps(30);
    let side = |h: &Harness<'static, MareaderApp>, name: &str| {
        let label = format!("Resize player at its {name}");
        h.get_by_label(&label).rect().center()
    };
    let before = side(&h, "end").x - side(&h, "start").x;
    let from = side(&h, "end");
    let to = from + egui::vec2(40.0, 0.0);
    h.hover_at(from);
    h.run();
    h.drag_at(from);
    h.run();
    for step in 1..=8 {
        h.hover_at(from.lerp(to, step as f32 / 8.0));
        h.run();
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    let set = h.state().settings().read_aloud.player_width;
    let wider = set.expect("a width set by hand");
    // Centred, it grows on both sides; the drag's first points are slack.
    assert!(
        wider > before + 50.0 && wider <= before + 82.0,
        "{before} → {wider}"
    );
    h.run_steps(20);
    let now = side(&h, "end").x - side(&h, "start").x;
    assert!((now - wider).abs() < 1.0, "it keeps that width: {now}");
}

#[test]
fn the_player_wears_its_own_panes_look_and_the_windows_when_away() {
    use mareader_ui::theme::Tokens;
    let mut app = MareaderApp::default();
    for (key, value) in [
        ("workers", "0"),
        ("pdf", "sample:3"),
        ("panes", "2"),
        ("independent", "true"),
        ("set", "read_aloud.engine:\"silent\""),
    ] {
        app.set_option(key, value).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1100.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    let window = Tokens::new(&h.state().settings().appearance);
    let panes = h.state().panes();
    let own = |h: &Harness<'static, MareaderApp>, p| Tokens::new(&h.state().look(p));
    let pane = (panes.iter().copied())
        .find(|&p| own(&h, p) != window)
        .expect("a pane in a look of its own");
    h.state_mut().queue(Command::FocusPane(pane));
    h.run_steps(2);
    // The book opened there, so it reads in that look.
    replace_book(&mut h);
    step_until(&mut h, |h| h.state().documents().page_count(pane).is_some());
    start(&mut h, ReadFrom::default());
    h.run_steps(30);
    let aloud = h.state().read_aloud();
    assert_eq!(aloud.home(), Some(pane));
    assert_ne!(own(&h, pane), window, "focus leaves the window's look be");
    assert_eq!(aloud.colours(), Some(own(&h, pane)), "its book's look");
    // Away from its book, it is the window's again.
    replace_book(&mut h);
    h.run_steps(30);
    let away = h.state().read_aloud().colours();
    assert!(
        away.is_some() && away != Some(own(&h, pane)),
        "the window's"
    );
}

#[test]
fn a_voice_not_downloaded_links_to_its_settings() {
    let shelf = tempfile::tempdir().expect("an empty models folder");
    let mut app = MareaderApp::default();
    let shelf_path = shelf.path().to_string_lossy().into_owned();
    let options = [
        ("workers", "0"),
        ("pdf", "sample:2"),
        ("voice_models", shelf_path.as_str()),
        ("set", "read_aloud.engine:\"kokoro\""),
    ];
    for (k, v) in options {
        app.set_option(k, v).expect("a known option");
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    let pane = h.state().active_pane();
    let from = ReadFrom::default();
    h.state_mut().queue(Command::ReadAloud { pane, from });
    let link = "Kokoro isn't downloaded · Voice settings";
    step_until(&mut h, |h| h.query_by_label(link).is_some());
    assert!(!h.state().read_aloud().booth.is_open());
    h.get_by_label(link).click();
    h.run_steps(4);
    assert!(
        h.state().read_aloud().booth.is_open(),
        "on the Read aloud tab"
    );
}

#[test]
fn the_line_read_stays_clear_of_the_player() {
    // Short enough that a page's lines run on under the player.
    let mut h = reader_sized(3, egui::vec2(900.0, 420.0));
    start(&mut h, ReadFrom::default());
    let pane = h.state().active_pane();
    let mut lines = Vec::new();
    for _ in 0..12 {
        h.get_by_label("Pause").click();
        h.run_steps(60);
        let (from, to) = lit(&h).expect("a line is lit");
        let end = if to.page == from.page {
            to.char
        } else {
            from.char + 1
        };
        let docs = h.state().documents();
        let line = docs.hit_box(pane, from.page, from.char..end);
        let line = line.expect("the line read is on screen");
        let player = h.get_by_label("Play").rect();
        assert!(
            line.bottom() <= player.top(),
            "{line:?} under the player at {player:?}"
        );
        lines.push(from);
        h.get_by_label("Play").click();
        step_until(&mut h, |h| lit(h).is_some_and(|l| l.0 != from));
    }
    lines.dedup();
    assert!(lines.len() >= 6, "{lines:?}");
}

/// The page and first character of the first table in the sample book.
fn first_table(h: &Harness<'static, MareaderApp>) -> Option<Spot> {
    let pane = h.state().active_pane();
    let document = &h.state().documents().reading(pane)?.document;
    (0..document.page_count()).find_map(|page| {
        let layer = document.text_layer(page).ok()?;
        let (run, _) = layer.tables.first()?;
        Some(Spot {
            page,
            char: run.start,
        })
    })
}

/// Whether the sentence lit now starts in a table cell.
fn lit_in_table(h: &Harness<'static, MareaderApp>) -> bool {
    let pane = h.state().active_pane();
    let Some(((start, _), reading)) = lit(h).zip(h.state().documents().reading(pane)) else {
        return false;
    };
    let layer = reading.document.text_layer(start.page).ok();
    layer.is_some_and(|l| l.table(start.char).is_some())
}

#[test]
fn a_table_read_aloud_can_be_skipped_and_stepped_back_into() {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("epub", "sample:3").ok();
    app.set_option("set", "read_aloud.engine:\"silent\"").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    let skip = "Skip the rest of this table";
    assert!(h.query_by_label(skip).is_none(), "no table, no skip");
    let at = first_table(&h).expect("the sample has a table");
    let from = ReadFrom {
        anchor: None,
        at: Some(at),
        until: None,
    };
    start(&mut h, from);
    h.get_by_label("Pause").click();
    h.run_steps(2);
    assert!(lit_in_table(&h), "a cell is read first");
    // Skipped, every cell left is passed; the words after it are lit.
    let was = lit(&h);
    h.get_by_label(skip).click();
    step_until(&mut h, |h| lit(h) != was && !lit_in_table(h));
    // The player draws before reading moves on: a frame more to show it.
    h.run_steps(2);
    assert!(!lit_in_table(&h), "past the table: {:?}", lit(&h));
    assert!(lit(&h).is_some_and(|(s, _)| s > at), "it went on");
    assert!(h.query_by_label(skip).is_none(), "no skip outside it");
    let _ = h.get_by_label("Play");
    // A step back lands on the table's last cell.
    h.get_by_label("Previous sentence").click();
    h.run_steps(2);
    assert!(lit_in_table(&h), "back in the table");
    assert!(lit(&h) > was, "on a later cell");
    assert!(h.query_by_label(skip).is_some(), "it can be skipped again");
}

#[test]
fn a_table_can_be_asked_about_or_passed_over() {
    use mareader_settings::Skip;
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("epub", "sample:3").ok();
    app.set_option("set", "read_aloud.engine:\"silent\"").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    let ask = Skip {
        on: true,
        ask: true,
    };
    h.state_mut().settings_mut().read_aloud.skips.tables = ask;
    let at = first_table(&h).expect("the sample has a table");
    let before = Spot {
        page: at.page.saturating_sub(1),
        char: 0,
    };
    let from = ReadFrom {
        anchor: None,
        at: Some(before),
        until: None,
    };
    let past =
        |h: &Harness<'static, MareaderApp>| !lit_in_table(h) && lit(h).is_some_and(|(s, _)| s > at);
    // Asked, reading holds at the table's start for the answer.
    start(&mut h, from);
    step_until(&mut h, |h| h.query_by_label("Read the table").is_some());
    h.run_steps(20);
    assert!(lit_in_table(&h), "held at the table: {:?}", lit(&h));
    let _ = h.get_by_label("Play");
    h.key_press(egui::Key::Escape);
    step_until(&mut h, past);
    h.run_steps(30);
    assert!(h.query_by_label("Read the table").is_none(), "answered");
    // Read, its cells are read on unasked.
    start(&mut h, from);
    step_until(&mut h, |h| h.query_by_label("Read the table").is_some());
    h.run_steps(20);
    h.key_press(egui::Key::Space);
    h.run_steps(2);
    let _ = h.get_by_label("Pause");
    assert!(lit_in_table(&h), "reading the table");
    // Set to skip, it is passed over without a word or a question.
    h.state_mut().settings_mut().read_aloud.skips.tables.ask = false;
    start(&mut h, from);
    let seen = std::cell::Cell::new(false);
    step_until(&mut h, |h| {
        seen.set(seen.get() || lit_in_table(h));
        past(h)
    });
    assert!(!seen.get(), "no cell was read");
    assert!(h.query_by_label("Read the table").is_none());
}

/// The lists the sentence lit now starts in, by which one.
fn lit_lists(h: &Harness<'static, MareaderApp>) -> Vec<usize> {
    let pane = h.state().active_pane();
    let Some(((start, _), reading)) = lit(h).zip(h.state().documents().reading(pane)) else {
        return Vec::new();
    };
    let Ok(layer) = reading.document.text_layer(start.page) else {
        return Vec::new();
    };
    let lists = layer.passages(start.char).iter();
    let lists = lists.filter(|(kind, _)| *kind == mareader_session::Passage::List);
    lists.map(|&(_, id)| id).collect()
}

/// The page and first character of the first list in the sample book.
fn first_list(h: &Harness<'static, MareaderApp>) -> Option<Spot> {
    let pane = h.state().active_pane();
    let document = &h.state().documents().reading(pane)?.document;
    (0..document.page_count()).find_map(|page| {
        let layer = document.text_layer(page).ok()?;
        let (run, _) = layer.passages.first()?;
        Some(Spot {
            page,
            char: run.start,
        })
    })
}

#[test]
fn a_list_can_be_asked_about_or_passed_over() {
    use mareader_settings::Skip;
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("epub", "sample:3").ok();
    app.set_option("set", "read_aloud.engine:\"silent\"").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    let ask = Skip {
        on: true,
        ask: true,
    };
    h.state_mut().settings_mut().read_aloud.skips.lists = ask;
    let at = first_list(&h).expect("the sample has a list");
    let before = Spot {
        page: at.page.saturating_sub(1),
        char: 0,
    };
    let from = ReadFrom {
        anchor: None,
        at: Some(before),
        until: None,
    };
    // Asked, reading holds at the list's start for the answer.
    start(&mut h, from);
    step_until(&mut h, |h| h.query_by_label("Read the list").is_some());
    h.run_steps(20);
    let first = lit_lists(&h);
    assert_eq!(first.len(), 1, "held at the list: {:?}", lit(&h));
    let _ = h.get_by_label("Play");
    // Skipped, its nested list goes too; the next list is asked about.
    h.get_by_label("Skip the list").click();
    let left = |h: &Harness<'static, MareaderApp>| lit_lists(h) != first;
    step_until(&mut h, left);
    step_until(&mut h, |h| h.query_by_label("Read the list").is_some());
    h.run_steps(20);
    let second = lit_lists(&h);
    assert!(!second.is_empty() && second != first, "the next list");
    // Read, it is read on unasked, with a way out of it.
    h.get_by_label("Read the list").click();
    h.run_steps(2);
    let _ = h.get_by_label("Pause");
    assert!(h.query_by_label("Skip the rest of this list").is_some());
    // Set to skip, no list is read and nothing is asked.
    h.state_mut().settings_mut().read_aloud.skips.lists.ask = false;
    start(&mut h, from);
    let seen = std::cell::Cell::new(false);
    let past = |h: &Harness<'static, MareaderApp>| lit(h).is_some_and(|(s, _)| s.page > at.page);
    step_until(&mut h, |h| {
        seen.set(seen.get() || !lit_lists(h).is_empty());
        past(h)
    });
    assert!(!seen.get(), "no list was read");
    assert!(h.query_by_label("Read the list").is_none());
}

#[test]
fn the_options_menu_sets_the_player_and_stays_over_it() {
    use mareader_settings::{Highlight, Player};
    let mut h = reader(3);
    start(&mut h, ReadFrom::default());
    h.get_by_label("Pause").click();
    h.run_steps(2);
    h.get_by_label("Read-aloud options").click();
    h.run_steps(4);
    h.get_by_label("Full").click();
    h.run_steps(2);
    h.get_by_label("Words").click();
    h.run_steps(2);
    let follow = h.state().settings().read_aloud.follow;
    h.get_by_label("Turn pages to follow").click();
    h.run_steps(2);
    let s = &h.state().settings().read_aloud;
    assert_eq!((s.player, s.highlight), (Player::Full, Highlight::Words));
    assert_ne!(s.follow, follow, "the switch flips it");
    // Choosing keeps the menu open; pressing the player keeps it on top.
    let row = h.get_by_label("Voice settings…").rect();
    let menu = h.ctx.layer_id_at(row.center()).expect("the menu's layer");
    let play = h.get_by_label("Play").rect();
    pointer(&mut h, play.center(), Some(true));
    h.run_steps(1);
    let bar = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("read-aloud-bar"));
    let order: Vec<_> = h.ctx.memory(|m| m.layer_ids().collect());
    let at = |l: egui::LayerId| order.iter().position(|o| *o == l);
    assert!(at(menu) > at(bar), "{order:?}");
}

#[test]
fn player_confirmations_ride_the_player_without_shrinking_it() {
    let mut h = reader(2);
    start(&mut h, ReadFrom::default());
    h.get_by_label("Pause").click();
    h.run_steps(30);
    let id = egui::Id::new("read-aloud-bar");
    let before = h.ctx.memory(|m| m.area_rect(id)).expect("player");
    h.state_mut()
        .set_option("toast", "player:Speed_saved")
        .unwrap();
    h.run_steps(10);
    assert!(h.query_by_label("Speed saved").is_some());
    assert!(
        h.ctx
            .memory(|m| m.area_rect(egui::Id::new("toast-island")))
            .is_some()
    );
    let after = h.ctx.memory(|m| m.area_rect(id)).expect("player");
    assert!((before.width() - after.width()).abs() < 1.0);
}

#[test]
fn a_long_player_confirmation_uses_the_regular_toast() {
    let mut h = reader(2);
    start(&mut h, ReadFrom::default());
    h.get_by_label("Pause").click();
    h.run_steps(30);
    let message = "This confirmation is much too long to fit above a compact player";
    h.state_mut()
        .set_option("toast", &format!("player:{message}"))
        .unwrap();
    h.run_steps(10);
    assert!(h.query_by_label(message).is_some());
    assert!(
        h.ctx
            .memory(|m| m.area_rect(egui::Id::new("toasts")))
            .is_some()
    );
}

#[test]
fn changing_speed_sends_a_player_confirmation() {
    let mut h = reader(2);
    h.state_mut().settings_mut().read_aloud.player_width = Some(650.0);
    start(&mut h, ReadFrom::default());
    h.get_by_label("Pause").click();
    h.run_steps(20);
    h.get_by_label("1×").click();
    h.run_steps(3);
    h.get_by_label("1.25×").click();
    h.run_steps(10);
    assert!(h.query_by_label("Speed 1.25×").is_some());
    assert!(
        h.ctx
            .memory(|m| m.area_rect(egui::Id::new("toast-island")))
            .is_some()
    );
}

#[test]
fn player_at_wide_width_minimizes_for_toast_and_restores() {
    let mut h = reader(3);
    h.state_mut().settings_mut().read_aloud.player_width = Some(720.0);
    start(&mut h, ReadFrom::default());
    h.run_steps(10);
    let bar = h
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("read-aloud-bar")))
        .unwrap();
    assert!(bar.width() > 500.0, "wide player: {bar:?}");
    // Toast arrives while wide player is idling.
    h.state_mut().set_option("toast", "Saved").unwrap();
    h.run_steps(20);
    let shrunk = h
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("read-aloud-bar")))
        .unwrap();
    assert!(
        shrunk.width() < 400.0,
        "minimized for toast: {shrunk:?} vs {bar:?}"
    );
    // Toast cleared: the player eases back to its set width.
    h.state_mut().queue(Command::ClearToasts);
    h.run_steps(30);
    let back = h
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("read-aloud-bar")))
        .unwrap();
    assert!(
        (back.width() - bar.width()).abs() < 2.0,
        "restored after toast: {back:?} vs {bar:?}"
    );
}

#[test]
fn player_rises_above_the_selection_bar_and_comes_back_down() {
    use egui_kittest::kittest::Queryable as _;
    use mareader_ui::library::{Action, Pick, PickAction};

    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("books", "1").ok();
    app.set_option("pdf", "sample:3").ok();
    app.set_option("set", "read_aloud.engine:\"silent\"").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    start(&mut h, ReadFrom::default());
    let book = library(&h).books().next().unwrap().id;
    h.state_mut().queue(Command::ShowLibrary);
    h.run_steps(40);
    let bar_rect = |h: &Harness<MareaderApp>| {
        h.ctx
            .memory(|m| m.area_rect(egui::Id::new("read-aloud-bar")))
    };
    let low = bar_rect(&h).expect("the player's bar");
    // A pick summons the selection bar along the foot, under the player.
    h.state_mut()
        .queue(Command::Library(Action::Pick(PickAction::Only(vec![
            Pick::Book(book),
        ]))));
    h.run_steps(20);
    let risen = bar_rect(&h).expect("the player's bar");
    assert!(
        risen.bottom() < low.bottom() - 20.0,
        "{risen:?} should rise well above {low:?}"
    );
    // Selection done: it glides back to where it sat.
    h.get_by_label("Done").click();
    h.run_steps(30);
    let back = bar_rect(&h).expect("the player's bar");
    assert!(
        (back.bottom() - low.bottom()).abs() < 2.0,
        "{back:?} should return to {low:?}"
    );
}

#[test]
fn the_players_first_conflict_uses_an_animated_tuck() {
    let mut h = reader(1);
    start(&mut h, ReadFrom::default());
    h.get_by_label("Pause").click();
    h.run_steps(300);
    let id = egui::Id::new("read-aloud-bar");
    let home = h.ctx.memory(|m| m.area_rect(id)).expect("the player");
    let window = &mut h.state_mut().settings_mut().dictionary.window;
    window.open = true;
    window.minimized = false;
    window.rect = Some([home.left() - 100.0, home.top() - 120.0, 500.0, 240.0]);
    let mut moving = None;
    for _ in 0..20 {
        h.run_steps(1);
        let rect = h.ctx.memory(|m| m.area_rect(id)).expect("the player");
        let visible = rect.intersect(h.ctx.content_rect());
        if visible.height() < home.height() - 1.0 {
            moving = Some(visible);
            break;
        }
    }
    let visible = moving.expect("the first tuck starts moving");
    assert!(
        visible.height() > PEEK + 2.0,
        "first tuck snapped: {visible:?}"
    );
}

#[test]
fn a_covered_player_in_tab_mode_keeps_a_play_button() {
    let mut h = reader(1);
    h.state_mut().settings_mut().workspace.edge_parking = mareader_settings::EdgeParking::Tab;
    let window = &mut h.state_mut().settings_mut().dictionary.window;
    window.open = true;
    window.minimized = false;
    window.rect = Some([260.0, 400.0, 380.0, 240.0]);
    h.run_steps(30);
    start(&mut h, ReadFrom::default());
    h.get_by_label("Pause").click();
    h.run_steps(360);
    let orb = h
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("read-aloud-orb")));
    assert!(orb.is_some(), "covered player has an orb");
    assert!(h.query_by_label("Play").is_some());
    h.get_by_label("Play").click();
    h.run_steps(3);
    assert!(h.query_by_label("Pause").is_some());
}

#[test]
fn the_edge_orb_animates_back_when_the_dictionary_closes() {
    let mut h = reader(1);
    h.state_mut().settings_mut().workspace.edge_parking = mareader_settings::EdgeParking::Tab;
    let window = &mut h.state_mut().settings_mut().dictionary.window;
    window.open = true;
    window.minimized = false;
    window.rect = Some([260.0, 400.0, 380.0, 240.0]);
    h.run_steps(30);
    start(&mut h, ReadFrom::default());
    h.get_by_label("Pause").click();
    h.run_steps(360);
    let orb_id = egui::Id::new("read-aloud-orb");
    let from = h.ctx.memory(|m| m.area_rect(orb_id)).expect("edge orb");
    h.state_mut().settings_mut().dictionary.window.open = false;
    h.run_steps(2);
    let moving = h
        .ctx
        .memory(|m| m.area_rect(orb_id))
        .expect("returning orb");
    assert!(
        moving.left() < from.left() - 1.0,
        "orb should move home: {from:?} to {moving:?}"
    );
    assert!(h.query_by_label("Play").is_some());
    h.run_steps(60);
    assert!(h.query_by_label("Play").is_some());
    let bar = h
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("read-aloud-bar")))
        .expect("restored bar");
    assert!(bar.width() > 160.0);
}

#[test]
fn the_player_peeks_as_its_shell_and_restores_on_click() {
    let mut h = reader(1);
    let window = &mut h.state_mut().settings_mut().dictionary.window;
    window.open = true;
    window.minimized = false;
    window.rect = Some([260.0, 400.0, 380.0, 240.0]);
    h.run_steps(30);
    let dict_home = h.state().dict().window_rect().expect("dictionary window");

    start(&mut h, ReadFrom::default());
    h.get_by_label("Pause").click();
    h.run_steps(60);
    let id = egui::Id::new("read-aloud-bar");
    let home = h.ctx.memory(|m| m.area_rect(id)).expect("the player");
    let bottom = h.ctx.content_rect().bottom();
    let aside = h.state().dict().window_rect().expect("dictionary steps up");
    assert!(aside.top() < dict_home.top() - 20.0, "{aside:?}");
    assert!(
        h.query_by_label("Play").is_some(),
        "the summoned player stays up"
    );

    h.run_steps(240);
    h.run_steps(60);
    let tucked = h.ctx.memory(|m| m.area_rect(id)).expect("the player peek");
    let visible = tucked.intersect(h.ctx.content_rect());
    assert_eq!(tucked.size(), home.size());
    assert!((visible.height() - PEEK).abs() < 1.0, "{visible:?}");
    assert!(
        (visible.bottom() - bottom).abs() < 2.0,
        "the shell reaches the bottom edge: {visible:?} vs {bottom}"
    );
    assert!(h.query_by_label("Pause").is_none());
    assert!(h.query_by_label("Play").is_none());
    assert!(h.query_by_label("Show the read-aloud player").is_some());

    let edge = egui::pos2(tucked.center().x, tucked.top() + PEEK * 0.5);
    pointer(&mut h, edge, None);
    h.run_steps(20);
    let lifted = h
        .ctx
        .memory(|m| m.area_rect(id))
        .expect("the lifted player");
    let lifted_peek = lifted.intersect(h.ctx.content_rect());
    assert!(lifted_peek.height() > PEEK + 5.0, "{lifted_peek:?}");
    pointer(&mut h, egui::pos2(30.0, 30.0), None);
    h.run_steps(20);
    let settled = h
        .ctx
        .memory(|m| m.area_rect(id))
        .expect("the settled player");
    let settled_peek = settled.intersect(h.ctx.content_rect());
    assert!(
        (settled_peek.height() - PEEK).abs() < 1.0,
        "{settled_peek:?}"
    );

    let edge = egui::pos2(settled.center().x, settled.top() + PEEK * 0.5);
    pointer(&mut h, edge, None);
    pointer(&mut h, edge, Some(true));
    pointer(&mut h, edge, Some(false));
    h.run_steps(60);

    let restored = h.ctx.memory(|m| m.area_rect(id)).expect("restored player");
    assert!(
        (restored.bottom() - home.bottom()).abs() < 2.0,
        "{restored:?}"
    );
    assert!(
        h.query_by_label("Play").is_some(),
        "controls return after reveal"
    );
    let dict = h.state().dict().window_rect().expect("dictionary window");
    assert!(dict.top() < 380.0, "the dictionary moves up: {dict:?}");
    h.run_steps(240);
    assert!(
        h.query_by_label("Play").is_some(),
        "manual reveal stays open"
    );
    let dict = h
        .state()
        .dict()
        .window_rect()
        .expect("dictionary stays aside");
    assert!(
        dict.top() < dict_home.top() - 20.0,
        "it stays clear past the grace period: {dict:?}"
    );
    let at = egui::pos2(dict.left() + 100.0, dict.top() + 20.0);
    pointer(&mut h, at, None);
    pointer(&mut h, at, Some(true));
    pointer(&mut h, at, Some(false));
    h.run_steps(60);
    assert!(
        h.query_by_label("Show the read-aloud player").is_some(),
        "using the dictionary returns priority to it"
    );
}

#[test]
fn the_last_used_float_keeps_priority_and_shrinks_the_dictionary() {
    use mareader_settings::Player;

    let mut h = reader(1);
    start(&mut h, ReadFrom::default());
    h.get_by_label("Pause").click();
    h.run_steps(300);
    assert!(h.query_by_label("Expand player").is_some());
    let id = egui::Id::new("read-aloud-bar");
    let compact = h.ctx.memory(|m| m.area_rect(id)).expect("compact player");
    let room_top = h.ctx.content_rect().top() + mareader_ui::theme::TITLE_BAR_H + 12.0;
    let height = 420.0_f32.min(compact.top() - room_top - 16.0);
    assert!(height > 240.0, "not enough room for the test: {height}");
    let top = compact.top() - height - 4.0;
    let window = &mut h.state_mut().settings_mut().dictionary.window;
    window.open = true;
    window.minimized = false;
    window.rect = Some([150.0, top, 600.0, height]);
    h.run_steps(30);
    let home = h.state().dict().window_rect().expect("dictionary home");
    assert!(!home.intersects(compact), "compact player fits: {home:?}");

    h.get_by_label("Expand player").click();
    h.run_steps(30);
    h.state_mut().settings_mut().read_aloud.player = Player::Full;
    h.run_steps(60);
    assert!(h.query_by_label("Collapse player").is_some());
    let full = h.ctx.memory(|m| m.area_rect(id)).expect("full player");
    assert!(
        home.intersects(full),
        "full player needs room: {home:?} vs {full:?}"
    );
    let aside = h
        .state()
        .dict()
        .window_rect()
        .expect("dictionary steps clear");
    assert!(aside.height() < 240.0, "it shortens to fit: {aside:?}");
    assert!(!aside.intersects(full), "{aside:?} vs {full:?}");

    let at = egui::pos2(aside.left() + 100.0, aside.top() + 20.0);
    pointer(&mut h, at, None);
    pointer(&mut h, at, Some(true));
    pointer(&mut h, at, Some(false));
    h.run_steps(60);
    assert!(
        h.query_by_label("Show the read-aloud player").is_some(),
        "using the dictionary returns priority to it"
    );
    let tucked = h.ctx.memory(|m| m.area_rect(id)).expect("parked player");
    let visible = tucked.intersect(h.ctx.content_rect());
    assert!((visible.height() - PEEK).abs() < 1.0, "{visible:?}");
}

#[test]
fn a_remembered_skip_belongs_to_one_book_and_can_be_undone() {
    use mareader_ui::documents::OpenRequest;
    let dir = tempfile::tempdir().expect("temporary library");
    let (mut h, kept_path) = epub_from_file(dir.path());
    h.state_mut().settings_mut().read_aloud.skips.tables = mareader_settings::Skip {
        on: true,
        ask: true,
    };
    let at = first_table(&h).expect("the sample has a table");
    let from = ReadFrom {
        anchor: None,
        at: Some(Spot {
            page: at.page.saturating_sub(1),
            char: 0,
        }),
        until: None,
    };
    start(&mut h, from);
    step_until(&mut h, |h| h.query_by_label("Read the table").is_some());
    h.run_steps(20);
    let remember = "Always skip tables in this book";
    h.get_by_label(remember).click();
    h.run_steps(2);
    h.get_by_label("Skip the table").click();
    let past =
        |h: &Harness<'static, MareaderApp>| !lit_in_table(h) && lit(h).is_some_and(|(s, _)| s > at);
    step_until(&mut h, past);
    h.run_steps(30);
    assert_eq!(h.state().settings().read_aloud.book_skips.len(), 1);

    // Another book still asks at its own table.
    h.get_by_label("Stop reading aloud").click();
    h.run_steps(5);
    let other_path = dir.path().join("Other.epub");
    let other_bytes = mareader_epub::sample::epub(8, "Other");
    std::fs::write(&other_path, other_bytes).expect("a second sample file");
    let pane = h.state().active_pane();
    h.state_mut().queue(Command::Open(OpenRequest {
        pane: Some(pane),
        ..OpenRequest::path(other_path)
    }));
    step_until(&mut h, |h| {
        let docs = h.state().documents();
        docs.path(pane).is_some_and(|p| p.ends_with("Other.epub"))
            && docs.page_count(pane).is_some_and(|n| n > 4)
    });
    start(&mut h, from);
    step_until(&mut h, |h| h.query_by_label("Read the table").is_some());
    assert!(
        h.query_by_label("Read the table").is_some(),
        "another book is unaffected"
    );
    h.get_by_label("Stop reading aloud").click();
    h.run_steps(5);

    // Reopening skips asking; options can undo it.
    h.state_mut().queue(Command::Open(OpenRequest {
        pane: Some(pane),
        ..OpenRequest::path(kept_path)
    }));
    step_until(&mut h, |h| {
        let docs = h.state().documents();
        docs.path(pane).is_some_and(|p| p.ends_with("Kept.epub"))
    });
    start(&mut h, from);
    let asked = std::cell::Cell::new(false);
    step_until(&mut h, |h| {
        asked.set(asked.get() || h.query_by_label("Read the table").is_some());
        past(h)
    });
    assert!(!asked.get(), "remembered book skips without asking");
    h.get_by_label("Read-aloud options").click();
    h.run_steps(3);
    h.get_by_label("Skip tables").click();
    h.run_steps(3);
    assert!(h.state().settings().read_aloud.book_skips.is_empty());
}
