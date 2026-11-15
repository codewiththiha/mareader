//! Measures progressive EPUB opening and page-demand latency.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use mareader_session::{
    Budget, DocId, Event, OpenTarget, PageKey, Part, Scale, Session, Source, SpinePlace, Ticket,
    Typography,
};

const CHAPTERS: usize = 800;
const TARGET_CHAPTER: usize = CHAPTERS / 2;
const TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Budget = Budget {
    items: 32,
    bytes: 32 << 20,
};

fn resident(field: &str) -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = text.lines().find(|line| line.starts_with(field))?;
        let kib = line.split_whitespace().nth(1)?.parse::<f64>().ok()?;
        Some(kib / 1024.0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = field;
        None
    }
}

fn reset_peak() {
    #[cfg(target_os = "linux")]
    {
        let _ = std::fs::write("/proc/self/clear_refs", "5");
    }
}

fn wait_until(
    session: &mut Session,
    mut matches: impl FnMut(&Event) -> bool,
) -> Result<(Vec<Event>, Duration), String> {
    let start = Instant::now();
    let mut seen = Vec::new();
    loop {
        let batch = session.poll(POLL);
        let hit = batch.iter().any(&mut matches);
        seen.extend(batch);
        if hit {
            return Ok((seen, start.elapsed()));
        }
        if start.elapsed() >= TIMEOUT {
            return Err("timed out waiting for EPUB work".to_owned());
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn saved_spine_place() -> Result<SpinePlace, String> {
    let bytes = Arc::new(mareader_epub::sample::epub(1, "Locator"));
    let book = mareader_epub::Book::open(bytes, &Typography::default())
        .map_err(|error| error.to_string())?;
    let place = (0..book.page_count())
        .find_map(|page| {
            book.anchor(page)
                .and_then(|anchor| book.spine_place(anchor))
        })
        .ok_or("sample has no stable spine location")?;
    let chapter = format!("chapter-{TARGET_CHAPTER}.xhtml");
    let href = place.href.replacen("chapter-1.xhtml", &chapter, 1);
    if href == place.href {
        return Err("sample spine href did not identify chapter 1".to_owned());
    }
    Ok(SpinePlace {
        href,
        item: TARGET_CHAPTER.saturating_sub(1),
        block: 0,
        offset: 0,
    })
}

fn page_key(doc: DocId, page: usize) -> PageKey {
    PageKey {
        doc,
        page,
        scale: Scale::covering(0.25),
        part: Part::Whole,
    }
}

fn preview(events: &[Event], ticket: Ticket) -> Option<DocId> {
    events.iter().find_map(|event| match event {
        Event::Preview {
            ticket: opened,
            doc,
        } if *opened == ticket => Some(*doc),
        _ => None,
    })
}

fn completed(events: &[Event], ticket: Ticket) -> Option<(DocId, DocId)> {
    events.iter().find_map(|event| match event {
        Event::Completed {
            ticket: opened,
            old,
            new,
        } if *opened == ticket => Some((*old, *new)),
        _ => None,
    })
}

fn raster_arrived(events: &[Event], want: PageKey) -> bool {
    events
        .iter()
        .any(|event| matches!(event, Event::Rendered { key, result: Ok(_) } if *key == want))
}

fn run(run: &str) -> Result<(), String> {
    let spine = saved_spine_place()?;
    let bytes = Arc::new(mareader_epub::sample::epub(CHAPTERS, "EPUB performance"));
    let resident_before = resident("VmRSS:");
    reset_peak();

    let target = OpenTarget {
        page: TARGET_CHAPTER,
        spine: Some(spine),
        ..OpenTarget::default()
    };
    let mut session = Session::new(1, || {});
    let started = Instant::now();
    let ticket = session.open_at(
        Source::Bytes(bytes),
        None,
        Arc::new(Typography::default()),
        target.clone(),
    );
    let (events, preview_elapsed) = wait_until(
        &mut session,
        |event| matches!(event, Event::Preview { ticket: opened, .. } if *opened == ticket),
    )?;
    let doc = preview(&events, ticket).ok_or("preview event had no document")?;
    let preview_doc = session
        .get(doc)
        .ok_or("preview document was not retained")?;
    let preview_pages = preview_doc.doc.page_count();
    let local = preview_doc
        .doc
        .target_page(&target)
        .ok_or("preview did not contain the saved spine location")?;
    if local.saturating_add(1) >= preview_pages {
        return Err("preview had no adjacent page to request".to_owned());
    }
    let preview_rss = resident("VmRSS:");

    let first_want = page_key(doc, local);
    session.want([first_want]);
    let (events, target_raster_elapsed) = wait_until(
        &mut session,
        |event| matches!(event, Event::Rendered { key, result: Ok(_) } if *key == first_want),
    )?;
    if !raster_arrived(&events, first_want) {
        return Err("saved-place raster was not rendered".to_owned());
    }
    let target_ready = started.elapsed();

    // Let the background continuation enter a batch before asking for scroll.
    thread::sleep(Duration::from_millis(2));
    let early = session.poll(POLL);
    let early_done = completed(&early, ticket);
    let early_completed_at = early_done.map(|_| started.elapsed());
    let scroll_want = page_key(doc, local + 1);
    let scroll_started = Instant::now();
    session.want([scroll_want]);
    let (mut scroll_events, _) = wait_until(&mut session, |event| {
        matches!(event, Event::Rendered { key, result: Ok(_) } if *key == scroll_want)
            || matches!(event, Event::Completed { ticket: opened, .. } if *opened == ticket)
    })?;
    let mut done = completed(&scroll_events, ticket).or(early_done);
    let mut completed_at = completed(&scroll_events, ticket)
        .map(|_| started.elapsed())
        .or(early_completed_at);
    let mut scroll_ready_at =
        raster_arrived(&scroll_events, scroll_want).then(|| scroll_started.elapsed());
    if scroll_ready_at.is_none() {
        let (events, _) = wait_until(
            &mut session,
            |event| matches!(event, Event::Rendered { key, result: Ok(_) } if *key == scroll_want),
        )?;
        if done.is_none() {
            done = completed(&events, ticket);
            completed_at = done.map(|_| started.elapsed());
        }
        scroll_events.extend(events);
        if raster_arrived(&scroll_events, scroll_want) {
            scroll_ready_at = Some(scroll_started.elapsed());
        }
    }
    let scroll_elapsed = scroll_ready_at.ok_or("scroll page raster was not rendered")?;
    if done.is_none() {
        let (events, _) = wait_until(
            &mut session,
            |event| matches!(event, Event::Completed { ticket: opened, .. } if *opened == ticket),
        )?;
        done = completed(&events, ticket);
        completed_at = done.map(|_| started.elapsed());
    }
    let completed_at = completed_at.ok_or("completion event had no document pair")?;
    let (old, new) = done.ok_or("completion IDs were not retained")?;
    let full_pages = session
        .get(new)
        .ok_or("completed document was not retained")?
        .doc
        .page_count();
    let complete_rss = resident("VmRSS:");
    let peak_rss = resident("VmHWM:");
    session.close(old);

    let delta = |at: Option<f64>| {
        at.zip(resident_before)
            .map(|(value, before)| (value - before).max(0.0))
    };
    let peak_delta = delta(peak_rss);
    let fmt = |value: Option<f64>| value.map_or_else(|| "n/a".to_owned(), |n| format!("{n:.1}"));
    let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
    let completion_after_target = completed_at.saturating_sub(target_ready);

    println!(
        "| {run} | {:.1} | {:.1} | {:.1} | {:.1} | {:.2} | {:.2} | {preview_pages} | {full_pages} | {} | {} | {} |",
        ms(preview_elapsed),
        ms(target_raster_elapsed),
        ms(target_ready),
        ms(scroll_elapsed),
        completed_at.as_secs_f64(),
        completion_after_target.as_secs_f64(),
        fmt(delta(preview_rss)),
        fmt(delta(complete_rss)),
        fmt(peak_delta),
    );
    Ok(())
}

fn main() {
    let run_id = std::env::args().nth(1).unwrap_or_else(|| "1".to_owned());
    if let Err(error) = run(&run_id) {
        eprintln!("EPUB performance run {run_id}: {error}");
        std::process::exit(1);
    }
}
