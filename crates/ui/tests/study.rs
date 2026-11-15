//! The selection bar, Explain, marks and the word card, headless.

use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use mareader_library::{Pen, Spot};
use mareader_pdf::TextLayer;
use mareader_session::Anchor;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::selection::Sheet;
use mareader_ui::study::words;

use crate::common::{ai_reader as reader, find, page};

type H = Harness<'static, MareaderApp>;
type Out = Result<(), Box<dyn std::error::Error>>;

/// Character `i`'s middle on screen.
fn middle(layer: &TextLayer, sheet: Sheet, i: usize) -> Result<Pos2, String> {
    let c = layer.text.chars().get(i).ok_or("no such character")?;
    Ok(sheet.to_screen(c.bounds).center())
}

fn press(h: &mut H, pos: Pos2, button: PointerButton) {
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run();
}

fn double_click(h: &mut H, pos: Pos2) {
    h.hover_at(pos);
    h.step();
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.step();
        }
    }
    h.run();
}

/// Selects the first `text` on `page`, as a drag would.
fn select(h: &mut H, page: usize, text: &str) {
    let pane = h.state().active_pane();
    let text = text.to_owned();
    h.state_mut()
        .queue(Command::SelectText { pane, page, text });
    h.run();
}

fn marks(h: &H) -> Vec<Spot> {
    let pane = h.state().active_pane();
    h.state().marks(pane).into_iter().map(|m| m.spot).collect()
}

#[test]
fn explain_marks_the_word_and_its_card_comes_back_on_a_click() -> Out {
    let mut h = reader(&[("pdf", "sample:5")]);
    let (layer, sheet) = page(&h, 0)?;
    let quick = find(&layer, "quick")?;
    double_click(&mut h, middle(&layer, sheet, quick + 2)?);
    let pane = h.state().active_pane();
    assert!(h.state().documents().selection(pane).is_some());
    // Released: the bar is up under the word.
    h.get_by_label("Copy");
    h.get_by_label("AI Dictionary").click();
    h.run();
    let study = h.state().study();
    assert_eq!(study.card_word(), Some("quick"));
    let pos = study.card_answer().map(|a| a.pos.clone());
    assert_eq!(pos.as_deref(), Some("adjective"));
    assert!(h.state().documents().selection(pane).is_none());
    let spots = marks(&h);
    assert!(
        matches!(spots.as_slice(), [Spot::Page { page: 0, .. }]),
        "{spots:?}"
    );
    h.get_by_label("Synonyms");
    // Who answered, and a way to ask the AI chosen now.
    h.get_by_label("Sample answers");
    h.get_by_label("Ask again");

    // A click elsewhere closes the card; a click on the mark brings it.
    let blank = sheet.rect.left_top() + egui::vec2(12.0, 12.0);
    press(&mut h, blank, PointerButton::Primary);
    assert!(!h.state().study().card_open());
    press(
        &mut h,
        middle(&layer, sheet, quick + 1)?,
        PointerButton::Primary,
    );
    assert_eq!(h.state().study().card_word(), Some("quick"));
    // Explaining the same word there again keeps one mark.
    select(&mut h, 0, "quick");
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    assert_eq!(marks(&h).len(), 1);
    Ok(())
}

#[test]
fn a_right_click_removes_a_mark_and_undo_brings_it_back() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("selection", "0:quick"),
        ("gloss", "open"),
    ]);
    h.run();
    assert_eq!(marks(&h).len(), 1, "the options mark the word");
    let (layer, sheet) = page(&h, 0)?;
    let at = middle(&layer, sheet, find(&layer, "quick")? + 2)?;
    press(&mut h, at, PointerButton::Secondary);
    assert!(!h.state().study().card_open(), "the menu replaces the card");
    h.get_by_label("Remove highlight").click();
    h.run();
    assert!(marks(&h).is_empty());
    h.get_by_label("Undo").click();
    h.run();
    assert_eq!(marks(&h).len(), 1);
    Ok(())
}

#[test]
fn a_mark_s_menu_rides_along_with_its_words_as_the_page_scrolls() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("selection", "0:quick"),
        ("gloss", "open"),
    ]);
    h.run();
    let (layer, sheet) = page(&h, 0)?;
    let i = find(&layer, "quick")?;
    let at = middle(&layer, sheet, i + 2)?;
    press(&mut h, at, PointerButton::Secondary);
    let words = |h: &H| -> Result<Rect, String> {
        let (layer, sheet) = page(h, 0)?;
        let first = middle(&layer, sheet, i)?;
        let last = middle(&layer, sheet, i + 4)?;
        Ok(Rect::from_two_pos(first, last))
    };
    // Snug against its words: just under them, or just over.
    let snug = |menu: Rect, w: Rect| {
        let gap = (menu.top() - w.bottom())
            .abs()
            .min((menu.bottom() - w.top()).abs());
        gap < 16.0
    };
    let before = words(&h)?;
    let menu = h.state().study().menu_rect().ok_or("a menu")?;
    assert!(snug(menu, before), "{menu:?} beside {before:?}");
    h.hover_at(at);
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -60.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run();
    let after = words(&h)?;
    assert!(
        (before.top() - after.top()).abs() > 10.0,
        "the page scrolled"
    );
    let menu = h.state().study().menu_rect().ok_or("the menu stays")?;
    assert!(snug(menu, after), "{menu:?} follows {after:?}");
    Ok(())
}

#[test]
fn a_phrase_is_not_explained_and_nothing_is_marked() -> Out {
    let mut h = reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    select(&mut h, 0, "quick brown");
    assert!(h.state().documents().selection(pane).is_some());
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    assert!(!h.state().study().card_open());
    assert!(marks(&h).is_empty());
    Ok(())
}

#[test]
fn a_failed_answer_says_why_and_offers_a_retry() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("explain", "none")]);
    let pane = h.state().active_pane();
    select(&mut h, 0, "quick");
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    assert!(h.state().study().card_open());
    assert!(h.state().study().card_answer().is_none());
    // Nothing on this machine can answer: no retry is offered.
    assert!(h.query_by_label("Try again").is_none());
    assert_eq!(marks(&h).len(), 1, "the word is marked all the same");
    Ok(())
}

#[test]
fn a_book_s_mark_is_found_again_after_the_type_changes() -> Out {
    let mut h = reader(&[("epub", "sample:2"), ("view", "single")]);
    let pane = h.state().active_pane();
    let page = (0..8).find(|&p| {
        select(&mut h, p, "customs");
        h.state().documents().selection(pane).is_some()
    });
    let page = page.ok_or("no customs in the sample")?;
    h.state_mut().queue(Command::GoTo { pane, page });
    h.run();
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    assert_eq!(h.state().study().card_word(), Some("customs"));
    let spots = marks(&h);
    let [Spot::Text { block, start, end }] = spots.as_slice() else {
        return Err(format!("{spots:?}").into());
    };
    let (block, bytes) = (*block, *start..*end);
    assert_eq!(bytes.len(), "customs".len());

    h.state_mut().set_option("text_size", "30")?;
    h.run_steps(10);
    h.run();
    let reading = h.state().documents().reading(pane).ok_or("no book")?;
    let doc = reading.document;
    let anchor = Anchor {
        block,
        offset: bytes.start,
    };
    let page = doc.locate(anchor).ok_or("not located")?;
    let layer = doc.text_layer(page)?;
    let chars = words::find(&layer, block, bytes).ok_or("not found again")?;
    assert_eq!(layer.text.text(chars), "customs");
    Ok(())
}

#[test]
fn a_library_book_keeps_its_marks() -> Out {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("sample.pdf");
    std::fs::write(&file, mareader_pdf::sample::pdf(5, "A Kept Sample"))?;
    let path = file.to_string_lossy().into_owned();
    let mut h = reader(&[("pdf", path.as_str())]);
    h.run_steps(5);
    h.run();
    let pane = h.state().active_pane();
    select(&mut h, 0, "quick");
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    let lib = h.state().library();
    let book = lib.books().next().ok_or("not in the library")?.id;
    let kept: Vec<String> = lib.marks(book).iter().map(|m| m.word.clone()).collect();
    assert_eq!(kept, ["quick"]);
    let said = lib.marks(book).first().and_then(|m| m.gloss.value.clone());
    assert_eq!(said.map(|g| g.by).as_deref(), Some("Sample answers"));
    // Another AI keeps the answer: the card comes back answered, unasked.
    h.state_mut().set_option("explain", "none")?;
    select(&mut h, 0, "quick");
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    assert!(h.state().study().card_answer().is_some());
    Ok(())
}

#[test]
fn the_options_turn_to_the_page_and_open_the_card() -> Out {
    let opts = [
        ("epub", "sample:2"),
        ("selection", "1:customs"),
        ("gloss", "open"),
    ];
    let mut h = reader(&opts);
    h.run_steps(10);
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let pages: Vec<usize> = docs.sheets(pane).iter().map(|s| s.page).collect();
    let state = format!("{:?} {pages:?} {:?}", marks(&h), docs.selection(pane));
    assert_eq!(h.state().study().card_word(), Some("customs"), "{state}");
    assert_eq!(marks(&h).len(), 1);
    Ok(())
}

#[test]
fn the_bar_highlights_and_underlines_and_a_click_offers_removal() -> Out {
    let mut h = reader(&[("pdf", "sample:5")]);
    let (layer, sheet) = page(&h, 0)?;
    let quick = find(&layer, "quick brown")?;
    select(&mut h, 0, "quick brown");
    // Half is offered only once the reader adds it to the menu.
    assert!(h.query_by_label("Half highlight").is_none());
    h.get_by_label("Highlight").click();
    h.run();
    let pane = h.state().active_pane();
    assert!(h.state().documents().selection(pane).is_none());
    let (from, to) = ([0, quick], [0, quick + 11]);
    assert_eq!(marks(&h), [Spot::Chars { from, to }]);
    select(&mut h, 0, "lazy");
    h.get_by_label("Underline").click();
    h.run();
    let pens: Vec<Pen> = h.state().marks(pane).iter().map(|m| m.ink.pen).collect();
    assert_eq!(pens, [Pen::Full, Pen::Underline]);
    let yellow = h.state().settings().study.highlights.full;
    assert_eq!(
        h.state().marks(pane).first().map(|m| m.ink.color),
        Some(yellow)
    );
    // A click on the reader's own highlight offers removal, not a card.
    press(
        &mut h,
        middle(&layer, sheet, quick + 2)?,
        PointerButton::Primary,
    );
    assert_eq!(h.state().study().card_word(), None);
    h.get_by_label("Remove highlight").click();
    h.run();
    assert_eq!(marks(&h).len(), 1);
    Ok(())
}

#[test]
fn asking_for_a_colour_each_time_never_offers_the_ai_look() -> Out {
    use mareader_settings::{BarItem, PickColor};
    let mut h = reader(&[("pdf", "sample:5")]);
    let look = &mut h.state_mut().settings_mut().study.highlights;
    look.pick = PickColor::Ask;
    look.bar.push(BarItem::Highlight(Pen::Half));
    let violet = look.ai.color.to_hex();
    select(&mut h, 0, "quick");
    h.get_by_label("Highlight").click();
    h.run();
    // The AI marks words in violet with this pen, so it is left out.
    assert!(h.query_by_label(&violet).is_none());
    h.get_by_label("#ffbe00").click();
    h.run();
    let pane = h.state().active_pane();
    let shown = h.state().marks(pane);
    let ink = shown.first().map(|m| (m.ink.pen, m.ink.color.to_hex()));
    assert_eq!(ink, Some((Pen::Full, "#ffbe00".to_owned())));
    // Half was added to the menu, where the AI's colour is fine.
    select(&mut h, 0, "lazy");
    h.get_by_label("Half highlight").click();
    h.run();
    h.get_by_label(&violet);
    Ok(())
}

#[test]
fn the_options_highlight_a_book_across_its_blocks() -> Out {
    let opts = [
        ("epub", "sample:2"),
        ("selection", "1:customs"),
        ("highlight", "underline"),
    ];
    let mut h = reader(&opts);
    h.run_steps(10);
    let spots = marks(&h);
    assert!(
        matches!(spots.as_slice(), [Spot::Blocks { .. }]),
        "{spots:?}"
    );
    Ok(())
}

#[test]
fn the_highlights_tab_sets_colours_the_menu_and_keeps_the_ai_look() {
    use mareader_appearance::Rgb;
    use mareader_settings::BarItem;
    use mareader_ui::settings_window::Tab;
    let mut h = reader(&[("pdf", "sample")]);
    h.set_size(egui::vec2(1000.0, 1200.0));
    h.state_mut().queue(Command::OpenSettings(Tab::Highlights));
    h.run();
    // The AI marks words with this pen and colour: it can't be picked.
    h.get_by_label("Highlight #a58af0").click();
    h.get_by_label("Highlight #ffbe00").click();
    h.run();
    let look = &h.state().settings().study.highlights;
    assert_eq!(look.full, Rgb(0xff, 0xbe, 0x00));
    // Half joins the menu; Copy moves down past Explain.
    h.get_by_role_and_label(Role::CheckBox, "Half highlight")
        .click();
    h.get_by_label("Move Copy down").click();
    h.run();
    let bar = &h.state().settings().study.highlights.bar;
    let want = [
        BarItem::Explain,
        BarItem::Copy,
        BarItem::LookUp,
        BarItem::ReadAloud,
        BarItem::Note,
        BarItem::Highlight(Pen::Full),
        BarItem::Highlight(Pen::Underline),
        BarItem::Highlight(Pen::Half),
    ];
    assert_eq!(bar.as_slice(), want);
    // Half takes violet, then the AI takes Half: Half moves on.
    h.get_by_label("Half highlight #a58af0").click();
    h.run();
    // The AI's look waits in the AI tab, under its name.
    h.get_by_label("Change in AI").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "AI Dictionary")
        .click();
    h.run();
    let ai_half = h.get_by_role_and_label(Role::Button, "Half highlight");
    ai_half.scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::Button, "Half highlight")
        .click();
    h.run();
    let look = &h.state().settings().study.highlights;
    assert_eq!(look.ai.pen, Pen::Half);
    assert_ne!(look.half, look.ai.color);
}

#[test]
fn marks_kept_off_the_pens_show_only_while_in_use() {
    use mareader_ui::settings_window::Tab;
    let mut h = reader(&[("pdf", "sample"), ("explain", "none")]);
    h.set_size(egui::vec2(1000.0, 1200.0));
    h.state_mut().queue(Command::OpenSettings(Tab::Highlights));
    h.run();
    // No model answers and no word levels are in: nothing to show.
    assert!(h.query_by_label("Change in AI").is_none());
    assert!(h.query_by_label("Change in Vocabulary").is_none());
    assert!(h.query_by_label("Other marks").is_none());
}

#[test]
fn the_ai_tab_picks_who_explains() {
    use mareader_settings::Provider;
    use mareader_ui::settings_window::Tab;
    let mut h = reader(&[("pdf", "sample")]);
    h.state_mut().queue(Command::OpenSettings(Tab::Ai));
    h.run();
    // With none saved, the AI tab's one button leads to the models.
    if h.query_by_label("Add a model").is_some() {
        h.get_by_label("Add a model").click();
    } else {
        h.state_mut().queue(Command::OpenSettings(Tab::Models));
    }
    h.run();
    // Tiles show at once while none is saved; else they open on a click.
    if h.query_by_label("+  Add a model").is_some() {
        h.get_by_label("+  Add a model").click();
        h.run();
    }
    h.get_by_label("OpenAI").click();
    h.run();
    let ai = &h.state().settings().ai;
    let openai = ai.presets.iter().find(|p| p.provider == Provider::OpenAi);
    let id = openai.map(|p| p.id);
    assert!(id.is_some(), "{:?}", ai.presets);
    if !cfg!(target_os = "macos") {
        assert_eq!(ai.tasks.explain.preset, id, "the first model explains");
    }
    // Its provider opens below it, address and all.
    assert!(h.query_all_by_label("Address").next().is_some());
}

#[test]
fn several_marks_are_picked_and_removed_together() -> Out {
    let mut h = reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    for word in ["quick", "lazy"] {
        select(&mut h, 0, word);
        let (pen, color) = (Pen::Full, None);
        h.state_mut().queue(Command::Highlight { pane, pen, color });
        h.run();
    }
    assert_eq!(marks(&h).len(), 2);
    let (layer, sheet) = page(&h, 0)?;
    let quick = middle(&layer, sheet, find(&layer, "quick")? + 2)?;
    let lazy = middle(&layer, sheet, find(&layer, "lazy")? + 1)?;
    // The menu's Select several, then a click takes the other in.
    press(&mut h, quick, PointerButton::Secondary);
    h.get_by_label("Select several").click();
    h.run();
    assert!(h.state().study().picking());
    press(&mut h, lazy, PointerButton::Primary);
    h.get_by_label("2 selected");
    h.get_by_label("Remove (2)").click();
    h.run();
    assert!(marks(&h).is_empty());
    assert!(!h.state().study().picking());
    h.get_by_label("Undo").click();
    h.run();
    assert_eq!(marks(&h).len(), 2);

    // A long press picks; letting go doesn't pick it off again.
    h.hover_at(quick);
    h.step();
    let button = PointerButton::Primary;
    let modifiers = Modifiers::NONE;
    let pos = quick;
    h.event(Event::PointerButton {
        pos,
        button,
        pressed: true,
        modifiers,
    });
    h.run_steps(45);
    h.event(Event::PointerButton {
        pos,
        button,
        pressed: false,
        modifiers,
    });
    h.step();
    h.run();
    assert_eq!(h.state().study().picked(pane).map(|p| p.len()), Some(1));
    h.get_by_label("All").click();
    h.run();
    h.get_by_label("2 selected");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().study().picking());
    assert_eq!(marks(&h).len(), 2, "nothing removed");
    Ok(())
}

fn noted(h: &H) -> Vec<bool> {
    let pane = h.state().active_pane();
    h.state().marks(pane).iter().map(|m| m.noted).collect()
}

#[test]
fn a_note_is_written_read_deleted_and_brought_back() -> Out {
    let mut h = reader(&[("pdf", "sample:5")]);
    let (layer, sheet) = page(&h, 0)?;
    let quick = find(&layer, "quick brown")?;
    select(&mut h, 0, "quick brown");
    h.get_by_label("Note").click();
    h.run();
    // The words are highlighted at once, under an empty draft.
    assert_eq!(noted(&h), [false]);
    let draft = h
        .state()
        .study()
        .note_open()
        .map(|(_, d)| d.map(str::to_owned));
    assert_eq!(draft, Some(Some(String::new())));
    h.event(Event::Text("Check the fox.".into()));
    h.run();
    h.get_by_label("Save").click();
    h.run();
    assert!(h.state().study().note_open().is_none());
    assert_eq!(noted(&h), [true]);
    // A click on a noted highlight reads the note.
    press(
        &mut h,
        middle(&layer, sheet, quick + 2)?,
        PointerButton::Primary,
    );
    h.get_by_label("Check the fox.");
    h.get_by_label("Edit").click();
    h.run();
    let draft = h
        .state()
        .study()
        .note_open()
        .and_then(|(_, d)| d.map(str::to_owned));
    assert_eq!(draft.as_deref(), Some("Check the fox."));
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(noted(&h), [true], "Escape keeps the saved note");
    press(
        &mut h,
        middle(&layer, sheet, quick + 2)?,
        PointerButton::Primary,
    );
    h.get_by_label("Delete note").click();
    h.run();
    assert_eq!(noted(&h), [false], "the highlight stays");
    h.get_by_label("Undo").click();
    h.run();
    assert_eq!(noted(&h), [true]);
    Ok(())
}

#[test]
fn a_cancelled_note_takes_back_only_the_highlight_it_made() -> Out {
    let mut h = reader(&[("pdf", "sample:5")]);
    select(&mut h, 0, "lazy");
    h.get_by_label("Note").click();
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(marks(&h).is_empty(), "the note's highlight went with it");
    select(&mut h, 0, "lazy");
    h.get_by_label("Highlight").click();
    h.run();
    select(&mut h, 0, "lazy");
    h.get_by_label("Note").click();
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!(marks(&h).len(), 1, "a highlight made before stays");
    Ok(())
}

#[test]
fn the_notes_panel_lists_highlights_and_filters_to_notes() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("sidebar", "notes")]);
    h.get_by_label("No highlights yet");
    select(&mut h, 0, "lazy");
    h.get_by_label("Highlight").click();
    h.run();
    select(&mut h, 0, "quick brown");
    h.get_by_label("Note").click();
    h.run();
    h.event(Event::Text("Check the fox.".into()));
    h.run();
    h.get_by_label("Save").click();
    h.run();
    // In reading order, the note read out with its words.
    h.get_by_label("quick brown. Note: Check the fox.");
    h.get_by_label("lazy");
    h.get_by_label("Notes · 1").click();
    h.run();
    assert!(h.query_by_label("lazy").is_none());
    h.get_by_label("quick brown. Note: Check the fox.");
    Ok(())
}

#[test]
fn a_long_note_opens_whole_in_the_notes_panel() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("sidebar", "notes")]);
    select(&mut h, 0, "quick brown");
    h.get_by_label("Note").click();
    h.run();
    h.event(Event::Text("Nevermore, quoth the raven. ".repeat(14)));
    h.run();
    h.get_by_label("Save").click();
    h.run();
    // Cut at four lines, it says so, and opens in place.
    h.get_by_label("Show more").click();
    h.run();
    h.get_by_label("Show less").click();
    h.run();
    h.get_by_label("Show more");
    Ok(())
}

#[test]
fn a_notes_row_s_menu_is_menu_wide_not_sidebar_wide() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("sidebar", "notes")]);
    select(&mut h, 0, "lazy");
    h.get_by_label("Highlight").click();
    h.run();
    let row = h.get_by_label("lazy").rect();
    press(&mut h, row.center(), PointerButton::Secondary);
    let item = h.get_by_label("Remove highlight").rect();
    assert!(item.width() <= 210.0, "{:.0} wide", item.width());
    assert!(item.width() < row.width() - 40.0, "as wide as its row");
    Ok(())
}

fn ink(h: &H) -> Vec<(Pen, String)> {
    let pane = h.state().active_pane();
    let shown = h.state().marks(pane);
    shown
        .iter()
        .map(|m| (m.ink.pen, m.ink.color.to_hex()))
        .collect()
}

#[test]
fn a_highlight_is_restyled_from_its_menu_but_never_as_the_ai() -> Out {
    let mut h = reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    select(&mut h, 0, "quick");
    let (pen, color) = (Pen::Full, None);
    h.state_mut().queue(Command::Highlight { pane, pen, color });
    h.run();
    let (layer, sheet) = page(&h, 0)?;
    let quick = middle(&layer, sheet, find(&layer, "quick")? + 2)?;
    press(&mut h, quick, PointerButton::Secondary);
    // Another pen keeps the colour; the menu stays to try more.
    h.get_by_role_and_label(Role::RadioButton, "Underline")
        .click();
    h.run();
    assert_eq!(ink(&h), [(Pen::Underline, "#ffff00".to_owned())]);
    h.get_by_label("#ff0000").click();
    h.run();
    assert_eq!(ink(&h), [(Pen::Underline, "#ff0000".to_owned())]);
    // Half in violet is free, but full violet is the AI's.
    h.get_by_role_and_label(Role::RadioButton, "Half highlight")
        .click();
    h.run();
    h.get_by_label("#a58af0").click();
    h.run();
    assert_eq!(ink(&h), [(Pen::Half, "#a58af0".to_owned())]);
    h.get_by_role_and_label(Role::RadioButton, "Highlight")
        .click();
    h.run();
    assert_eq!(ink(&h), [(Pen::Full, "#ffff00".to_owned())], "its own");
    assert!(h.get_by_label("#a58af0").accesskit_node().is_disabled());
    h.get_by_label("#a58af0").click();
    h.run();
    assert_eq!(ink(&h), [(Pen::Full, "#ffff00".to_owned())]);
    Ok(())
}

#[test]
fn an_ai_mark_s_menu_offers_no_restyle() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("selection", "0:quick"),
        ("gloss", "open"),
    ]);
    h.run();
    let (layer, sheet) = page(&h, 0)?;
    let at = middle(&layer, sheet, find(&layer, "quick")? + 2)?;
    press(&mut h, at, PointerButton::Secondary);
    h.get_by_label("Remove highlight");
    assert!(
        h.query_by_role_and_label(Role::RadioButton, "Underline")
            .is_none()
    );
    Ok(())
}

/// What reached the clipboard this frame.
fn copied(h: &H) -> Option<String> {
    let commands = &h.output().platform_output.commands;
    commands.iter().find_map(|c| match c {
        egui::OutputCommand::CopyText(t) => Some(t.clone()),
        _ => None,
    })
}

#[test]
fn the_notes_panel_copies_and_offers_to_export_what_it_shows() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("sidebar", "notes"),
        ("selection", "0:lazy"),
        ("highlight", "full"),
        ("selection", "0:quick_brown"),
        ("noted", "Check_the_fox."),
    ]);
    for _ in 0..4 {
        h.run();
    }
    h.get_by_label("Copy all as Markdown").click();
    h.step();
    let md = copied(&h).ok_or("nothing copied")?;
    assert!(
        md.contains("> quick brown\n\n**Note:** Check the fox.\n\n*Page 1*"),
        "{md}"
    );
    assert!(
        md.find("quick brown") < md.find("> lazy"),
        "in reading order"
    );
    h.run();
    h.get_by_label("Copied 2 highlights · 1 note");
    // The Notes filter narrows both.
    h.get_by_label("Notes · 1").click();
    h.run();
    h.get_by_label("Copy all as Markdown").click();
    h.step();
    let md = copied(&h).ok_or("nothing copied")?;
    assert!(!md.contains("lazy"), "{md}");
    h.run();
    h.get_by_label("Export…").click();
    h.run();
    h.get_by_label("Export 1 highlight · 1 note");
    for format in ["Markdown (.md)", "Plain text (.txt)", "JSON (.json)"] {
        h.get_by_label(format);
    }
    Ok(())
}

#[test]
fn the_notes_panel_groups_by_chapter_and_searches() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("sidebar", "notes"),
        ("selection", "0:lazy"),
        ("highlight", "full"),
        ("selection", "1:quick_brown"),
        ("noted", "Check_the_fox."),
    ]);
    for _ in 0..4 {
        h.run();
    }
    // Headed by the outline's chapters, each with its count.
    h.get_by_label("Chapter 1: Opening, 1");
    h.get_by_label("1.1 Scrolling, 1");
    // A search reads quotes and notes, in any case.
    h.get_by_label("Search notes").click();
    h.run();
    h.event(Event::Text("FOX".into()));
    h.run();
    assert!(h.query_by_label("lazy").is_none());
    assert!(h.query_by_label("Chapter 1: Opening, 1").is_none());
    h.get_by_label("quick brown. Note: Check the fox.");
    // Exports follow the search too.
    h.get_by_label("Copy all as Markdown").click();
    h.step();
    let md = copied(&h).ok_or("nothing copied")?;
    assert!(!md.contains("lazy"), "{md}");
    h.run();
    // Copy took the focus; back to the field to type on.
    h.get_by_label("Search notes").click();
    h.run();
    h.event(Event::Text(" zebra".into()));
    h.run();
    h.get_by_label("No matches");
    h.get_by_label("Clear search").click();
    h.run();
    h.get_by_label("lazy");
    h.get_by_label("quick brown. Note: Check the fox.");
    Ok(())
}

fn current(h: &H) -> Option<usize> {
    let pane = h.state().active_pane();
    h.state().documents().reading(pane).map(|r| r.current)
}

#[test]
fn a_page_is_bookmarked_listed_and_taken_back_with_undo() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("sidebar", "notes")]);
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::D);
    h.run();
    h.get_by_label_contains("Bookmark, page 1: ");
    // A ribbon on the page, not a mark over its words.
    let pane = h.state().active_pane();
    assert!(h.state().marks(pane).is_empty());
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert_eq!(current(&h), Some(1));
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::D);
    h.run();
    h.get_by_label("Bookmarks · 2").click();
    h.run();
    h.get_by_label_contains("Bookmark, page 1: ").click();
    h.run();
    assert_eq!(current(&h), Some(0), "a bookmark goes to its page");
    // Again on a bookmarked page takes it back.
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::D);
    h.run();
    assert!(h.query_by_label_contains("Bookmark, page 1: ").is_none());
    h.get_by_label("Removed bookmark");
    h.get_by_label("Undo").click();
    h.run();
    h.get_by_label_contains("Bookmark, page 1: ");
    h.get_by_label_contains("Bookmark, page 2: ");
    Ok(())
}

#[test]
fn picked_highlights_are_restyled_together_where_each_may_be() -> Out {
    let mut h = reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    for (word, pen) in [("quick", Pen::Full), ("lazy", Pen::Underline)] {
        select(&mut h, 0, word);
        let color = None;
        h.state_mut().queue(Command::Highlight { pane, pen, color });
        h.run();
    }
    let (layer, sheet) = page(&h, 0)?;
    let quick = middle(&layer, sheet, find(&layer, "quick")? + 2)?;
    let lazy = middle(&layer, sheet, find(&layer, "lazy")? + 1)?;
    press(&mut h, quick, PointerButton::Secondary);
    h.get_by_label("Select several").click();
    h.run();
    press(&mut h, lazy, PointerButton::Primary);
    h.get_by_label("Style…").click();
    h.run();
    h.get_by_label("Style of 2 highlights");
    let sorted = |h: &H| {
        let mut v = ink(h);
        v.sort_by(|a, b| a.1.cmp(&b.1));
        v
    };
    let before = sorted(&h);
    // A pen: each keeps its colour.
    h.get_by_label("Half highlight").click();
    h.run();
    let half = [
        (Pen::Half, "#1677ff".to_owned()),
        (Pen::Half, "#ffff00".to_owned()),
    ];
    assert_eq!(sorted(&h), half);
    h.get_by_label("Restyled 2 highlights");
    h.get_by_label("Undo").click();
    h.run();
    assert_eq!(sorted(&h), before);
    assert!(h.state().study().picking(), "still picking");
    // Full red is CEFR's: only the underline takes it.
    h.get_by_label("Style…").click();
    h.run();
    h.get_by_label("#ff0000").click();
    h.run();
    let red = [
        (Pen::Underline, "#ff0000".to_owned()),
        (Pen::Full, "#ffff00".to_owned()),
    ];
    assert_eq!(sorted(&h), red);
    h.get_by_label("Restyled 1 of 2 highlights");
    Ok(())
}
