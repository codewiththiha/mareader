use super::*;
use crate::{Budget, PageKey, Part, Scale, Source};

const ALL: Budget = Budget {
    items: 64,
    bytes: usize::MAX,
};

fn key(doc: DocId, page: usize) -> PageKey {
    PageKey {
        doc,
        page,
        scale: Scale::covering(0.5),
        part: Part::Whole,
    }
}

fn drain_until(session: &mut Session, done: impl Fn(&Event) -> bool) -> Vec<Event> {
    let mut out = Vec::new();
    for _ in 0..200 {
        let batch = session.poll(ALL);
        let hit = batch.iter().any(&done);
        out.extend(batch);
        if hit {
            break;
        }
    }
    out
}

#[test]
fn large_epub_retypesets_with_local_preview_then_completes() -> Result<(), &'static str> {
    let mut s = Session::new(0, || {});
    let bytes = Arc::new(mareader_epub::sample::epub(80, "Novel"));
    s.open(Source::Bytes(bytes), None, Arc::default());
    let events = drain_until(&mut s, |e| matches!(e, Event::Opened { .. }));
    let doc = events
        .iter()
        .find_map(|e| match e {
            Event::Opened { result: Ok(id), .. } => Some(*id),
            _ => None,
        })
        .ok_or("book did not open")?;
    let entry = s.get(doc).ok_or("missing open entry")?;
    let page = entry.doc.page_count() / 2;
    let anchor = entry.doc.anchor(page);
    let spine = anchor.and_then(|a| entry.doc.spine_place(a));
    let target = OpenTarget {
        page,
        pages: entry.doc.page_count(),
        place: anchor,
        spine,
    };
    let big = Arc::new(Typography {
        font_size: 24.0,
        ..Typography::default()
    });
    s.reset_at(doc, Arc::clone(&big), None, 7, Some(target));
    let events = drain_until(&mut s, |e| matches!(e, Event::Retypeset { .. }));
    let preview = events
        .iter()
        .find_map(|e| match e {
            Event::Retypeset { old, new, .. } if *old == doc => Some(*new),
            _ => None,
        })
        .ok_or("preview was not produced")?;
    assert!(s.get(preview).is_some_and(|e| !e.doc.is_complete()));
    // Closing the superseded doc keeps full layout alive for the preview.
    s.close(doc);
    assert_eq!(s.pending(), 0, "full layout waits for preview render");
    let local = anchor
        .and_then(|a| s.get(preview).and_then(|e| e.doc.locate(a)))
        .unwrap_or(0);
    s.want([key(preview, local)]);
    let rendered = s.poll(ALL);
    assert!(rendered.iter().any(|e| matches!(e, Event::Rendered { .. })));
    let done = drain_until(&mut s, |e| matches!(e, Event::Retypeset { .. }));
    let full = done.iter().find_map(|e| match e {
        Event::Retypeset { old, new, .. } if *old == preview => Some(*new),
        _ => None,
    });
    assert!(
        full.and_then(|id| s.get(id))
            .is_some_and(|e| e.doc.is_complete())
    );
    Ok(())
}

#[test]
fn rapid_retypeset_cancels_superseded_preview_continuation() -> Result<(), &'static str> {
    let mut s = Session::new(0, || {});
    let bytes = Arc::new(mareader_epub::sample::epub(80, "Drag"));
    s.open(Source::Bytes(bytes), None, Arc::default());
    let events = drain_until(&mut s, |e| matches!(e, Event::Opened { .. }));
    let doc = events
        .iter()
        .find_map(|e| match e {
            Event::Opened { result: Ok(id), .. } => Some(*id),
            _ => None,
        })
        .ok_or("book did not open")?;
    let target = OpenTarget::default();
    let t1 = Arc::new(Typography {
        font_size: 20.0,
        ..Typography::default()
    });
    let t2 = Arc::new(Typography {
        font_size: 26.0,
        ..Typography::default()
    });
    s.reset_at(doc, t1, None, 1, Some(target.clone()));
    let first = drain_until(&mut s, |e| matches!(e, Event::Retypeset { .. }));
    let p1 = first
        .iter()
        .find_map(|e| match e {
            Event::Retypeset { new, .. } => Some(*new),
            _ => None,
        })
        .ok_or("first preview missing")?;
    s.close(doc);
    s.want([key(p1, 0)]);
    let _ = s.poll(ALL);
    // Dragging again cancels p1's full layout and previews t2 immediately.
    s.reset_at(p1, Arc::clone(&t2), None, 1, Some(target));
    let second = drain_until(&mut s, |e| matches!(e, Event::Retypeset { .. }));
    let p2 = second.iter().find_map(|e| match e {
        Event::Retypeset { old, new, .. } if *old == p1 => Some(*new),
        _ => None,
    });
    assert!(p2.is_some_and(|id| {
        s.get(id)
            .is_some_and(|e| e.typo == t2 && !e.doc.is_complete())
    }));
    Ok(())
}

fn wait_events(session: &mut Session, done: impl Fn(&Event) -> bool) -> Vec<Event> {
    let start = std::time::Instant::now();
    let mut out = Vec::new();
    while start.elapsed() < std::time::Duration::from_secs(10) {
        let batch = session.poll(ALL);
        let hit = batch.iter().any(&done);
        out.extend(batch);
        if hit {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    out
}

fn wait_retypeset(session: &mut Session, from: DocId) -> Option<DocId> {
    let events = wait_events(
        session,
        |e| matches!(e, Event::Retypeset { old, .. } if *old == from),
    );
    events.iter().find_map(|e| match e {
        Event::Retypeset { old, new, .. } if *old == from => Some(*new),
        _ => None,
    })
}

#[test]
fn threaded_retypeset_debounces_background_and_completes_latest() -> Result<(), &'static str> {
    let mut s = Session::new(2, || {});
    let bytes = Arc::new(mareader_epub::sample::epub(80, "Bg"));
    s.open(Source::Bytes(bytes), None, Arc::default());
    let opened = wait_events(&mut s, |e| matches!(e, Event::Opened { .. }));
    let doc = opened
        .iter()
        .find_map(|e| match e {
            Event::Opened { result: Ok(id), .. } => Some(*id),
            _ => None,
        })
        .ok_or("book did not open")?;
    let target = OpenTarget::default();
    let t1 = Arc::new(Typography {
        font_size: 20.0,
        ..Typography::default()
    });
    let t2 = Arc::new(Typography {
        font_size: 26.0,
        ..Typography::default()
    });
    s.reset_at(doc, t1, None, 1, Some(target.clone()));
    let p1 = wait_retypeset(&mut s, doc).ok_or("first preview missing")?;
    s.close(doc);
    s.want([key(p1, 0)]);
    let _ = wait_events(&mut s, |e| matches!(e, Event::Rendered { .. }));
    s.reset_at(p1, Arc::clone(&t2), None, 1, Some(target));
    let p2 = wait_retypeset(&mut s, p1).ok_or("second preview missing")?;
    s.close(p1);
    s.want([key(p2, 0)]);
    let _ = wait_events(&mut s, |e| matches!(e, Event::Rendered { .. }));
    let full = wait_retypeset(&mut s, p2).ok_or("full book missing")?;
    let entry = s.get(full).ok_or("missing completed entry")?;
    assert!(entry.typo == t2 && entry.doc.is_complete());
    Ok(())
}

#[test]
fn progressive_preview_edit_primes_visible_page_and_completes_with_latest_type()
-> Result<(), &'static str> {
    let mut s = Session::new(2, || {});
    let bytes = Arc::new(mareader_epub::sample::epub(80, "ProgEdit"));
    let base = Arc::new(Typography::default());
    s.open_at(
        Source::Bytes(bytes),
        None,
        Arc::clone(&base),
        OpenTarget::default(),
    );
    let preview_events = wait_events(&mut s, |e| matches!(e, Event::Preview { .. }));
    let preview = preview_events
        .iter()
        .find_map(|e| match e {
            Event::Preview { doc, .. } => Some(*doc),
            _ => None,
        })
        .ok_or("preview did not arrive")?;
    s.want([key(preview, 0)]);
    let _ = wait_events(&mut s, |e| matches!(e, Event::Rendered { .. }));

    let bigger = Arc::new(Typography {
        font_size: 24.0,
        ..Typography::default()
    });
    let prime = Prime {
        scale: Scale::covering(0.5),
        pages: 1,
        spread: false,
    };
    s.reset_primed(
        preview,
        Arc::clone(&bigger),
        None,
        3,
        (Some(OpenTarget::default()), Some(prime)),
    );
    let batch = wait_events(&mut s, |e| matches!(e, Event::Retypeset { .. }));
    let retyped = batch
        .iter()
        .find_map(|e| match e {
            Event::Retypeset { old, new, .. } if *old == preview => Some(*new),
            _ => None,
        })
        .ok_or("preview retypeset did not arrive")?;
    assert!(
        batch.iter().any(|e| matches!(
            e,
            Event::Rendered { key, result: Ok(_) } if key.doc == retyped && key.page == 0
        )),
        "primed raster must arrive in the same poll batch as Event::Retypeset"
    );
    assert!(
        batch.iter().any(|e| matches!(
            e,
            Event::Text { doc, page: 0, result: Ok(_) } if *doc == retyped
        )),
        "primed text layer must arrive in the same poll batch as Event::Retypeset"
    );

    let done = wait_events(&mut s, |e| matches!(e, Event::Completed { .. }));
    let completed = done
        .iter()
        .find_map(|e| match e {
            Event::Completed { new, .. } => Some(*new),
            _ => None,
        })
        .ok_or("progressive open did not complete after preview edit")?;
    let entry = s.get(completed).ok_or("missing completed entry")?;
    assert!(entry.doc.is_complete());
    assert_eq!(entry.typo, bigger);
    Ok(())
}
