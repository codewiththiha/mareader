//! EPUB 3 fidelity: non-ASCII paths, covers, floated lists, backdrop art.
use std::io::{Cursor, Write};
use std::sync::Arc;

use mareader_epub::Book;
use mareader_reflow::Typography;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Valid 4x4 red PNG (`#e03030`) so `Picture::ink()` classifies it as colour.
const RED_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 4, 8, 2, 0,
    0, 0, 38, 147, 9, 41, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 120, 96, 96, 0, 71, 12, 196,
    113, 0, 19, 82, 20, 1, 224, 212, 196, 30, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn pack(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let _ = zip.start_file("mimetype", stored);
    let _ = zip.write_all(b"application/epub+zip");
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in files {
        let _ = zip.start_file(*name, deflated);
        let _ = zip.write_all(data);
    }
    zip.finish().map(Cursor::into_inner).unwrap_or_default()
}

fn container_xml(opf: &str) -> Vec<u8> {
    format!(
        "<?xml version='1.0' encoding='UTF-8'?>\
         <container version='1.0' xmlns='urn:oasis:names:tc:opendocument:xmlns:container'>\
         <rootfiles><rootfile full-path='{opf}' \
         media-type='application/oebps-package+xml'/></rootfiles></container>"
    )
    .into_bytes()
}

#[test]
fn non_ascii_percent_encoded_spine_and_resources_open() {
    let opf = "<?xml version='1.0' encoding='UTF-8'?>\
        <package xmlns='http://www.idpf.org/2007/opf' version='3.0' unique-identifier='id'>\
        <metadata xmlns:dc='http://purl.org/dc/elements/1.1/'>\
        <dc:identifier id='id'>urn:test:jp</dc:identifier>\
        <dc:title>草枕</dc:title><dc:language>ja</dc:language>\
        <meta property='dcterms:modified'>2026-01-01T00:00:00Z</meta></metadata>\
        <manifest>\
        <item id='p0' href='xhtml/%E8%A1%A8%E7%B4%99.xhtml' \
            media-type='application/xhtml+xml'/>\
        <item id='img' href='image/%E8%A1%A8%E7%B4%99.png' media-type='image/png'/>\
        </manifest><spine><itemref idref='p0'/></spine></package>";
    let xhtml = "<?xml version='1.0' encoding='UTF-8'?>\
        <html xmlns='http://www.w3.org/1999/xhtml'><body>\
        <p>山路を登りながら</p>\
        <img src='../image/%E8%A1%A8%E7%B4%99.png' alt='cover'/>\
        </body></html>";
    let container = container_xml("OPS/package.opf");
    let bytes = pack(&[
        ("META-INF/container.xml", &container),
        ("OPS/package.opf", opf.as_bytes()),
        ("OPS/xhtml/表紙.xhtml", xhtml.as_bytes()),
        ("OPS/image/表紙.png", RED_PNG),
    ]);
    let book = Book::open(Arc::new(bytes), &Typography::default()).unwrap();
    assert!(book.page_count() >= 1);
    assert!(crate::common::text(&book).contains("山路"));
    assert!(!book.pictures(0).is_empty());
}

#[test]
fn cover_image_reused_as_floated_portrait_keeps_heading_with_prose() {
    let opf = "<?xml version='1.0' encoding='UTF-8'?>\
        <package xmlns='http://www.idpf.org/2007/opf' version='3.0' unique-identifier='id'>\
        <metadata xmlns:dc='http://purl.org/dc/elements/1.1/'>\
        <dc:identifier id='id'>urn:test:cole</dc:identifier>\
        <dc:title>Voyage</dc:title><dc:language>en</dc:language>\
        <meta property='dcterms:modified'>2026-01-01T00:00:00Z</meta></metadata>\
        <manifest>\
        <item id='c0' href='intro.xhtml' media-type='application/xhtml+xml'/>\
        <item id='port' href='portrait.png' media-type='image/png' properties='cover-image'/>\
        </manifest><spine><itemref idref='c0'/></spine></package>";
    let xhtml = "<?xml version='1.0' encoding='UTF-8'?>\
        <html xmlns='http://www.w3.org/1999/xhtml'><head><style>\
        img#portrait { float: right; }\
        </style></head><body>\
        <h1>The Voyage of Life</h1>\
        <h2>Background</h2>\
        <img id='portrait' src='portrait.png' alt='portrait'/>\
        <p>Thomas Cole founded the Hudson River School.</p>\
        </body></html>";
    let container = container_xml("EPUB/book.opf");
    let bytes = pack(&[
        ("META-INF/container.xml", &container),
        ("EPUB/book.opf", opf.as_bytes()),
        ("EPUB/intro.xhtml", xhtml.as_bytes()),
        ("EPUB/portrait.png", RED_PNG),
    ]);
    let book = Book::open(Arc::new(bytes), &Typography::default()).unwrap();
    // Cover is prepended on page 0; heading and prose stay together on page 1.
    assert!(book.page_count() >= 2);
    let page1: String = book
        .text(1)
        .into_iter()
        .map(|l| l.chars.into_iter().map(|c| c.ch).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        page1.contains("Background"),
        "page 1 missing heading: {page1}"
    );
    assert!(
        page1.contains("Thomas Cole"),
        "heading orphaned from prose: {page1}"
    );
}

#[test]
fn floated_box_preserves_inner_list_offsets_and_keeps_backdrop_thumbnails() {
    let opf = "<?xml version='1.0' encoding='UTF-8'?>\
        <package xmlns='http://www.idpf.org/2007/opf' version='3.0' unique-identifier='id'>\
        <metadata xmlns:dc='http://purl.org/dc/elements/1.1/'>\
        <dc:identifier id='id'>urn:test:fp</dc:identifier>\
        <dc:title>Paintings</dc:title><dc:language>en</dc:language>\
        <meta property='dcterms:modified'>2026-01-01T00:00:00Z</meta></metadata>\
        <manifest>\
        <item id='c0' href='page.xhtml' media-type='application/xhtml+xml'/>\
        <item id='thumb' href='thumb.png' media-type='image/png'/>\
        </manifest><spine><itemref idref='c0'/></spine></package>";
    let xhtml = "<?xml version='1.0' encoding='UTF-8'?>\
        <html xmlns='http://www.w3.org/1999/xhtml'><head><style>\
        #card { float: left; width: 50%; margin-left: 20px; padding: 10px; \
                border: 1px solid #333; }\
        #card ol { margin-left: 36px; }\
        #thumb { width: 80px; height: 80px; background-image: url(thumb.png); }\
        </style></head><body>\
        <div id='card'><h2>Instructions</h2><ol><li>First step</li><li>Second step</li></ol></div>\
        <div id='thumb'>&#160;</div>\
        </body></html>";
    let container = container_xml("OPS/book.opf");
    let bytes = pack(&[
        ("META-INF/container.xml", &container),
        ("OPS/book.opf", opf.as_bytes()),
        ("OPS/page.xhtml", xhtml.as_bytes()),
        ("OPS/thumb.png", RED_PNG),
    ]);
    let book = Book::open(Arc::new(bytes), &Typography::default()).unwrap();
    let lines = book.text(0);
    let line_x = |needle: &str| {
        lines
            .iter()
            .find(|l| {
                l.chars
                    .iter()
                    .map(|c| c.ch)
                    .collect::<String>()
                    .contains(needle)
            })
            .and_then(|l| l.chars.first())
            .map(|c| c.rect[0])
            .unwrap()
    };
    let (head_x, item_x) = (line_x("Instructions"), line_x("First"));
    assert!(
        item_x > head_x + 20.0,
        "list item should sit indented inside floated box: head={head_x}, item={item_x}"
    );
    assert!(
        !book.pictures(0).is_empty(),
        "colour background-image thumbnail should be preserved in Flow::pictures"
    );
}
