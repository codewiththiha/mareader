//! The dictionary packs: which parquets join which two
//! languages. One row adds a language.

/// One downloadable dictionary: a parquet in the wikidict repo.
#[derive(Debug, Clone, Copy)]
pub struct PackDef {
    /// The job id and the file's stem.
    pub id: &'static str,
    /// The download row's name.
    pub label: &'static str,
    /// The headword language (English in every shipped pack).
    pub source: &'static str,
    /// The definition language.
    pub target: &'static str,
    /// The parquet's name in `output/curated`.
    pub file: &'static str,
    /// The rows the pack carries, for the download row's caption.
    pub rows: u64,
    /// The primary URL.
    pub url: &'static str,
    /// Mirror URLs for the same body.
    pub mirrors: &'static [&'static str],
}

/// Every pack the app may offer. Adding a language is adding a row.
pub const PACKS: &[PackDef] = &[
    PackDef {
        id: "mcfnlp-en-my",
        label: "MCF NLP English–Myanmar",
        source: "en",
        target: "my",
        file: "mcfnlp-en-my.parquet",
        rows: 110_640,
        url: "https://raw.githubusercontent.com/codewiththiha/wikidict/main/output/curated/mcfnlp-en-my.parquet",
        mirrors: &[
            "https://cdn.jsdelivr.net/gh/codewiththiha/wikidict@main/output/curated/mcfnlp-en-my.parquet",
            "https://fastly.jsdelivr.net/gh/codewiththiha/wikidict@main/output/curated/mcfnlp-en-my.parquet",
        ],
    },
    PackDef {
        id: "jmdict-en-jp",
        label: "JMdict English–Japanese",
        source: "en",
        target: "jp",
        file: "jmdict-en-jp.parquet",
        rows: 441_348,
        url: "https://raw.githubusercontent.com/codewiththiha/wikidict/main/output/curated/jmdict-en-jp.parquet",
        mirrors: &[
            "https://cdn.jsdelivr.net/gh/codewiththiha/wikidict@main/output/curated/jmdict-en-jp.parquet",
            "https://fastly.jsdelivr.net/gh/codewiththiha/wikidict@main/output/curated/jmdict-en-jp.parquet",
        ],
    },
    PackDef {
        id: "wiktionary-en-fr",
        label: "Wiktionary English–French",
        source: "en",
        target: "fr",
        file: "wiktionary-en-fr.parquet",
        rows: 128_263,
        url: "https://raw.githubusercontent.com/codewiththiha/wikidict/main/output/curated/wiktionary-en-fr.parquet",
        mirrors: &[
            "https://cdn.jsdelivr.net/gh/codewiththiha/wikidict@main/output/curated/wiktionary-en-fr.parquet",
            "https://fastly.jsdelivr.net/gh/codewiththiha/wikidict@main/output/curated/wiktionary-en-fr.parquet",
        ],
    },
];

/// The pack with this id, if it is offered.
pub fn pack(id: &str) -> Option<&'static PackDef> {
    PACKS.iter().find(|pack| pack.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str =
        "https://raw.githubusercontent.com/codewiththiha/wikidict/main/output/curated";
    const CDN: &str = "https://cdn.jsdelivr.net/gh/codewiththiha/wikidict@main/output/curated";
    const FASTLY: &str =
        "https://fastly.jsdelivr.net/gh/codewiththiha/wikidict@main/output/curated";

    #[test]
    fn every_pack_points_at_the_same_body_three_times() {
        for def in PACKS {
            assert!(def.url.starts_with(REPO), "{}", def.id);
            assert_eq!(def.mirrors.len(), 2);
            assert!(def.mirrors[0].starts_with(CDN), "{}", def.id);
            assert!(def.mirrors[1].starts_with(FASTLY), "{}", def.id);
            assert_eq!(def.url.rsplit('/').next(), Some(def.file));
        }
    }

    #[test]
    fn ids_are_the_stems_the_builder_lands_on() {
        for def in PACKS {
            assert!(def.file.starts_with(def.id));
            assert_eq!(pack(def.id).map(|p| p.id), Some(def.id));
        }
    }
}
