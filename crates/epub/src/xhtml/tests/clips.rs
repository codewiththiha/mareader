use super::*;

#[test]
fn video_is_a_card_that_plays_its_first_source() {
    let (b, refs) = read_all(
        &mut Shelf::default(),
        "<p>Before.</p><video controls='controls' title='Ocean'>\
             <source src='../media/clip.webm' type='video/webm'/>\
             <source src='../media/clip.mp4' type='video/mp4'/>\
             <p>zqold readers only</p></video><p>After.</p>",
    );
    let all = texts(&b);
    assert_eq!(all, ["Before.", "Play the video: Ocean", "After."]);
    let clip = Href::Media {
        path: "/OEBPS/media/clip.webm".into(),
        video: true,
    };
    assert_eq!(picture_links(&b, &refs), [[(&clip, [0.0, 0.0, 1.0, 1.0])]]);
    let caption = b
        .iter()
        .filter_map(text)
        .find(|t| t.text.starts_with("Play"));
    assert_eq!(caption.map(|t| t.links.len()), Some(1));
    let card = pictures_of(&b);
    assert!(
        card.first().is_some_and(|p| p.vector() && p.ink()),
        "{card:?}"
    );
}

#[test]
fn cards_play_the_first_clip_that_plays_or_say_why_not() {
    let (b, refs) = read_all(
        &mut Shelf::default(),
        "<video><source src='../media/tool.bin'/><source src='../media/b.mp4'/></video>\
             <video src='../media/tool.bin' title='Tool'/><audio src='../media/gone.wav'/>",
    );
    let words = [
        "Play the video: b.mp4",
        "The video can't play here: Tool",
        "The audio isn't in this book",
    ];
    assert_eq!(texts(&b), words);
    let played = |h: &Href| matches!(h, Href::Media { .. });
    assert_eq!(refs.hrefs.iter().filter(|h| played(h)).count(), 1);
}

#[test]
fn audio_cards_name_their_file_and_web_clips_open_outside() {
    let (b, refs) = read_all(
        &mut Shelf::default(),
        "<audio src='../media/song%20one.mp3'>zqfallback</audio>\
             <audio src='https://example.org/a.ogg'/><video><p>zqnone</p></video>",
    );
    let all = texts(&b);
    assert_eq!(
        all,
        [
            "Play the audio: song one.mp3",
            "Play the audio: a.ogg",
            "The video isn't in this book"
        ]
    );
    let song = Href::Media {
        path: "/OEBPS/media/song one.mp3".into(),
        video: false,
    };
    assert!(refs.hrefs.contains(&song), "{:?}", refs.hrefs);
    let web = Href::Outside("https://example.org/a.ogg".into());
    assert!(refs.hrefs.contains(&web), "{:?}", refs.hrefs);
    // Drawn cards are line art, so they take the theme's ink.
    let cards = pictures_of(&b);
    assert!(
        cards.len() == 3 && cards.iter().all(|p| p.ink()),
        "{cards:?}"
    );
}

#[test]
fn a_poster_shows_under_a_play_mark() {
    let mut shelf = Shelf::default();
    if let Some(p) = png(64, 36) {
        shelf.pictures.insert("/OEBPS/images/still.png".into(), p);
    }
    let b = read_with(
        &mut shelf,
        "<video src='../media/v.mp4' poster='../images/still.png'/>",
    );
    let pictures = pictures_of(&b);
    assert_eq!(pictures.len(), 1, "{pictures:?}");
    let Some(p) = pictures.first() else { return };
    assert!(p.vector() && (p.width, p.height) == (64, 36), "{p:?}");
    assert!(!p.ink(), "a photo keeps its colours");
    let drawn = p.thumbnail(64);
    // Black poster, white play mark: the middle is light, a corner dark.
    let shade = |x: u32, y: u32| {
        let (w, _, px) = drawn.as_ref()?;
        let i = usize::try_from((y * w + x) * 4).ok()?;
        px.get(i).copied()
    };
    assert!(
        shade(32, 18).is_some_and(|v| v > 150),
        "{:?}",
        shade(32, 18)
    );
    assert!(shade(1, 1).is_some_and(|v| v < 40), "{:?}", shade(1, 1));
}

#[test]
fn a_switch_shows_its_first_drawn_case_or_its_default() {
    let b = read(
        "<epub:switch id='s1'>\
             <epub:case required-namespace='http://www.xml-cml.org/schema'><p>zqchem</p></epub:case>\
             <epub:case required-namespace='http://www.w3.org/1998/Math/MathML'><p>Drawn</p></epub:case>\
             <epub:case required-namespace='http://www.w3.org/2000/svg'><p>zqsecond</p></epub:case>\
             <epub:default><p>zqdefault</p></epub:default></epub:switch>\
             <epub:switch><epub:case required-namespace='urn:x'><p>zqx</p></epub:case>\
             <epub:default><p>Default</p></epub:default></epub:switch>",
    );
    assert_eq!(texts(&b), ["Drawn", "Default"]);
}

#[test]
fn noscript_shows_since_scripts_never_run() {
    let b = read(
        "<script>var zq = 1;</script><noscript><p>Turn on <b>scripts</b>.</p></noscript>\
             <canvas width='9'><p>Canvas words</p></canvas>",
    );
    assert_eq!(texts(&b), ["Turn on scripts.", "Canvas words"]);
}

#[test]
fn objects_show_their_picture_or_else_their_fallback() {
    let mut shelf = Shelf::default();
    if let Some(p) = png(8, 8) {
        shelf.pictures.insert("/OEBPS/images/a.png".into(), p);
    }
    let b = read_with(
        &mut shelf,
        "<object data='../images/a.png' type='image/png'><p>zqhidden</p></object>\
             <object data='widget.xhtml' type='application/xhtml+xml'><p>Fallback words</p></object>\
             <object data='../media/t.mp3' type='audio/mpeg'><p>zqgone</p></object>\
             <object data='gone.png'><p>Missing words</p></object>\
             <embed src='../images/a.png'/>",
    );
    let all = texts(&b);
    assert_eq!(
        all,
        ["Fallback words", "Play the audio: t.mp3", "Missing words"]
    );
    let sizes: Vec<_> = pictures_of(&b).iter().map(|p| p.width).collect();
    assert_eq!(sizes, [8, 320, 8]);
}
