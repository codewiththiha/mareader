use super::*;

fn pairs(list: &[(&str, &str)]) -> impl Iterator<Item = (String, String)> {
    let owned: Vec<(String, String)> = (list.iter())
        .map(|(id, tag)| ((*id).to_owned(), (*tag).to_owned()))
        .collect();
    owned.into_iter()
}

#[test]
fn the_usual_voice_is_the_compact_one_in_the_users_language() {
    let voices = [
        ("com.apple.speech.synthesis.voice.Albert", "en-US"),
        ("com.apple.voice.compact.en-GB.Daniel", "en-GB"),
        ("com.apple.eloquence.en-US.Eddy", "en-US"),
        ("com.apple.voice.compact.en-US.Samantha", "en-US"),
        ("com.apple.voice.compact.fr-FR.Thomas", "fr-FR"),
    ];
    assert_eq!(usual(pairs(&voices), "en_US"), Some(3));
    assert_eq!(usual(pairs(&voices), "en-GB"), Some(1));
    // A region with no voice of its own takes its language's.
    assert_eq!(usual(pairs(&voices), "fr-CA"), Some(4));
    assert_eq!(usual(pairs(&voices), "de-DE"), None);
    assert_eq!(usual(pairs(&[]), "en-US"), None);
}

fn sounds(pairs: &[(&str, usize)]) -> Vec<(String, usize)> {
    pairs.iter().map(|(id, n)| ((*id).to_owned(), *n)).collect()
}

/// As the owner's Mac reported: bare until speech starts, then Eloquence.
#[test]
fn voices_without_sound_are_mute_once_others_report_theirs() {
    let none = HashSet::new();
    let eddy = "com.apple.eloquence.en-GB.Eddy";
    // Before anything spoke, every voice is bare: none is judged.
    let cold: Vec<_> = (0..40).map(|i| (format!("v{i}"), 0)).collect();
    assert_eq!(mute(&cold, &none), (HashSet::new(), false));
    // Warm: many unspoken voices report theirs, so the bare are mute.
    let mut warm: Vec<_> = (0..20).map(|i| (format!("v{i}"), 7)).collect();
    warm.push((eddy.to_owned(), 0));
    let (found, settled) = mute(&warm, &none);
    assert_eq!(
        (found.len(), found.contains(eddy), settled),
        (1, true, true)
    );
    // Cold but alive: only a voice heard to the end and still bare.
    let some = sounds(&[("samantha", 7), (eddy, 0), ("flo", 0)]);
    let heard = HashSet::from([eddy.to_owned()]);
    assert_eq!(mute(&some, &none).0, HashSet::new());
    assert_eq!(mute(&some, &heard), (heard.clone(), false));
    // Never every voice, even heard.
    let all = HashSet::from([eddy.to_owned(), "samantha".to_owned()]);
    let bare = sounds(&[("samantha", 0), (eddy, 0)]);
    assert_eq!(mute(&bare, &all), (HashSet::new(), false));
}

/// Only where the system's speech runs in CI: the remote `system` task.
#[cfg(any(target_vendor = "apple", windows))]
#[test]
fn system_voices_open_again_and_side_by_side() {
    let wake: Waker = Arc::new(|| {});
    let first = match System::new(Arc::clone(&wake)) {
        Ok(first) => first,
        Err(e) => panic!("{e}"),
    };
    let listed = first.voices().len();
    let mute = first.speech.voices.len() - listed;
    println!("system voices: {listed} listed, {mute} mute");
    assert!(listed > 0);
    drop(first);
    // Read aloud stopped and started, and the settings tab reopened.
    for _ in 0..3 {
        match System::new(Arc::clone(&wake)) {
            Ok(again) => assert_eq!(again.voices().len(), listed),
            Err(e) => panic!("opening again: {e}"),
        }
    }
    // The settings tab open while a book is read.
    let mut tab = System::new(Arc::clone(&wake)).ok().unwrap();
    let mut book = System::new(wake).ok().unwrap();
    let voices = tab.voices();
    tab.set_voice(voices.first().map(|v| v.id.as_str()));
    book.set_voice(None);
    assert!(book.say(1, "A sentence read from the book.").is_ok());
    // Closing the tab leaves the book's sentence alone.
    tab.stop();
    assert!(book.speaking() && !tab.speaking());
    assert!(tab.say(2, "A sample.").is_ok());
    // Shared, a sample cuts the book's sentence off; apart, both speak.
    assert!(tab.speaking() && book.speaking() != ONCE);
    // The book never takes the tab's sample for its own sentence.
    let heard = book.events();
    let theirs = [Event::Started(2), Event::Finished(2)];
    assert!(heard.iter().all(|e| !theirs.contains(e)), "{heard:?}");
    assert_eq!(book.current.is_none(), ONCE);
    tab.stop();
    book.stop();
    println!("system voices: open again and side by side");
}
