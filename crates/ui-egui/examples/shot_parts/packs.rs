//! The shot's dictionary folder: the test packs, and AI dictionary meanings.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mareader_dict::{Dictionaries, PACKS, Query, Saved, build, db_file, language_code};

/// The Burmese and French test packs, built into a temporary folder; with
/// `ai`, the AI dictionary keeps "quick" in Burmese, English and Russian.
pub fn fixtures(ai: bool) -> Result<PathBuf, String> {
    let name = match ai {
        true => "mareader-shot-packs-ai",
        false => "mareader-shot-packs",
    };
    let dir = std::env::temp_dir().join(name);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dict/tests/fixtures");
    for (file, id) in [
        ("en-my.parquet", "mcfnlp-en-my"),
        ("en-fr.parquet", "wiktionary-en-fr"),
    ] {
        let pack = PACKS.iter().find(|p| p.id == id);
        let pack = pack.ok_or(format!("no pack {id}"))?;
        let db = db_file(&dir, pack);
        std::fs::create_dir_all(db.parent().unwrap_or(&dir)).map_err(|e| e.to_string())?;
        build(&fixtures.join(file), &db).map_err(|e| e.to_string())?;
    }
    if ai {
        remember(&dir)?;
    }
    Ok(dir)
}

/// What an AI wrote of "quick" in `lang`.
fn quick(lang: &str, meaning: &str, synonyms: &[&str]) -> Saved {
    Saved {
        word: "quick".into(),
        pos: "adjective".into(),
        lang: lang.into(),
        meaning: meaning.into(),
        synonyms: synonyms.iter().map(|s| (*s).to_owned()).collect(),
        usages: vec!["She gave a quick nod and left.".into()],
        model: "OpenAI · gpt-5-mini".into(),
        added: 0,
    }
}

/// Keeps the AI meanings in `dir`'s AI dictionary, replacing any there.
fn remember(dir: &Path) -> Result<(), String> {
    let mut dicts = Dictionaries::default();
    dicts.attach(dir.to_owned(), Arc::new(|| {}))?;
    dicts.set_inline(true);
    let saved = vec![
        quick("my", "အမြန်လုပ်သော", &["မြန်ဆန်သော"]),
        quick("en", "moving or done fast", &["fast", "swift", "brisk"]),
        quick(&language_code("Russian"), "быстрый", &["скорый"]),
    ];
    dicts.ask(Query::Remember {
        saved,
        replace: true,
    });
    let (_, replies) = dicts.poll();
    let failed = replies.into_iter().find_map(|r| r.failed);
    failed.map_or(Ok(()), Err)
}
