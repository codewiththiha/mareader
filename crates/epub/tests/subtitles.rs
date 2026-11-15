//! Subtitle tracks: transcripts laid in the pages, and files for players.
use std::sync::Arc;

use mareader_epub::{Book, Destination, Inserts, Wanted};
use mareader_reflow::Typography;

use crate::common::text;

const BOOK: &[u8] = include_bytes!("books/subtitles.epub");

fn open() -> Book {
    Book::open(Arc::new(BOOK.to_vec()), &Typography::default()).unwrap()
}

fn name(tag: &str) -> Option<&'static str> {
    match tag {
        "en" => Some("English"),
        "fr" => Some("French"),
        "my" => Some("Burmese"),
        _ => None,
    }
}

fn wanted(tags: &[String]) -> Wanted<'_> {
    Wanted {
        languages: tags,
        name,
    }
}

fn tags(list: &[&str]) -> Vec<String> {
    list.iter().map(|t| t.to_string()).collect()
}

fn transcripts(book: &Book) -> Vec<(usize, Option<usize>)> {
    let pages = 0..book.page_count();
    let links = pages.flat_map(|p| book.links(p));
    let mut out: Vec<_> = links
        .filter_map(|l| match l.to {
            Destination::Transcript { card, track } => Some((card, track)),
            _ => None,
        })
        .collect();
    out.dedup();
    out
}

/// `book` with the transcripts of `open` laid in.
fn with(book: &Book, open: &[(usize, Option<usize>)], want: &[String]) -> Book {
    let list = (open.iter())
        .filter_map(|&(card, track)| book.transcript(card, track, wanted(want)))
        .map(Arc::new)
        .collect();
    let inserts = Arc::new(Inserts { key: 1, list });
    book.reset(&Typography::default(), Some(&inserts)).unwrap()
}

#[test]
fn cards_with_tracks_link_their_transcript_but_cells_dont() {
    let book = open();
    let all = text(&book);
    assert_eq!(all.matches("Show transcript").count(), 3, "{all}");
    assert_eq!(transcripts(&book), [(0, None), (1, None), (3, None)]);
    // A clip that isn't in the book still has its words.
    assert!(all.contains("The video isn't in this book"));
    assert!(book.transcript(2, None, wanted(&[])).is_none());
}

#[test]
fn a_transcript_replaces_its_link_in_the_wanted_language() {
    let book = open();
    let shown = with(&book, &[(0, None)], &tags(&["my", "en"]));
    let all = text(&shown);
    assert!(all.contains("Hide transcript"), "{all}");
    assert!(all.contains("လှိုင်းများ ဝင်လာသည်။"), "{all}");
    assert!(all.contains("0:01") && all.contains("0:04"), "{all}");
    assert!(!all.contains("Waves roll in."), "{all}");
    assert_eq!(all.matches("Show transcript").count(), 2);
    // The text around it stays, in order.
    let before = all.find("Before the clip.").unwrap();
    let hide = all.find("Hide transcript").unwrap();
    assert!(before < hide && hide < all.find("After the clip.").unwrap());
    // The other tracks are links that pick them; the shown one isn't.
    let links = transcripts(&shown);
    assert!(
        links.contains(&(0, None)) && links.contains(&(0, Some(0))),
        "{links:?}"
    );
    assert!(links.contains(&(0, Some(1))) && !links.contains(&(0, Some(2))));
    assert!(all.contains("English") && all.contains("French") && all.contains("Burmese"));
}

#[test]
fn the_default_track_shows_when_no_language_fits_and_a_pick_wins() {
    let book = open();
    let all = text(&with(&book, &[(0, None)], &tags(&["de"])));
    assert!(all.contains("Guide: Waves roll in."), "{all}");
    assert!(all.contains("The tide turns & rises."), "{all}");
    let all = text(&with(&book, &[(0, Some(1))], &tags(&["my"])));
    assert!(all.contains("Les vagues arrivent."), "{all}");
    let all = text(&with(&book, &[(0, None)], &tags(&["fr-CA"])));
    assert!(all.contains("La marée monte."), "{all}");
}

#[test]
fn audio_and_missing_clips_show_their_words_too() {
    let book = open();
    let all = text(&with(&book, &[(1, None), (3, None)], &tags(&[])));
    assert!(all.contains("Sing a song of the sea"), "{all}");
    assert!(
        all.contains("1:05") && all.contains("Second verse"),
        "{all}"
    );
    assert!(all.contains("Only words remain."), "{all}");
    // Only the first card's link is left.
    assert_eq!(all.matches("Show transcript").count(), 1, "{all}");
}

#[test]
fn players_get_each_subtitle_file_of_a_clip() {
    let book = open();
    let subs = book.subtitles("/OEBPS/media/sea.mp4");
    let got: Vec<_> = (subs.iter())
        .map(|s| (s.language.as_deref(), s.extension))
        .collect();
    assert_eq!(
        got,
        [
            (Some("en"), "vtt"),
            (Some("fr"), "srt"),
            (Some("my"), "vtt")
        ]
    );
    assert!(subs[0].data.starts_with(b"WEBVTT"));
    let song = book.subtitles("/OEBPS/media/song.mp3");
    assert_eq!(song.first().map(|s| s.extension), Some("ttml"));
    assert!(book.subtitles("/OEBPS/media/gone.mp4").is_empty());
}
