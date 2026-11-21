//! Word levels where "quick" is hard, and a dictionary pack, for suites.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use egui_kittest::Harness;
use mareader_dict::{PACKS, build as build_pack, db_file};
use mareader_ui_egui::MareaderApp;
use parquet::basic::{Compression, ZstdLevel};
use parquet::data_type::{ByteArray, ByteArrayType, FloatType};
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::parser::parse_message_type;

type H = Harness<'static, MareaderApp>;

/// "quick" is C2; the rest of its line is easy.
const ROWS: &[(&str, &str, f32)] = &[
    ("the", "DT", 1.0),
    ("quick", "JJ", 6.0),
    ("brown", "JJ", 1.0),
    ("fox", "NN", 2.0),
];

/// A parquet shaped like the real dataset.
fn dataset(path: &Path) {
    let message = "message schema {
        REQUIRED BYTE_ARRAY word (UTF8);
        OPTIONAL BYTE_ARRAY pos_tag (UTF8);
        REQUIRED FLOAT level;
        OPTIONAL BYTE_ARRAY lemma (UTF8);
    }";
    let schema = Arc::new(parse_message_type(message).unwrap());
    let zstd = Compression::ZSTD(ZstdLevel::default());
    let props = Arc::new(WriterProperties::builder().set_compression(zstd).build());
    let mut writer = SerializedFileWriter::new(File::create(path).unwrap(), schema, props).unwrap();
    let mut group = writer.next_row_group().unwrap();
    let words: Vec<ByteArray> = ROWS.iter().map(|r| r.0.into()).collect();
    let tags: Vec<ByteArray> = ROWS.iter().map(|r| r.1.into()).collect();
    let tagged = vec![1i16; ROWS.len()];
    let levels: Vec<f32> = ROWS.iter().map(|r| r.2).collect();
    let none = vec![0i16; ROWS.len()];
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<ByteArrayType>();
    typed.write_batch(&words, None, None).unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<ByteArrayType>();
    typed.write_batch(&tags, Some(&tagged), None).unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<FloatType>();
    typed.write_batch(&levels, None, None).unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<ByteArrayType>();
    typed.write_batch(&[], Some(&none), None).unwrap();
    col.close().unwrap();
    group.close().unwrap();
    writer.close().unwrap();
}

/// A folder with the word table converted, and one with the Burmese pack.
pub(crate) fn shelves() -> (tempfile::TempDir, tempfile::TempDir) {
    let words = tempfile::tempdir().unwrap();
    let parquet = words.path().join("source.parquet");
    dataset(&parquet);
    mareader_cefr::build(&parquet, &words.path().join("cefr.db")).unwrap();
    std::fs::remove_file(parquet).unwrap();
    let packs = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dict/tests/fixtures");
    let pack = PACKS.iter().find(|p| p.id == "mcfnlp-en-my").unwrap();
    build_pack(
        &fixtures.join("en-my.parquet"),
        &db_file(packs.path(), pack),
    )
    .unwrap();
    (words, packs)
}

/// A reader with these word levels and, maybe, dictionary packs.
pub(crate) fn levels_reader(words: &Path, packs: Option<&Path>) -> H {
    let mut app = MareaderApp::default();
    let words = words.to_string_lossy().into_owned();
    let packs = packs.map(|p| p.to_string_lossy().into_owned());
    let mut options = vec![
        ("workers", "0"),
        ("explain", "canned"),
        ("animations", "false"),
        ("cefr", words.as_str()),
        ("pdf", "sample:5"),
    ];
    if let Some(packs) = &packs {
        options.push(("dictionaries", packs.as_str()));
    }
    for (k, v) in options {
        app.set_option(k, v).expect("a known option");
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}
