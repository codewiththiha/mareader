//! A generated EPUB for tests, screenshots and the web demo.
use std::io::{Cursor, Write};

use rbook::Epub;
use rbook::epub::EpubChapter;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const PROSE: [&str; 6] = [
    "This book was written by mareader itself, so the demo needs no file. \
     Its pages are laid out from flowing text, not drawn ahead of time.",
    "The river ran slow and brown past the old customs house, where the \
     boats unloaded rice and timber at dawn. Nobody remembered who had \
     built the jetty, and nobody asked.",
    "Change the window, the zoom or the theme: the words stay where they \
     are and only the paper and ink around them move.",
    "A long paragraph is cut between lines, never through one, and a \
     heading always keeps company with the text that follows it.",
    "The quick brown fox jumps over the lazy dog. Sphinx of black quartz, \
     judge my vow. Fine ligatures: fi, fl, ffi.",
    "Each chapter opens on a fresh page, as it would in a printed book.",
];

/// Everything the reader draws besides prose, as books write it.
const FEATURES: &str = r##"
<h2>Things a book holds</h2>
<p>Links read as underlined words: on <a href="chapter-2.xhtml">to the next
chapter</a>, down <a href="#part-2">to Part 1.2</a>, or out
<a href="https://github.com/codewiththiha/mareader2">to the web</a>. Notes
stay beside their mark<a epub:type="noteref" href="#note-1">1</a>.</p>
<p>Lists hang their markers in the margin:</p>
<ul>
  <li>Rice and timber at dawn</li>
  <li>Boats <em>unloading</em> by the jetty
    <ul><li>a nested item</li><li>and another</li></ul>
  </li>
  <li>The old customs house</li>
</ul>
<ol><li>Open the book</li><li>Turn the page</li><li>Read on</li></ol>
<blockquote><p>Nobody remembered who had built the jetty, and nobody
asked.</p></blockquote>
<p>Words keep the book's <span style="color: #c01c28">red</span> and
<span style="color: #1c71d8">blue</span>, in any theme. Inline <code>code</code>
sits in the line; code blocks keep spaces:</p>
<pre><code>fn main() {
    let boats = 12;
    println!("{boats} boats at dawn");
}</code></pre>
<table>
  <thead><tr><th>Cargo</th><th>Boats</th><th>Arrives</th></tr></thead>
  <tbody>
    <tr><td>Rice</td><td>12</td><td>Dawn</td></tr>
    <tr><td>Timber</td><td>5</td><td>Noon, when the tide allows</td></tr>
    <tr><td>Salt</td><td>2</td><td>Dusk</td></tr>
  </tbody>
</table>
<hr/>
<div class="figure"><img src="../images/river.png" alt="The river at dusk"/></div>
<p class="caption">The river at dusk.</p>
<aside id="note-1" epub:type="footnote"><p>A note opens in a card on its mark.</p></aside>
"##;

/// A dusk landscape `w × h`: sky, sun, and a rippled river.
fn landscape(w: u32, h: u32) -> image::RgbImage {
    let horizon = h * 62 / 100;
    let (sun_x, sun_y, sun_r) = (
        w as f32 / 2.0,
        horizon as f32 - h as f32 * 0.04,
        w as f32 * 0.15,
    );
    image::RgbImage::from_fn(w, h, |x, y| {
        let mix = |a: [f32; 3], b: [f32; 3], t: f32| {
            image::Rgb(std::array::from_fn(|i| {
                let (a, b) = (a.get(i).copied(), b.get(i).copied());
                let (a, b) = (a.unwrap_or(0.0), b.unwrap_or(0.0));
                (a + (b - a) * t.clamp(0.0, 1.0)) as u8
            }))
        };
        let (fx, fy) = (x as f32, y as f32);
        if y < horizon {
            let d = ((fx - sun_x).powi(2) + (fy - sun_y).powi(2)).sqrt();
            if d < sun_r {
                return image::Rgb([255, 214, 128]);
            }
            mix(
                [35.0, 40.0, 110.0],
                [240.0, 140.0, 70.0],
                fy / horizon as f32,
            )
        } else {
            let t = (fy - horizon as f32) / (h - horizon).max(1) as f32;
            let ripple = ((fy * 0.35).sin() * (fx * 0.02).cos()).abs() > 0.92;
            if ripple && (fx - sun_x).abs() < sun_r * (1.0 - t) {
                image::Rgb([250, 190, 110])
            } else {
                mix([30.0, 80.0, 100.0], [15.0, 35.0, 55.0], t)
            }
        }
    })
}

fn encode(img: &image::RgbImage) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// The sample's cover, a portrait PNG.
pub fn cover_png() -> Option<Vec<u8>> {
    encode(&landscape(600, 900))
}

fn chapter_xhtml(n: usize) -> String {
    let mut body = format!("<h1>Chapter {n}</h1>");
    for i in 0..12 {
        let text = PROSE
            .get((n + i) % PROSE.len())
            .copied()
            .unwrap_or_default();
        if i == 6 {
            body.push_str(&format!("<h2 id=\"part-2\">Part {n}.2</h2>"));
        }
        if n == 1 && i == 3 {
            body.push_str(FEATURES);
        }
        body.push_str(&format!(
            "<p><em>{n}.{i}</em> {text} <strong>Bold</strong> end.</p>"
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head>\
         <title>Chapter {n}</title><style>\
         .figure, .caption {{ text-align: center }} \
         .caption {{ font-style: italic }}</style></head>\
         <body>{body}</body></html>"
    )
}

/// A cover, then chapters listed with their parts; the first shows all.
pub fn epub(chapters: usize, title: &str) -> Vec<u8> {
    let chapters: Vec<EpubChapter> = (1..=chapters.max(1))
        .map(|n| {
            let href = format!("text/chapter-{n}.xhtml");
            let part = EpubChapter::new(format!("Part {n}.2")).href(format!("{href}#part-2"));
            EpubChapter::new(format!("Chapter {n}"))
                .href(href)
                .xhtml(chapter_xhtml(n))
                .children(part)
        })
        .collect();
    let mut book = Epub::builder()
        .title(title)
        .author("mareader")
        .chapter(chapters);
    if let Some(cover) = cover_png() {
        book = book.cover_image(("images/cover.png", cover));
    }
    if let Some(river) = encode(&landscape(480, 270)) {
        book = book.resource(("images/river.png", river));
    }
    book.write().to_vec().unwrap_or_default()
}

/// A one-page pre-paginated EPUB filled with the dusk landscape artwork.
pub fn fixed_epub(title: &str) -> Vec<u8> {
    let opf = format!(
        "<?xml version='1.0' encoding='UTF-8'?>\
         <package xmlns='http://www.idpf.org/2007/opf' version='3.0' unique-identifier='id' \
         prefix='rendition: http://www.idpf.org/vocab/rendition/#'>\
         <metadata xmlns:dc='http://purl.org/dc/elements/1.1/'>\
         <dc:identifier id='id'>urn:mareader:fixed</dc:identifier>\
         <dc:title>{title}</dc:title><dc:language>en</dc:language>\
         <meta property='dcterms:modified'>2026-01-01T00:00:00Z</meta>\
         <meta property='rendition:layout'>pre-paginated</meta></metadata>\
         <manifest>\
         <item id='p0' href='page.xhtml' media-type='application/xhtml+xml'/>\
         <item id='art' href='cover.png' media-type='image/png'/>\
         </manifest><spine><itemref idref='p0'/></spine></package>"
    );
    let xhtml = "<?xml version='1.0' encoding='UTF-8'?>\
        <html xmlns='http://www.w3.org/1999/xhtml'><head>\
        <meta name='viewport' content='width=600, height=900'/>\
        <style>html, body { margin: 0; padding: 0; width: 600px; height: 900px; }\
        img { display: block; width: 600px; height: 900px; }</style></head>\
        <body><img src='cover.png' alt='Landscape'/></body></html>";
    let container = "<?xml version='1.0' encoding='UTF-8'?>\
        <container version='1.0' xmlns='urn:oasis:names:tc:opendocument:xmlns:container'>\
        <rootfiles><rootfile full-path='OPS/book.opf' \
        media-type='application/oebps-package+xml'/></rootfiles></container>";
    let art = cover_png().unwrap_or_default();
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let _ = zip.start_file("mimetype", stored);
    let _ = zip.write_all(b"application/epub+zip");
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let files: [(&str, &[u8]); 4] = [
        ("META-INF/container.xml", container.as_bytes()),
        ("OPS/book.opf", opf.as_bytes()),
        ("OPS/page.xhtml", xhtml.as_bytes()),
        ("OPS/cover.png", &art),
    ];
    for (name, data) in files {
        let _ = zip.start_file(name, deflated);
        let _ = zip.write_all(data);
    }
    zip.finish().map(Cursor::into_inner).unwrap_or_default()
}

/// What READMEs hold: emoji, tasks, code, tables, quotes and alerts.
const README: &str = r#"## Section {n}: setup {e1}

The river ran slow past the customs house {e2}, where boats unloaded rice
and timber at dawn. Nobody remembered who built the jetty {e3}, and nobody
asked. **Bold**, *italic*, `inline code` and [a link](https://example.com).

- [x] Clone the repository {e1}
- [x] Install the toolchain
- [ ] Write the first chapter {e4}
- [ ] Ship it {e2}

```rust
fn main() {
    let boats = 12; // {e3} at dawn
    for n in 0..boats {
        println!("boat {n} unloads rice and timber");
    }
}
```

| Cargo | Boats | Status |
|---|---|---|
| Rice | 12 | {e1} landed |
| Timber | 5 | {e4} waiting |
| Salt | 2 | {e2} at sea |

> [!WARNING]
> {e4} The tide turns at noon; boats still out wait for dusk.

> A quote of an old boatman {e3}: the river keeps no promises.

1. Open the book
2. Turn the page {e1}
3. Read on
"#;

/// `sections` README sections, about a page each; `emoji` or plain marks.
pub fn markdown(sections: usize, emoji: bool) -> String {
    let marks = if emoji {
        ["✅", "🚀", "⚠️", "📦"]
    } else {
        ["(ok)", "(go)", "(!)", "(box)"]
    };
    let mut out = String::from("# A README of emoji\n\n");
    for n in 1..=sections.max(1) {
        let section = README
            .replace("{n}", &n.to_string())
            .replace("{e1}", marks[0])
            .replace("{e2}", marks[1])
            .replace("{e3}", marks[2])
            .replace("{e4}", marks[3]);
        out.push_str(&section);
        out.push('\n');
    }
    out
}
