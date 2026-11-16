//! Bundled faces, plus the machine's own when a reader picks one.
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use parley::fontique::{
    Blob, Collection, CollectionOptions, FontInfoOverride, GenericFamily, SourceCache,
};
use parley::{FontContext, FontStyle, FontWeight, LayoutContext};

/// The reading face; its weight axis covers every weight setting.
pub const SERIF: &str = "Literata";
/// The heading face, as mareader sets headings in the sans slot.
pub const SANS: &str = "Inter";
/// Code and preformatted text.
pub const MONO: &str = "JetBrains Mono";
/// What a book's embedded face is registered as: never a machine's own.
pub const BOOK_FACE: &str = "book-face-";

/// Our own font files; a static, so their bytes have one address.
pub(crate) static BUNDLED: [&[u8]; 6] = [
    include_bytes!("../../../assets/fonts/Literata-Variable.ttf"),
    include_bytes!("../../../assets/fonts/Literata-Italic-Variable.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-Variable.ttf"),
];

/// Inter's regular, medium and semibold, for the interface to share.
pub fn inter() -> [&'static [u8]; 3] {
    let [_, _, regular, medium, semibold, _] = BUNDLED;
    [regular, medium, semibold]
}

/// Literata upright, for book-like type in the interface.
pub fn literata() -> &'static [u8] {
    let [upright, ..] = BUNDLED;
    upright
}

/// Which generic family a face stands in for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Serif,
    Sans,
    Mono,
}

impl Kind {
    /// The bundled face of this kind: every stack's last resort.
    pub fn bundled(self) -> &'static str {
        match self {
            Self::Serif => SERIF,
            Self::Sans => SANS,
            Self::Mono => MONO,
        }
    }
}

/// A face shipped with the app; `id` is what settings store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Face {
    pub id: &'static str,
    /// Its family name, which is also what pickers show.
    pub name: &'static str,
    pub kind: Kind,
}

/// Every bundled face, in picker order.
pub const FACES: &[Face] = &[
    Face {
        id: "literata",
        name: SERIF,
        kind: Kind::Serif,
    },
    Face {
        id: "inter",
        name: SANS,
        kind: Kind::Sans,
    },
    Face {
        id: "jetbrains-mono",
        name: MONO,
        kind: Kind::Mono,
    },
];

/// One entry of a font stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Family {
    Named(String),
    /// The system's own face of a kind, such as its default serif.
    Generic(Kind),
}

/// The stacks text is set in, tried in order; Inter covers symbols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Faces {
    pub body: Vec<Family>,
    pub heading: Vec<Family>,
    /// Code, inline and preformatted.
    pub mono: Vec<Family>,
}

impl Default for Faces {
    fn default() -> Self {
        let one = |name: &str| vec![Family::Named(name.to_owned())];
        Self {
            body: one(SERIF),
            heading: one(SANS),
            mono: one(MONO),
        }
    }
}

/// A run's grey level when drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Tone {
    #[default]
    Body,
    /// Quotes: a little lighter than body text.
    Soft,
    /// The smallest heading, as mareader mutes h6.
    Muted,
}

/// What parley carries per run for drawing: tone, and a code ground.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Ink {
    pub(crate) tone: Tone,
    pub(crate) code: bool,
    /// The book's colour, over the tone.
    pub(crate) colour: Option<[u8; 3]>,
    /// Drawn over a soft offset copy of itself.
    pub(crate) shadow: Option<crate::block::TextShadow>,
    /// Laid but not drawn.
    pub(crate) hidden: bool,
}

/// Font data plus layout scratch; one per layout job, never shared.
pub struct Fonts {
    pub(crate) fonts: FontContext,
    pub(crate) layout: LayoutContext<Ink>,
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Fonts")
    }
}

fn register(collection: &mut Collection) {
    for file in BUNDLED {
        collection.register_fonts(Blob::new(Arc::new(file)), None);
    }
}

/// The machine's faces and ours, scanned once; jobs take clones.
fn system() -> &'static Mutex<Collection> {
    static SYSTEM: OnceLock<Mutex<Collection>> = OnceLock::new();
    SYSTEM.get_or_init(|| {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: true,
        });
        register(&mut collection);
        Mutex::new(collection)
    })
}

/// One store of machine font files: each is mapped once, not per job.
fn sources() -> SourceCache {
    static SOURCES: OnceLock<SourceCache> = OnceLock::new();
    SOURCES.get_or_init(SourceCache::new_shared).clone()
}

impl Fonts {
    /// Bundled faces alone, generics mapped onto them: same layout anywhere.
    pub fn bundled() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        register(&mut collection);
        for (generic, kind) in [
            (GenericFamily::Serif, Kind::Serif),
            (GenericFamily::SansSerif, Kind::Sans),
            (GenericFamily::Monospace, Kind::Mono),
        ] {
            let id = collection.family_id(kind.bundled());
            collection.set_generic_families(generic, id.into_iter());
        }
        Self::with(collection, SourceCache::default())
    }

    /// The machine's faces as well; generics are the system's own.
    pub fn system() -> Self {
        let collection = system()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        Self::with(collection, sources())
    }

    fn with(collection: Collection, source_cache: SourceCache) -> Self {
        Self {
            fonts: FontContext {
                collection,
                source_cache,
            },
            layout: LayoutContext::new(),
        }
    }

    /// Another layout scratch sharing the same font collection and file cache.
    pub(crate) fn fork(&self) -> Self {
        Self::with(
            self.fonts.collection.clone(),
            self.fonts.source_cache.clone(),
        )
    }
}

/// A font a book ships, registered under the family its looks name.
#[derive(Clone)]
pub struct BookFace {
    pub family: String,
    data: Blob<u8>,
    pub weight: f32,
    pub italic: bool,
}

impl BookFace {
    pub fn new(family: String, bytes: Vec<u8>, weight: f32, italic: bool) -> Self {
        let data = Blob::new(Arc::new(bytes));
        Self {
            family,
            data,
            weight,
            italic,
        }
    }

    /// The file's size in bytes.
    pub fn len(&self) -> usize {
        self.data.as_ref().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl std::fmt::Debug for BookFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BookFace({}, {} bytes)", self.family, self.len())
    }
}

impl Fonts {
    /// A book's faces as well, each under its family; junk is skipped.
    pub fn with_faces(mut self, faces: &[BookFace]) -> Self {
        for face in faces {
            let style = match face.italic {
                true => FontStyle::Italic,
                false => FontStyle::Normal,
            };
            let info = FontInfoOverride {
                family_name: Some(&face.family),
                weight: Some(FontWeight::new(face.weight)),
                style: Some(style),
                ..FontInfoOverride::default()
            };
            let collection = &mut self.fonts.collection;
            collection.register_fonts(face.data.clone(), Some(info));
        }
        self
    }
}

/// Whether a face of this family name exists, bundled or installed.
pub fn has_family(name: &str) -> bool {
    let mut collection = system().lock().unwrap_or_else(PoisonError::into_inner);
    collection.family_id(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_is_registered() {
        let mut f = Fonts::bundled();
        let names: Vec<String> = f
            .fonts
            .collection
            .family_names()
            .map(str::to_owned)
            .collect();
        for face in FACES {
            assert!(names.iter().any(|n| n == face.name), "{names:?}");
        }
    }

    #[test]
    fn bundled_faces_count_as_installed_and_junk_does_not() {
        for face in FACES {
            assert!(has_family(face.name), "{}", face.name);
        }
        assert!(!has_family("No Such Face 7f3a"));
    }
}
