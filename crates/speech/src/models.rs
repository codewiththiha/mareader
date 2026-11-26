//! Voice models to download: their files, voices and where they come from.

/// One file a model needs, by its path inside the model's folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelFile {
    pub path: &'static str,
    pub bytes: u64,
    /// Lowercase hex, checked before the file is kept.
    pub sha256: &'static str,
    /// Its own addresses, when not the model's.
    pub hosts: Option<&'static [&'static str]>,
    /// Its name there, when not `path`.
    pub from: Option<&'static str>,
}

/// How a voice sounds to a listener.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gender {
    Female,
    Male,
    Neutral,
}

impl Gender {
    pub fn label(self) -> &'static str {
        match self {
            Self::Female => "female",
            Self::Male => "male",
            Self::Neutral => "neutral",
        }
    }
}

/// A voice a model speaks in; each downloads on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelVoice {
    pub id: &'static str,
    pub name: &'static str,
    /// A BCP 47 tag, as `en-GB`.
    pub language: &'static str,
    pub gender: Gender,
    /// None when the voice comes inside every build.
    pub file: Option<ModelFile>,
    /// A small file the voice needs beside `file`, such as its settings.
    pub extra: Option<ModelFile>,
    /// Files other voices need too, such as a language's dictionaries.
    pub shared: &'static [ModelFile],
    /// What sets it apart, as `Slower to make`; empty if nothing.
    pub note: &'static str,
}

impl ModelVoice {
    /// Every file the voice downloads.
    pub fn files(&self) -> Vec<ModelFile> {
        let own = self.file.into_iter().chain(self.extra);
        own.chain(self.shared.iter().copied()).collect()
    }

    pub fn bytes(&self) -> u64 {
        self.files().iter().map(|f| f.bytes).sum()
    }
}

/// A setting a model offers, as a share of each voice's own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knob {
    /// Its key in settings.
    pub id: &'static str,
    pub name: &'static str,
    /// One line on what it changes.
    pub about: &'static str,
    /// Least and most; 1 is the voice as it was made.
    pub range: (f32, f32),
}

/// Ranges are listed numbers, never NaN, so each equals itself.
impl Eq for Knob {}

impl Knob {
    /// `value` within range; a value not set, or not a number, is 1.
    pub fn clamp(&self, value: Option<f32>) -> f32 {
        let value = value.filter(|v| v.is_finite()).unwrap_or(1.0);
        value.clamp(self.range.0, self.range.1)
    }
}

/// One way to get a model's weights: the voices are shared, the sizes differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Build {
    pub id: &'static str,
    pub name: &'static str,
    /// One line on what it trades.
    pub about: &'static str,
    pub file: ModelFile,
    /// A small file the build needs beside `file`, such as its voices.
    pub extra: Option<ModelFile>,
}

impl Build {
    /// Every file the build downloads.
    pub fn files(&self) -> Vec<ModelFile> {
        std::iter::once(self.file).chain(self.extra).collect()
    }

    pub fn bytes(&self) -> u64 {
        self.files().iter().map(|f| f.bytes).sum()
    }
}

/// A model on offer: the engine it becomes once downloaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    /// Its engine id and its folder's name.
    pub id: &'static str,
    pub name: &'static str,
    /// One line on what it is.
    pub about: &'static str,
    pub license: &'static str,
    /// Addresses a file's path is joined to, tried in turn.
    pub hosts: &'static [&'static str],
    /// What every build needs before any voice.
    pub files: &'static [ModelFile],
    /// Its weights in each size, best first; none when voices bring their own.
    pub builds: &'static [Build],
    pub voices: &'static [ModelVoice],
    /// Comes with the model, so it can speak at once.
    pub first_voice: &'static str,
    /// Settings that change how it sounds.
    pub knobs: &'static [Knob],
}

impl Model {
    /// The voice with this id, if it has one.
    pub fn voice(&self, id: &str) -> Option<&'static ModelVoice> {
        self.voices.iter().find(|v| v.id == id)
    }

    /// The voice the model downloads with.
    pub fn first(&self) -> Option<&'static ModelVoice> {
        self.voice(self.first_voice)
    }

    pub fn build(&self, id: &str) -> Option<&'static Build> {
        self.builds.iter().find(|b| b.id == id)
    }

    /// The files every build shares: its files and first voice.
    pub fn core(&self) -> Vec<ModelFile> {
        let mut files: Vec<ModelFile> = self.files.to_vec();
        // Small files first, so the big one ends the download.
        files.extend(self.first().iter().flat_map(|v| v.files()));
        files.sort_by_key(|f| f.bytes);
        files
    }

    /// What the shared files weigh.
    pub fn bytes(&self) -> u64 {
        self.core().iter().map(|f| f.bytes).sum()
    }

    /// What a first download with `build` weighs.
    pub fn bytes_with(&self, build: &Build) -> u64 {
        self.bytes() + build.bytes()
    }

    /// Every address `file` can be fetched from.
    pub fn urls(&self, file: &ModelFile) -> Vec<String> {
        let hosts = file.hosts.unwrap_or(self.hosts);
        let name = file.from.unwrap_or(file.path);
        hosts.iter().map(|h| format!("{h}{name}")).collect()
    }
}

#[cfg(any(not(target_arch = "wasm32"), test))]
pub const RETIRED: &[(&str, &str)] = &[
    ("piper", "en/en_US/kathleen/low/en_US-kathleen-low.onnx"),
    (
        "piper",
        "en/en_US/kathleen/low/en_US-kathleen-low.onnx.json",
    ),
    ("piper", "en/en_US/joe/medium/en_US-joe-medium.onnx"),
    ("piper", "en/en_US/joe/medium/en_US-joe-medium.onnx.json"),
    ("piper", "en/en_US/mike/medium/en_US-mike-medium.onnx"),
    ("piper", "en/en_US/mike/medium/en_US-mike-medium.onnx.json"),
    (
        "piper",
        "en/en_US/reza_ibrahim/medium/en_US-reza_ibrahim-medium.onnx",
    ),
    (
        "piper",
        "en/en_US/reza_ibrahim/medium/en_US-reza_ibrahim-medium.onnx.json",
    ),
    ("piper", "en/en_US/sam/medium/en_US-sam-medium.onnx"),
    ("piper", "en/en_US/sam/medium/en_US-sam-medium.onnx.json"),
    ("piper", "en/en_GB/alba/medium/en_GB-alba-medium.onnx"),
    ("piper", "en/en_GB/alba/medium/en_GB-alba-medium.onnx.json"),
];

/// Every model on offer.
pub const MODELS: &[Model] = &[KOKORO, PIPER, KITTEN, PHONON];

/// How English words sound, for word cards: the voices' own data, alone.
pub const PRONUNCIATION: Model = Model {
    id: "pronunciation",
    name: "Pronunciation",
    about: "How English words sound, in American and British English",
    license: "Apache 2.0",
    hosts: G2P_HOSTS,
    files: &G2P,
    builds: &[],
    voices: &[],
    first_voice: "",
    knobs: &[],
};

/// Downloads beside the voice models, sharing their files.
pub const AIDS: &[Model] = &[PRONUNCIATION];

/// Pinned, so a file never changes under its checksum.
const KOKORO_HOSTS: &[&str] = &[
    "https://huggingface.co/hexgrad/Kokoro-82M/resolve/f3ff3571791e39611d31c381e3a41a3af07b4987/",
    "https://hf-mirror.com/hexgrad/Kokoro-82M/resolve/f3ff3571791e39611d31c381e3a41a3af07b4987/",
];

/// The same weights converted to GGUF, Apache 2.0 as well; pinned.
const GGUF_HOSTS: &[&str] = &[
    "https://huggingface.co/cstr/kokoro-82m-GGUF/resolve/ab19534a3b5cc12bc2741478e9790eadb328654a/",
    "https://hf-mirror.com/cstr/kokoro-82m-GGUF/resolve/ab19534a3b5cc12bc2741478e9790eadb328654a/",
];

/// The same weights exported to ONNX, Apache 2.0 as well; pinned.
const ONNX_HOSTS: &[&str] = &[
    "https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/1939ad2a8e416c0acfeecc08a694d14ef25f2231/",
    "https://hf-mirror.com/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/1939ad2a8e416c0acfeecc08a694d14ef25f2231/",
];

/// Pinned to a commit; the files are loqui-g2p 0.4.0's, byte for byte.
const G2P_HOSTS: &[&str] = &[
    "https://raw.githubusercontent.com/Awakened-Labs/loqui/717691aadf67408c85d87dc33e5d1d77b2ea4b4d/crates/loqui-g2p/data/",
    "https://cdn.jsdelivr.net/gh/Awakened-Labs/loqui@717691aadf67408c85d87dc33e5d1d77b2ea4b4d/crates/loqui-g2p/data/",
];

/// English pronunciation data, read by `mareader-g2p`.
const fn g2p(path: &'static str, bytes: u64, sha256: &'static str) -> ModelFile {
    ModelFile {
        path,
        bytes,
        sha256,
        hosts: Some(G2P_HOSTS),
        from: None,
    }
}

/// Every file `mareader-g2p` reads, for both dialects.
const G2P: [ModelFile; 8] = [
    g2p(
        "spacy-en-tokenizer.json",
        79_554,
        "2ffa6b3b6f4da114531ff897c02f3d2551485ef45c3ca1f6e6b2fdc80e034d1d",
    ),
    g2p(
        "tagger-en.bin",
        1_415_442,
        "df3d6cb2c34d7da57f92e47ae9dea6bcc9c9f8ed7cf9ceec703e84d7d67d698f",
    ),
    g2p(
        "misaki-us_gold.json",
        3_000_469,
        "dc414872a49a28ae6c141463d502fd945f3b2fde040484fdc47d00cc4612686f",
    ),
    g2p(
        "misaki-us_silver.json",
        3_099_517,
        "de8f67be911bb6c659187b4a65fd966b6a30e56350e0f790d763210b053ac475",
    ),
    g2p(
        "g2p-en-us.safetensors",
        3_011_692,
        "dc4a02e62d4fcb4bb4097ecf00db89b8e1a12a549a52ab6adfbba220b80a55c5",
    ),
    g2p(
        "misaki-gb_gold.json",
        2_838_552,
        "29e62f4b60261c88f7f3c2c7811ca3825978948090b72d2b27d565b729282f71",
    ),
    g2p(
        "misaki-gb_silver.json",
        3_663_898,
        "48131e2d92ccc41655f4543e87e0f938e71463eb5a54be7f0693bb712ebb6bce",
    ),
    g2p(
        "g2p-en-gb.safetensors",
        3_011_692,
        "4994f474bb6f4584076a4e98189caaec11aa3f773a0d82d5bc263a03dd07e703",
    ),
];

/// Natural speech from a small model that runs on the computer.
pub const KOKORO: Model = Model {
    id: "kokoro",
    name: "Kokoro",
    about: "Natural voices in English, Spanish, Italian, Portuguese, French, Hindi and Chinese from a small model, 82 million parameters",
    license: "Apache 2.0",
    hosts: KOKORO_HOSTS,
    files: &[
        ModelFile {
            path: "config.json",
            bytes: 2_351,
            sha256: "5abb01e2403b072bf03d04fde160443e209d7a0dad49a423be15196b9b43c17f",
            hosts: None,
            from: None,
        },
        G2P[0],
        G2P[1],
        G2P[2],
        G2P[3],
        G2P[4],
        G2P[5],
        G2P[6],
        G2P[7],
    ],
    builds: &[
        Build {
            id: "full",
            name: "Full",
            about: "The original weights: the best sound",
            file: ModelFile {
                path: "kokoro-v1_0.pth",
                bytes: 327_212_226,
                sha256: "496dba118d1a58f5f3db2efc88dbdc216e0483fc89fe6e47ee1f2c53f18ad1e4",
                hosts: None,
                from: None,
            },
            extra: None,
        },
        Build {
            id: "half",
            name: "Half",
            about: "Half-precision weights: half the download, near the same sound",
            file: ModelFile {
                path: "kokoro-82m-f16.gguf",
                bytes: 163_728_096,
                sha256: "341d5e96c2d7f06fc71dbc8d955150a5b6e3a59840f8905cb3fbe9b12120df52",
                hosts: Some(GGUF_HOSTS),
                from: None,
            },
            extra: None,
        },
        Build {
            id: "small",
            name: "Small",
            about: "8-bit weights: a small download, a little rougher",
            file: ModelFile {
                path: "kokoro-82m-q8_0.gguf",
                bytes: 141_322_336,
                sha256: "3c8b60064b287a10abd35396a59c9981675c618db8bc91bd290db63f55eb492d",
                hosts: Some(GGUF_HOSTS),
                from: None,
            },
            extra: None,
        },
        Build {
            id: "tiny",
            name: "Tiny",
            about: "8-bit weights packed tighter: the smallest download, rougher still",
            file: ModelFile {
                path: "kokoro-v1_0-q8.onnx",
                bytes: 92_361_116,
                sha256: "fbae9257e1e05ffc727e951ef9b9c98418e6d79f1c9b6b13bd59f5c9028a1478",
                hosts: Some(ONNX_HOSTS),
                from: Some("onnx/model_quantized.onnx"),
            },
            extra: None,
        },
    ],
    voices: KOKORO_VOICES,
    first_voice: "af_heart",
    knobs: &[],
};

/// A Kokoro voice; `path` is `voices/<id>.pt`.
const fn voice(
    (id, name, language): (&'static str, &'static str, &'static str),
    feminine: bool,
    path: &'static str,
    (bytes, sha256): (u64, &'static str),
) -> ModelVoice {
    ModelVoice {
        id,
        name,
        language,
        gender: match feminine {
            true => Gender::Female,
            false => Gender::Male,
        },
        file: Some(ModelFile {
            path,
            bytes,
            sha256,
            hosts: None,
            from: None,
        }),
        extra: None,
        shared: &[],
        note: "",
    }
}

/// pypinyin 0.55.0's readings, pinned to its release commit.
const PINYIN_HOSTS: &[&str] = &[
    "https://raw.githubusercontent.com/mozillazg/python-pinyin/df101577145af2eb1abe5656e592e34e3bb56d23/pypinyin/",
    "https://cdn.jsdelivr.net/gh/mozillazg/python-pinyin@df101577145af2eb1abe5656e592e34e3bb56d23/pypinyin/",
];

/// jieba 0.42.1's words, pinned to its release commit.
const JIEBA_HOSTS: &[&str] = &[
    "https://raw.githubusercontent.com/fxsjy/jieba/1e20c89b66f56c9301b0feed211733ffaa1bd72a/jieba/",
    "https://cdn.jsdelivr.net/gh/fxsjy/jieba@1e20c89b66f56c9301b0feed211733ffaa1bd72a/jieba/",
];

/// What `mareader-hanzi` reads, in `hanzi/`; every Chinese voice shares it.
pub(crate) const HANZI: [ModelFile; 4] = [
    hanzi(
        ("hanzi/pinyin_dict.json", "pinyin_dict.json", PINYIN_HOSTS),
        788_780,
        "5f294c01e6c6c0a1c8e329c79335a3f8e0b27d06bf1de7a99244b765892d1e5b",
    ),
    hanzi(
        ("hanzi/phrases_dict.json", "phrases_dict.json", PINYIN_HOSTS),
        2_545_585,
        "a45ff140a6b631ca9c82127b280a2f414e0aba6bb2824a0e9d1e77fff359c665",
    ),
    hanzi(
        ("hanzi/dict.txt", "dict.txt", JIEBA_HOSTS),
        5_071_852,
        "7197c3211ddd98962b036cdf40324d1ea2bfaa12bd028e68faa70111a88e12a8",
    ),
    hanzi(
        ("hanzi/prob_emit.py", "finalseg/prob_emit.py", JIEBA_HOSTS),
        1_321_732,
        "27d46b1c9efe4dd148fde8be042a21be40e3562d0c7f1273f9de7abae12ebb8d",
    ),
];

/// Chinese reading data: its path here, its name there, and where.
const fn hanzi(
    (path, from, hosts): (&'static str, &'static str, &'static [&'static str]),
    bytes: u64,
    sha256: &'static str,
) -> ModelFile {
    ModelFile {
        path,
        bytes,
        sha256,
        hosts: Some(hosts),
        from: Some(from),
    }
}

/// A Kokoro voice that reads Chinese, so it needs the dictionaries.
const fn chinese(voice: ModelVoice) -> ModelVoice {
    ModelVoice {
        shared: &HANZI,
        ..voice
    }
}

/// Ids: language `a`, `b`, `e`, `i`, `p`, `f`, `h` or `z`, then `f` or `m`.
const KOKORO_VOICES: &[ModelVoice] = &[
    voice(
        ("af_alloy", "Alloy", "en-US"),
        true,
        "voices/af_alloy.pt",
        (
            523_425,
            "6d877149dd8b348fbad12e5845b7e43d975390e9f3b68a811d1d86168bef5aa3",
        ),
    ),
    voice(
        ("af_aoede", "Aoede", "en-US"),
        true,
        "voices/af_aoede.pt",
        (
            523_425,
            "c03bd1a4c3716c2d8eaa3d50022f62d5c31cfbd6e15933a00b17fefe13841cc4",
        ),
    ),
    voice(
        ("af_bella", "Bella", "en-US"),
        true,
        "voices/af_bella.pt",
        (
            523_425,
            "8cb64e02fcc8de0327a8e13817e49c76c945ecf0052ceac97d3081480e8e48d6",
        ),
    ),
    voice(
        ("af_heart", "Heart", "en-US"),
        true,
        "voices/af_heart.pt",
        (
            523_425,
            "0ab5709b8ffab19bfd849cd11d98f75b60af7733253ad0d67b12382a102cb4ff",
        ),
    ),
    voice(
        ("af_jessica", "Jessica", "en-US"),
        true,
        "voices/af_jessica.pt",
        (
            523_435,
            "cdfdccb8cc975aa34ee6b89642963b0064237675de0e41a30ae64cc958dd4e87",
        ),
    ),
    voice(
        ("af_kore", "Kore", "en-US"),
        true,
        "voices/af_kore.pt",
        (
            523_420,
            "8bfbc512321c3db49dff984ac675fa5ac7eaed5a96cc31104d3a9080e179d69d",
        ),
    ),
    voice(
        ("af_nicole", "Nicole", "en-US"),
        true,
        "voices/af_nicole.pt",
        (
            523_430,
            "c5561808bcf5250fe8c5f5de32caf2d94f27e57e95befdb098c5c85991d4c5da",
        ),
    ),
    voice(
        ("af_nova", "Nova", "en-US"),
        true,
        "voices/af_nova.pt",
        (
            523_420,
            "e0233676ddc21908c37a1f102f6b88a59e4e5c1bd764983616eb9eda629dbcd2",
        ),
    ),
    voice(
        ("af_river", "River", "en-US"),
        true,
        "voices/af_river.pt",
        (
            523_425,
            "e149459bd9c084416b74756b9bd3418256a8b839088abb07d463730c369dab8f",
        ),
    ),
    voice(
        ("af_sarah", "Sarah", "en-US"),
        true,
        "voices/af_sarah.pt",
        (
            523_425,
            "49bd364ea3be9eb3e9685e8f9a15448c4883112a7c0ff7ab139fa4088b08cef9",
        ),
    ),
    voice(
        ("af_sky", "Sky", "en-US"),
        true,
        "voices/af_sky.pt",
        (
            523_351,
            "c799548aed06e0cb0d655a85a01b48e7f10484d71663f9a3045a5b9362e8512c",
        ),
    ),
    voice(
        ("am_adam", "Adam", "en-US"),
        false,
        "voices/am_adam.pt",
        (
            523_420,
            "ced7e284aba12472891be1da3ab34db84cc05cc02b5889535796dbf2d8b0cb34",
        ),
    ),
    voice(
        ("am_echo", "Echo", "en-US"),
        false,
        "voices/am_echo.pt",
        (
            523_420,
            "8bcfdc852bc985fb45c396c561e571ffb9183930071f962f1b50df5c97b161e8",
        ),
    ),
    voice(
        ("am_eric", "Eric", "en-US"),
        false,
        "voices/am_eric.pt",
        (
            523_420,
            "ada66f0eefff34ec921b1d7474d7ac8bec00cd863c170f1c534916e9b8212aae",
        ),
    ),
    voice(
        ("am_fenrir", "Fenrir", "en-US"),
        false,
        "voices/am_fenrir.pt",
        (
            523_430,
            "98e507eca1db08230ae3b6232d59c10aec9630022d19accac4f5d12fcec3c37a",
        ),
    ),
    voice(
        ("am_liam", "Liam", "en-US"),
        false,
        "voices/am_liam.pt",
        (
            523_420,
            "c82550757ddb31308b97f30040dda8c2d609a9e2de6135848d0a948368138518",
        ),
    ),
    voice(
        ("am_michael", "Michael", "en-US"),
        false,
        "voices/am_michael.pt",
        (
            523_435,
            "9a443b79a4b22489a5b0ab7c651a0bcd1a30bef675c28333f06971abbd47bd37",
        ),
    ),
    voice(
        ("am_onyx", "Onyx", "en-US"),
        false,
        "voices/am_onyx.pt",
        (
            523_420,
            "e8452be16cd0f6da7b4579eaf7b1e4506e92524882053d86d72b96b9a7fed584",
        ),
    ),
    voice(
        ("am_puck", "Puck", "en-US"),
        false,
        "voices/am_puck.pt",
        (
            523_420,
            "dd1d8973f4ce4b7d8ae407c77a435f485dabc052081b80ea75c4f30b84f36223",
        ),
    ),
    voice(
        ("am_santa", "Santa", "en-US"),
        false,
        "voices/am_santa.pt",
        (
            523_425,
            "7f2f7582fa2b1f160e90aafe6d0b442a685e773608b6667e545d743b073e97a7",
        ),
    ),
    voice(
        ("bf_alice", "Alice", "en-GB"),
        true,
        "voices/bf_alice.pt",
        (
            523_425,
            "d292651b6af6c0d81705c2580dcb4463fccc0ff7b8d618a471dbb4e45655b3f3",
        ),
    ),
    voice(
        ("bf_emma", "Emma", "en-GB"),
        true,
        "voices/bf_emma.pt",
        (
            523_420,
            "d0a423deabf4a52b4f49318c51742c54e21bb89bbbe9a12141e7758ddb5da701",
        ),
    ),
    voice(
        ("bf_isabella", "Isabella", "en-GB"),
        true,
        "voices/bf_isabella.pt",
        (
            523_440,
            "cdd4c37003805104d1d08fb1e05855c8fb2c68de24ca6e71f264a30aaa59eefd",
        ),
    ),
    voice(
        ("bf_lily", "Lily", "en-GB"),
        true,
        "voices/bf_lily.pt",
        (
            523_420,
            "6e09c2e481e2d53004d7e5ae7d3a325369e130a6f45c35a6002de75084be9285",
        ),
    ),
    voice(
        ("bm_daniel", "Daniel", "en-GB"),
        false,
        "voices/bm_daniel.pt",
        (
            523_430,
            "fc3fce4e9c12ed4dbc8fa9680cfe51ee190a96444ce7c3ad647549a30823fc5d",
        ),
    ),
    voice(
        ("bm_fable", "Fable", "en-GB"),
        false,
        "voices/bm_fable.pt",
        (
            523_425,
            "d44935f3135257a9064df99f007fc1342ff1aa767552b4a4fa4c3b2e6e59079c",
        ),
    ),
    voice(
        ("bm_george", "George", "en-GB"),
        false,
        "voices/bm_george.pt",
        (
            523_430,
            "f1bc812213dc59774769e5c80004b13eeb79bd78130b11b2d7f934542dab811b",
        ),
    ),
    voice(
        ("bm_lewis", "Lewis", "en-GB"),
        false,
        "voices/bm_lewis.pt",
        (
            523_425,
            "b5204750dcba01029d2ac9cec17aec3b20a6d64073c579d694a23cb40effbd0e",
        ),
    ),
    voice(
        ("ef_dora", "Dora", "es-ES"),
        true,
        "voices/ef_dora.pt",
        (
            523_420,
            "d9d69b0f8a2b87a345f269d89639f89dfbd1a6c9da0c498ae36dd34afcf35530",
        ),
    ),
    voice(
        ("em_alex", "Alex", "es-ES"),
        false,
        "voices/em_alex.pt",
        (
            523_420,
            "5eac53f767c3f31a081918ba531969aea850bed18fe56419b804d642c6973431",
        ),
    ),
    voice(
        ("em_santa", "Santa", "es-ES"),
        false,
        "voices/em_santa.pt",
        (
            523_430,
            "aa8620cb96cec705823efca0d956a63e158e09ad41aca934d354b7f0778f63cb",
        ),
    ),
    voice(
        ("if_sara", "Sara", "it-IT"),
        true,
        "voices/if_sara.pt",
        (
            523_425,
            "6c0b253b955fe32f1a1a86006aebe83d050ea95afd0e7be15182f087deedbf55",
        ),
    ),
    voice(
        ("im_nicola", "Nicola", "it-IT"),
        false,
        "voices/im_nicola.pt",
        (
            523_341,
            "234ed06648649f9bd874b37508ea17560b9c993ef85b4ddb3e3a71e062bd2c12",
        ),
    ),
    voice(
        ("pf_dora", "Dora", "pt-BR"),
        true,
        "voices/pf_dora.pt",
        (
            523_425,
            "07e4ff987c5d5a8c3995efd15cc4f0db7c4c15e881b198d8ab7f67ecf51f5eb7",
        ),
    ),
    voice(
        ("pm_alex", "Alex", "pt-BR"),
        false,
        "voices/pm_alex.pt",
        (
            523_425,
            "cf0ba8c573c2480fc54123683a35cf1e2ae130428e441eb91f9149bdb188a526",
        ),
    ),
    voice(
        ("pm_santa", "Santa", "pt-BR"),
        false,
        "voices/pm_santa.pt",
        (
            523_430,
            "d42103169c5c872abbafb9129133af7e942bb9d272c3cc3b95c203e7d7198c29",
        ),
    ),
    voice(
        ("ff_siwis", "Siwis", "fr-FR"),
        true,
        "voices/ff_siwis.pt",
        (
            523_425,
            "8073bf2d2c4b9543a90f2f0fd2144de4ed157e2d4b79ddeb0d5123066171fbc9",
        ),
    ),
    voice(
        ("hf_alpha", "Alpha", "hi-IN"),
        true,
        "voices/hf_alpha.pt",
        (
            523_425,
            "06906fe05746d13a79c5c01e21fd7233b05027221a933c9ada650f5aafc8f044",
        ),
    ),
    voice(
        ("hf_beta", "Beta", "hi-IN"),
        true,
        "voices/hf_beta.pt",
        (
            523_420,
            "63c0a1a6272e98d43f4511bba40e30dd9c8ceaf5f39af869509b9f51a71c503e",
        ),
    ),
    voice(
        ("hm_omega", "Omega", "hi-IN"),
        false,
        "voices/hm_omega.pt",
        (
            523_425,
            "b55f02a8e8483fffe0afa566e7d22ed8013acf47ad4f6bbee2795a840155703e",
        ),
    ),
    voice(
        ("hm_psi", "Psi", "hi-IN"),
        false,
        "voices/hm_psi.pt",
        (
            523_351,
            "2f0f055cea4f1083f4ef127ece48d71606347f6557dbb961c0ca5740a2da485b",
        ),
    ),
    chinese(voice(
        ("zf_xiaobei", "Xiaobei", "zh-CN"),
        true,
        "voices/zf_xiaobei.pt",
        (
            523_435,
            "9b76be63dab4f4f96962030acc0126a9aee9728608fbbe115e2b58a2bd504df6",
        ),
    )),
    chinese(voice(
        ("zf_xiaoni", "Xiaoni", "zh-CN"),
        true,
        "voices/zf_xiaoni.pt",
        (
            523_430,
            "95b49f169bf1640f4f43c25e13daa39f7b98d15d00823e83ef5c14b3ced59e49",
        ),
    )),
    chinese(voice(
        ("zf_xiaoxiao", "Xiaoxiao", "zh-CN"),
        true,
        "voices/zf_xiaoxiao.pt",
        (
            523_440,
            "cfaf6f2ded1ee56f1ff94fcd2b0e6cdf32e5b794bdc05b44e7439d44aef5887c",
        ),
    )),
    chinese(voice(
        ("zf_xiaoyi", "Xiaoyi", "zh-CN"),
        true,
        "voices/zf_xiaoyi.pt",
        (
            523_430,
            "b5235dbaeef85a4c613bf78af9a88ff63c25bac5f26ba77e36186d8b7ebf05e2",
        ),
    )),
    chinese(voice(
        ("zm_yunjian", "Yunjian", "zh-CN"),
        false,
        "voices/zm_yunjian.pt",
        (
            523_435,
            "76cbf8bad35901d011d9628a2fdceb7b4f1f127e7a3269cb393b3941eb7fc417",
        ),
    )),
    chinese(voice(
        ("zm_yunxi", "Yunxi", "zh-CN"),
        false,
        "voices/zm_yunxi.pt",
        (
            523_425,
            "dbe6e1ce7c3dbaf2f5667432947b638b1c6831ccbe154c4610dbcc44f431e27b",
        ),
    )),
    chinese(voice(
        ("zm_yunxia", "Yunxia", "zh-CN"),
        false,
        "voices/zm_yunxia.pt",
        (
            523_430,
            "bb2b03b08e84d64e1214440ce3b624987fac177f2eeb5bab8571799a3d980acd",
        ),
    )),
    chinese(voice(
        ("zm_yunyang", "Yunyang", "zh-CN"),
        false,
        "voices/zm_yunyang.pt",
        (
            523_435,
            "5238ac22e0c7f8b6cdd2eddd6e444b8a700b73c4674d9a047d59a94ff96379a2",
        ),
    )),
];

/// Pinned, so a file never changes under its checksum.
const PIPER_HOSTS: &[&str] = &[
    "https://huggingface.co/rhasspy/piper-voices/resolve/5041a7a49ae5b3a0f30e6837769eb03fdbb231bd/",
    "https://hf-mirror.com/rhasspy/piper-voices/resolve/5041a7a49ae5b3a0f30e6837769eb03fdbb231bd/",
];

/// Quick English voices, each a small model of its own.
pub const PIPER: Model = Model {
    id: "piper",
    name: "Piper",
    about: "Quick, clear English voices; each is a small model of its own",
    license: "MIT; voices public domain",
    hosts: PIPER_HOSTS,
    files: &G2P,
    builds: &[],
    voices: PIPER_VOICES,
    first_voice: "en_US-ljspeech-medium",
    knobs: &[
        Knob {
            id: "expression",
            name: "Expression",
            about: "How much the voice's tone rises and falls",
            range: (0.0, 1.5),
        },
        Knob {
            id: "rhythm",
            name: "Rhythm",
            about: "How freely the timing of words varies",
            range: (0.0, 1.5),
        },
    ],
};

type Listed = (&'static str, u64, &'static str);

/// A Piper voice: its weights, then its settings beside them.
const fn piper(
    (id, name, language): (&'static str, &'static str, &'static str),
    (gender, note): (Gender, &'static str),
    weights: Listed,
    settings: Listed,
) -> ModelVoice {
    ModelVoice {
        id,
        name,
        language,
        gender,
        file: Some(listed(weights)),
        extra: Some(listed(settings)),
        shared: &[],
        note,
    }
}

const fn listed((path, bytes, sha256): Listed) -> ModelFile {
    ModelFile {
        path,
        bytes,
        sha256,
        hosts: None,
        from: None,
    }
}

/// Voices made only from public-domain recordings, base voices included.
const PIPER_VOICES: &[ModelVoice] = &[
    piper(
        ("en_US-ljspeech-medium", "LJ", "en-US"),
        (Gender::Female, ""),
        (
            "en/en_US/ljspeech/medium/en_US-ljspeech-medium.onnx",
            63_531_379,
            "6f52a751e2349abe7a76735eb09dc1875298c77ea2342ffd2fef79ff81b87f22",
        ),
        (
            "en/en_US/ljspeech/medium/en_US-ljspeech-medium.onnx.json",
            4_972,
            "141d612cc0a95ed7efc1ca936b845c2364967f2e9217c5dbfcf69fc4d6c65860",
        ),
    ),
    piper(
        ("en_US-ljspeech-high", "LJ, finer", "en-US"),
        (
            Gender::Female,
            "Finer sound, about six times slower to make",
        ),
        (
            "en/en_US/ljspeech/high/en_US-ljspeech-high.onnx",
            114_199_011,
            "5d4f08ba6a2a48c44592eed3ce56bf85e9de3dd4e20df90541ae68a8310c029a",
        ),
        (
            "en/en_US/ljspeech/high/en_US-ljspeech-high.onnx.json",
            4_970,
            "7e1f4634af596d83cca997fb7a931ba80b70f8a316a2655ee69c55365e0ace14",
        ),
    ),
    piper(
        ("en_US-kristin-medium", "Kristin", "en-US"),
        (Gender::Female, ""),
        (
            "en/en_US/kristin/medium/en_US-kristin-medium.onnx",
            63_531_379,
            "5849957f929cbf720c258f8458692d6103fff2f0e3d3b19c8259474bb06a18d4",
        ),
        (
            "en/en_US/kristin/medium/en_US-kristin-medium.onnx.json",
            4_968,
            "5681426d4aead22195de70531eeeeddb46493cfaffc5764b2ea3db73428b651c",
        ),
    ),
    piper(
        ("en_US-john-medium", "John", "en-US"),
        (Gender::Male, ""),
        (
            "en/en_US/john/medium/en_US-john-medium.onnx",
            63_531_379,
            "789c6c875726e627ddee93d51d8727859abe9c091c3d141591f4b83c2072e988",
        ),
        (
            "en/en_US/john/medium/en_US-john-medium.onnx.json",
            4_965,
            "af60f177b6b550f3d7a302720c0fb89e7f94a82b5dca464775ef63b1c69ba09a",
        ),
    ),
    piper(
        ("en_US-norman-medium", "Norman", "en-US"),
        (Gender::Male, ""),
        (
            "en/en_US/norman/medium/en_US-norman-medium.onnx",
            63_531_379,
            "b9739443232a80a59c7d18810dd856899bf16a7964725f5ab81ea49b1351cb71",
        ),
        (
            "en/en_US/norman/medium/en_US-norman-medium.onnx.json",
            4_968,
            "6c2db7f558a4a8deb9fe822583c1c5105f6c4e834dd0f9de8ad17a888ee9fe1d",
        ),
    ),
    piper(
        ("en_US-bryce-medium", "Bryce", "en-US"),
        (Gender::Male, ""),
        (
            "en/en_US/bryce/medium/en_US-bryce-medium.onnx",
            63_531_379,
            "dc9caa6c313199ffb5ac698b6e542fa6cba388aeaf2731e25262e33b9810aef1",
        ),
        (
            "en/en_US/bryce/medium/en_US-bryce-medium.onnx.json",
            4_966,
            "7ceb1bc4af6d4e41b6d1edbb86c67e91e01eaa71f66db4cd0ae92ac704d415be",
        ),
    ),
    piper(
        ("en_GB-cori-medium", "Cori", "en-GB"),
        (Gender::Female, ""),
        (
            "en/en_GB/cori/medium/en_GB-cori-medium.onnx",
            63_531_379,
            "1899f98e5fb8310154f3c2973f4b8a929ba7245e722b3d3a85680b833d95f10d",
        ),
        (
            "en/en_GB/cori/medium/en_GB-cori-medium.onnx.json",
            4_966,
            "e262c16d7f192f69d4edd6b4ef8a5915379e67495fcc402f1ab15eeb33da3d36",
        ),
    ),
    piper(
        ("en_GB-cori-high", "Cori, finer", "en-GB"),
        (
            Gender::Female,
            "Finer sound, about six times slower to make",
        ),
        (
            "en/en_GB/cori/high/en_GB-cori-high.onnx",
            114_219_352,
            "470b4dd634c98f8a4850d7626ffc3dfc90774628eeef6605a6dd8f88f30a5903",
        ),
        (
            "en/en_GB/cori/high/en_GB-cori-high.onnx.json",
            4_963,
            "9e7fb5b5671612c22f3c81cbe46c1ae87b031a4632bcb509e499dad6f1e2adec",
        ),
    ),
];

/// Each Kitten size is its own repository, every one pinned.
macro_rules! kitten_hosts {
    ($repo:literal, $rev:literal) => {
        &[
            concat!(
                "https://huggingface.co/KittenML/",
                $repo,
                "/resolve/",
                $rev,
                "/"
            ),
            concat!(
                "https://hf-mirror.com/KittenML/",
                $repo,
                "/resolve/",
                $rev,
                "/"
            ),
        ]
    };
}

const KITTEN_NANO: &[&str] = kitten_hosts!(
    "kitten-tts-nano-0.8-fp32",
    "7a1db645b1f3ab9420761d87428e042b9cec3f26"
);
const KITTEN_NANO8: &[&str] = kitten_hosts!(
    "kitten-tts-nano-0.8-int8",
    "84781d74e29ee25217551556398b42f80593a813"
);
const KITTEN_MICRO: &[&str] = kitten_hosts!(
    "kitten-tts-micro-0.8",
    "1ccf72b2c2048fd17efac7de2fab32d10e225084"
);
const KITTEN_MINI: &[&str] = kitten_hosts!(
    "kitten-tts-mini-0.8",
    "c02725660cea441db4c383af69f1f26f5cd00947"
);

/// A Kitten file kept as `path`, fetched as `from` off `hosts`.
const fn kitten_file(
    (path, from): (&'static str, &'static str),
    hosts: &'static [&'static str],
    (bytes, sha256): (u64, &'static str),
) -> ModelFile {
    ModelFile {
        path,
        bytes,
        sha256,
        hosts: Some(hosts),
        from: Some(from),
    }
}

/// The nano builds' voices: one file, so both share it.
const NANO_VOICES: ModelFile = kitten_file(
    ("nano-voices.npz", "voices.npz"),
    KITTEN_NANO,
    (
        3_278_902,
        "8aa7cee235abb0739cb51e6559685f65a4dacd95568833d05699b1633f519b3f",
    ),
);

/// Small English models that run on the computer; every build has every voice.
pub const KITTEN: Model = Model {
    id: "kitten",
    name: "Kitten",
    about: "Light English voices from tiny models, 15 to 80 million parameters",
    license: "Apache 2.0",
    hosts: KITTEN_NANO,
    // American English is all Kitten speaks.
    files: &[G2P[0], G2P[1], G2P[2], G2P[3], G2P[4]],
    builds: &[
        Build {
            id: "mini",
            name: "Mini",
            about: "80 million parameters: the best sound",
            file: kitten_file(
                ("mini.onnx", "kitten_tts_mini_v0_8.onnx"),
                KITTEN_MINI,
                (
                    78_268_016,
                    "0f5bbae4fc4800c98dbc544a87ecfa79510de2fb8222db30d12e5bfe9177df91",
                ),
            ),
            extra: Some(kitten_file(
                ("mini-voices.npz", "voices.npz"),
                KITTEN_MINI,
                (
                    3_278_902,
                    "40ad2638952b77b7b2f30127e2608e169fc69dd256b53bd8aaa3409a33193c42",
                ),
            )),
        },
        Build {
            id: "micro",
            name: "Micro",
            about: "40 million parameters: near the best, sooner heard",
            file: kitten_file(
                ("micro.onnx", "kitten_tts_micro_v0_8.onnx"),
                KITTEN_MICRO,
                (
                    41_384_970,
                    "95481626fee1ba70ce683e69c534fc7cb38433c46ce42d3abbeafb4b9f1a4123",
                ),
            ),
            extra: Some(kitten_file(
                ("micro-voices.npz", "voices.npz"),
                KITTEN_MICRO,
                (
                    3_278_902,
                    "112710c1be8ad0e967c190fb0fd95cbe5848ec4791b93209f20b28b7da20dac1",
                ),
            )),
        },
        Build {
            id: "nano",
            name: "Nano",
            about: "15 million parameters: the quickest",
            file: kitten_file(
                ("nano.onnx", "kitten_tts_nano_v0_8.onnx"),
                KITTEN_NANO,
                (
                    56_767_095,
                    "320564d2615f235de972ca27a7f39551c94185cfa24ca85b07a29084135f1e5e",
                ),
            ),
            extra: Some(NANO_VOICES),
        },
        Build {
            id: "nano8",
            name: "Nano 8-bit",
            about: "Nano in 8-bit weights: the smallest download, a little rougher",
            file: kitten_file(
                ("nano-8bit.onnx", "kitten_tts_nano_v0_8.onnx"),
                KITTEN_NANO8,
                (
                    24_369_971,
                    "f7b0afcbee92870b32b8e0276d855b954dc25470c9f051b376ac7eee537c76fc",
                ),
            ),
            extra: Some(NANO_VOICES),
        },
    ],
    voices: KITTEN_VOICES,
    first_voice: "expr-voice-2-f",
    knobs: &[],
};

/// A voice inside every Kitten build.
const fn kitten_voice(id: &'static str, name: &'static str, gender: Gender) -> ModelVoice {
    ModelVoice {
        id,
        name,
        language: "en-US",
        gender,
        file: None,
        extra: None,
        shared: &[],
        note: "",
    }
}

const KITTEN_VOICES: &[ModelVoice] = &[
    kitten_voice("expr-voice-2-f", "Bella", Gender::Female),
    kitten_voice("expr-voice-2-m", "Jasper", Gender::Male),
    kitten_voice("expr-voice-3-f", "Luna", Gender::Female),
    kitten_voice("expr-voice-3-m", "Bruno", Gender::Male),
    kitten_voice("expr-voice-4-f", "Rosie", Gender::Female),
    kitten_voice("expr-voice-4-m", "Hugo", Gender::Male),
    kitten_voice("expr-voice-5-f", "Kiki", Gender::Female),
    kitten_voice("expr-voice-5-m", "Leo", Gender::Male),
];

/// Phonon's weights and tokenizer, pinned.
const PHONON_HOSTS: &[&str] = &[
    "https://huggingface.co/gradium/phonon-31m-en/resolve/92741c5b07d085ff840e26fea8ce43110b87852f/",
    "https://hf-mirror.com/gradium/phonon-31m-en/resolve/92741c5b07d085ff840e26fea8ce43110b87852f/",
];

/// Its voice catalogue, Apache 2.0 as well; pinned.
const PHONON_VOICE_HOSTS: &[&str] = &[
    "https://huggingface.co/gradium/phonon-voices/resolve/0ee17205f254b5abc43b266c8a0f215565c352f7/",
    "https://hf-mirror.com/gradium/phonon-voices/resolve/0ee17205f254b5abc43b266c8a0f215565c352f7/",
];

/// Every voice file weighs the same: one prompt of 125 frames.
const PHONON_VOICE_BYTES: u64 = 256_136;

const fn phonon_file(path: &'static str, bytes: u64, sha256: &'static str) -> ModelFile {
    ModelFile {
        path,
        bytes,
        sha256,
        hosts: None,
        from: None,
    }
}

/// Gradium's English model, run on the computer; voices download one by one.
pub const PHONON: Model = Model {
    id: "phonon",
    name: "Phonon",
    about: "Natural English voices from a 31 million parameter model",
    license: "Apache 2.0",
    hosts: PHONON_HOSTS,
    files: &[
        phonon_file(
            "config.json",
            4_634,
            "b5d0f88353638370d94035b7d73d7d6577c0554d78f8d7ecfd40490aae39ae85",
        ),
        phonon_file(
            "tokenizer.json",
            277_967,
            "691a00068fa7b989f2f3e5ae5daa2d1a5bb319f2d5360deafcfb19a8bb6e7936",
        ),
    ],
    builds: &[Build {
        id: "q8",
        name: "8-bit",
        about: "The model in 8-bit weights, as its makers ship it",
        file: phonon_file(
            "model.q8.gguf",
            64_512_352,
            "a7d9aad2cd6dc66473f24a08bc6d2939030ef4d751ef82349361fbef3272dbae",
        ),
        extra: None,
    }],
    voices: PHONON_VOICES,
    first_voice: "default",
    knobs: &[],
};

/// A voice from the catalogue; `path` is `voices/<id>.safetensors`.
const fn phonon_voice(
    (id, name, language): (&'static str, &'static str, &'static str),
    gender: Gender,
    path: &'static str,
    sha256: &'static str,
) -> ModelVoice {
    ModelVoice {
        id,
        name,
        language,
        gender,
        file: Some(ModelFile {
            path,
            bytes: PHONON_VOICE_BYTES,
            sha256,
            hosts: Some(PHONON_VOICE_HOSTS),
            from: None,
        }),
        extra: None,
        shared: &[],
        note: "",
    }
}

const PHONON_VOICES: &[ModelVoice] = &[
    // The checkpoint's own voice, which it downloads with.
    ModelVoice {
        id: "default",
        name: "Default",
        language: "en-US",
        gender: Gender::Male,
        file: Some(phonon_file(
            "default-voice.safetensors",
            PHONON_VOICE_BYTES,
            "dc537478eaadc1a5257fcda2e3501ab81e14c111a5d95a5838f350db2afd0f1a",
        )),
        extra: None,
        shared: &[],
        note: "",
    },
    phonon_voice(
        ("NbpkqMVS3CJeq2j8", "Zoey", "en-US"),
        Gender::Female,
        "voices/NbpkqMVS3CJeq2j8.safetensors",
        "97c7f8c253413574406ab80dd23f49c3ebb2517163b00a9bf86b9c4cf43e7e23",
    ),
    phonon_voice(
        ("NIWIOv5vTi5eRMj8", "Juniper", "en-US"),
        Gender::Female,
        "voices/NIWIOv5vTi5eRMj8.safetensors",
        "444378eb06b7c492b86e8fc7d13e4db1a3c0cf3e68891a960bf6f0a41a13ef35",
    ),
    phonon_voice(
        ("vjFyp2BcCyTlUmYb", "Everly", "en-US"),
        Gender::Female,
        "voices/vjFyp2BcCyTlUmYb.safetensors",
        "958ea94da14f46dd48c7f64adac11ed2ad2aa45f7ccd165dfebab5272664402e",
    ),
    phonon_voice(
        ("4SZHfMpw-p46Ywgs", "Harper", "en-US"),
        Gender::Female,
        "voices/4SZHfMpw-p46Ywgs.safetensors",
        "6b3da6e2d796a20c51af892c482bd6509fb88267a221aff83c870843dee2a123",
    ),
    phonon_voice(
        ("bXwrkHhURL1VFJdG", "Hadley", "en-US"),
        Gender::Female,
        "voices/bXwrkHhURL1VFJdG.safetensors",
        "a879938aa9a2df0a8220bdadef88137929f71e887bd91e7db205bd5fa11661d2",
    ),
    phonon_voice(
        ("nGzMHMPAWGaJFHOo", "Delia", "en-US"),
        Gender::Female,
        "voices/nGzMHMPAWGaJFHOo.safetensors",
        "a0c3dcb91b2247eca56536be9ae8e8becc1c807b1136c5fe101552d6a155b24a",
    ),
    phonon_voice(
        ("LMDIYE0Oa377Qh6Z", "Easton", "en-US"),
        Gender::Male,
        "voices/LMDIYE0Oa377Qh6Z.safetensors",
        "45c582673cb20edb04ddcca2b5a8865ddfb0a82599efadb269021e87d20fbdd4",
    ),
    phonon_voice(
        ("40QrMVgAxFiUnP9f", "Micah", "en-US"),
        Gender::Male,
        "voices/40QrMVgAxFiUnP9f.safetensors",
        "0de1098d1051686a138d068e83ff09deb5053328bc74ec9541a1fdb90bfc24da",
    ),
    phonon_voice(
        ("0zLApsZfVLLNaEGl", "Trevor", "en-US"),
        Gender::Male,
        "voices/0zLApsZfVLLNaEGl.safetensors",
        "dc1341c8937af5534e6bbcc5c0fc5e7c10312229b59061228074f93683fe2261",
    ),
    phonon_voice(
        ("_6Aslh2DxfmnRLmP", "Russell", "en-US"),
        Gender::Male,
        "voices/_6Aslh2DxfmnRLmP.safetensors",
        "beae9e4030efce0a935fcb0f807dc0784d598bcaa87ad4aacb047436b1eaa411",
    ),
    phonon_voice(
        ("qIxlZ7Ei8KvWlyQH", "Preston", "en-US"),
        Gender::Male,
        "voices/qIxlZ7Ei8KvWlyQH.safetensors",
        "6e04b84d3ae8751b22bca0a4f63936f9cb648f4f2182b2d0788071ea1c0b68a1",
    ),
    phonon_voice(
        ("d9XCyhjo4JhkHF1d", "Miles", "en-US"),
        Gender::Male,
        "voices/d9XCyhjo4JhkHF1d.safetensors",
        "eab20e4f159b48cfbc01b1885804e24f681f80e7a64f909182763a4e3361bee8",
    ),
    phonon_voice(
        ("6PWnV0Nq4wu7RVBT", "Maeve", "en-GB"),
        Gender::Female,
        "voices/6PWnV0Nq4wu7RVBT.safetensors",
        "56494ee67d533ad227ff0dfd0b1c13ff5aa91f1ad6e596e66c2986ba3005e844",
    ),
    phonon_voice(
        ("GgfEkEJtxZR7gnpy", "Freya", "en-GB"),
        Gender::Female,
        "voices/GgfEkEJtxZR7gnpy.safetensors",
        "eb3cd47418349e618408c16bea7153988b3ad04b41a6033039dd474a9a4dcd23",
    ),
    phonon_voice(
        ("uem82D50GRv2Dwma", "Pippa", "en-GB"),
        Gender::Female,
        "voices/uem82D50GRv2Dwma.safetensors",
        "d41a8f01d6e33450f1ce062d332914da10beee99e585dab531750706f187c48a",
    ),
    phonon_voice(
        ("y9bQqIDnyxwqK01t", "Elodie-Rose", "en-GB"),
        Gender::Female,
        "voices/y9bQqIDnyxwqK01t.safetensors",
        "21eade9d83b2e66b9364e1b6cd999a3a6704c000a71f27f34e05ace5cdfd58bb",
    ),
    phonon_voice(
        ("gDT1nz7Ie36ZhL-C", "Imogen", "en-GB"),
        Gender::Female,
        "voices/gDT1nz7Ie36ZhL-C.safetensors",
        "1980ab3c35fa98a25b5f3b5e643fd852e14ca8a12a4a26b06aaf1abf2a8c68b1",
    ),
    phonon_voice(
        ("m9MxpY6IUGXtWp76", "Lottie", "en-GB"),
        Gender::Female,
        "voices/m9MxpY6IUGXtWp76.safetensors",
        "a906ebf17e43fded12b3d6bd077abb2df33bc86eff51c1beeb82e5d416527db3",
    ),
    phonon_voice(
        ("gfP44RlHwr2kEwk7", "Bryony", "en-GB"),
        Gender::Female,
        "voices/gfP44RlHwr2kEwk7.safetensors",
        "bcc207a73b7885e45b49ab633d3377a10158292e3a7cef9cd6764ad107f1cb9b",
    ),
    phonon_voice(
        ("X6NceqF285Gr04gb", "Alfie", "en-GB"),
        Gender::Male,
        "voices/X6NceqF285Gr04gb.safetensors",
        "4de75497fff3030168e8aa6dca23ffeb332b1bc8c25d0bf3507ceadac953ae9c",
    ),
    phonon_voice(
        ("YyKcxPG7iIgS3jsK", "Hugh", "en-GB"),
        Gender::Male,
        "voices/YyKcxPG7iIgS3jsK.safetensors",
        "3169d74b47a54d1eb15e4c243f56393abe9fe90c7e1c308ecf96ee67618b152a",
    ),
    phonon_voice(
        ("zUCxzrOrugTSis03", "Edwin", "en-GB"),
        Gender::Male,
        "voices/zUCxzrOrugTSis03.safetensors",
        "ff1d0735b8b45710462b1d1f846da040f970c6eb62c7c2f36dbfb8e5bde74f1e",
    ),
    phonon_voice(
        ("JOW0vchyswkQUMDS", "Spencer", "en-GB"),
        Gender::Male,
        "voices/JOW0vchyswkQUMDS.safetensors",
        "cc9ac1107b0ffce282374a19beb7bfa95847724e6b7afbac991b4ae4277d5e6d",
    ),
    phonon_voice(
        ("dME3IWyZBvmh1n1q", "Toby", "en-GB"),
        Gender::Male,
        "voices/dME3IWyZBvmh1n1q.safetensors",
        "09ab315c7f4b72b4d71e66cb756184cedfb1f14d222d096e115bf95bfe7b29dc",
    ),
    phonon_voice(
        ("Zn7g6NxLqXlJb7Qi", "Teddy", "en-GB"),
        Gender::Male,
        "voices/Zn7g6NxLqXlJb7Qi.safetensors",
        "baea0ea9506f3237a5ebfc61afb7c72dbd189c5603b199ef42fbb00dbda379e0",
    ),
];

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn every_file_is_listed_with_a_size_and_a_checksum() {
        for model in MODELS {
            assert!(
                model.first().is_some(),
                "{} lacks its first voice",
                model.id
            );
            let all = (model.files.iter().copied())
                .chain(model.builds.iter().flat_map(|b| b.files()))
                .chain(model.voices.iter().flat_map(|v| v.files()));
            for f in all {
                assert!(f.bytes > 0, "{}", f.path);
                assert_eq!(f.sha256.len(), 64, "{}", f.path);
                assert!(f.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            }
            for v in model.voices {
                assert_eq!(model.voices.iter().filter(|w| w.id == v.id).count(), 1);
            }
            for b in model.builds {
                assert_eq!(model.builds.iter().filter(|c| c.id == b.id).count(), 1);
            }
            for f in (model.core().into_iter())
                .chain(model.builds.iter().flat_map(|b| b.files()))
                .chain(model.voices.iter().flat_map(|v| v.files()))
            {
                let urls = model.urls(&f);
                assert!(urls.len() > 1, "{} has no mirror", f.path);
                assert!(urls.iter().all(|u| u.starts_with("https://")));
                let name = f.from.unwrap_or(f.path);
                assert!(urls.iter().all(|u| u.ends_with(name)));
            }
        }
    }

    #[test]
    fn the_model_downloads_with_one_voice_and_a_build_of_its_choosing() {
        let core = KOKORO.core();
        assert_eq!(core.len(), 10);
        assert!(core.iter().any(|f| f.path == "voices/af_heart.pt"));
        assert!(core.iter().all(|f| !f.path.contains("kokoro-")));
        let g2p = 20_120_816;
        assert_eq!(KOKORO.bytes(), 2_351 + 523_425 + g2p);
        let ids: Vec<&str> = KOKORO.builds.iter().map(|b| b.id).collect();
        assert_eq!(ids, ["full", "half", "small", "tiny"]);
        let small = KOKORO.build("small").unwrap();
        assert_eq!(KOKORO.bytes_with(small), KOKORO.bytes() + 141_322_336);
        // Each smaller build is a smaller download.
        let sizes: Vec<u64> = KOKORO.builds.iter().map(|b| b.file.bytes).collect();
        assert!(sizes.windows(2).all(|w| w[0] > w[1]));
        assert_eq!(KOKORO.voices.len(), 49);
        for v in KOKORO.voices {
            let file = v.file.unwrap();
            assert_eq!(file.path, format!("voices/{}.pt", v.id));
            // Chinese voices share the dictionaries; the rest bring one file.
            match v.language {
                "zh-CN" => assert_eq!(v.files()[1..], HANZI),
                _ => assert_eq!(v.files(), [file]),
            }
        }
    }

    #[test]
    fn a_retired_file_is_listed_by_no_model() {
        for (m, path) in RETIRED {
            let model = MODELS.iter().find(|x| x.id == *m).unwrap();
            let voices = model.voices.iter().flat_map(|v| v.files());
            let builds = model.builds.iter().flat_map(|b| b.files());
            let mut all = model.files.iter().copied().chain(voices).chain(builds);
            assert!(all.all(|f| f.path != *path), "{path} is still offered");
        }
    }

    #[test]
    fn each_piper_voice_brings_its_weights_and_settings() {
        assert!(PIPER.builds.is_empty());
        assert_eq!(PIPER.files, &G2P[..]);
        let first = PIPER.first().unwrap();
        assert_eq!(PIPER.core().len(), G2P.len() + 2);
        assert_eq!(PIPER.bytes(), 20_120_816 + first.bytes());
        for v in PIPER.voices {
            let [weights, settings] = v.files()[..] else {
                panic!("{} has two files", v.id);
            };
            // As `en/en_US/john/medium/en_US-john-medium.onnx`.
            let [locale, speaker, quality] = v.id.splitn(3, '-').collect::<Vec<_>>()[..] else {
                panic!("{} is locale-speaker-quality", v.id);
            };
            let stem = format!("en/{locale}/{speaker}/{quality}/{}", v.id);
            assert_eq!(weights.path, format!("{stem}.onnx"));
            assert_eq!(settings.path, format!("{stem}.onnx.json"));
            assert_eq!(v.language, locale.replace('_', "-"));
            // High voices are slow to make, and say so.
            assert_eq!(
                quality == "high",
                !v.note.is_empty() && v.note.contains("slower")
            );
        }
        let knob = PIPER.knobs[0];
        assert_eq!(knob.clamp(None), 1.0);
        assert_eq!(knob.clamp(Some(f32::NAN)), 1.0);
        assert_eq!(knob.clamp(Some(9.0)), knob.range.1);
    }

    #[test]
    fn each_kitten_build_brings_its_voices_and_every_voice_comes_with_it() {
        assert_eq!(KITTEN.files, &G2P[..5]);
        assert_eq!(KITTEN.bytes(), 10_606_674);
        let ids: Vec<&str> = KITTEN.builds.iter().map(|b| b.id).collect();
        assert_eq!(ids, ["mini", "micro", "nano", "nano8"]);
        let nano = |id| KITTEN.build(id).unwrap().extra;
        assert_eq!(nano("nano"), nano("nano8"), "one voices file for both");
        assert_eq!(nano("nano").unwrap().path, "nano-voices.npz");
        for b in KITTEN.builds {
            let [onnx, voices] = b.files()[..] else {
                panic!("{} brings weights and voices", b.id);
            };
            assert!(onnx.path.ends_with(".onnx") && voices.path.ends_with(".npz"));
            assert_eq!(voices.from, Some("voices.npz"));
            assert_eq!(b.bytes(), onnx.bytes + 3_278_902);
        }
        let url = KITTEN.urls(&KITTEN.build("micro").unwrap().file);
        assert!(url[0].ends_with("kitten-tts-micro-0.8/resolve/1ccf72b2c2048fd17efac7de2fab32d10e225084/kitten_tts_micro_v0_8.onnx"));
        assert_eq!(KITTEN.voices.len(), 8);
        assert!(KITTEN.voices.iter().all(|v| v.files().is_empty()));
        assert!(KITTEN.core().iter().all(|f| KITTEN.files.contains(f)));
    }

    #[test]
    fn phonon_downloads_its_own_voice_and_each_other_alone() {
        let core = PHONON.core();
        assert_eq!(core.len(), 3);
        assert!(core.iter().any(|f| f.path == "default-voice.safetensors"));
        assert_eq!(PHONON.bytes(), 4_634 + 277_967 + 256_136);
        let q8 = PHONON.build("q8").unwrap();
        assert_eq!(PHONON.bytes_with(q8), PHONON.bytes() + 64_512_352);
        assert_eq!(PHONON.voices.len(), 26);
        for v in &PHONON.voices[1..] {
            let file = v.file.unwrap();
            assert_eq!(file.path, format!("voices/{}.safetensors", v.id));
            assert!(PHONON.urls(&file)[0].contains("phonon-voices"));
        }
        let us = PHONON.voices.iter().filter(|v| v.language == "en-US");
        let gb = PHONON.voices.iter().filter(|v| v.language == "en-GB");
        assert_eq!((us.count(), gb.count()), (13, 13));
    }

    #[test]
    fn the_models_bring_every_file_their_pronunciation_reads() {
        use mareader_g2p::{Dialect, TAGGER, TOKENIZER};
        let dialects = [Dialect::American, Dialect::British];
        let needed = dialects.iter().flat_map(|d| d.files());
        for name in needed.chain([TOKENIZER, TAGGER]) {
            for model in [KOKORO, PIPER] {
                assert!(model.files.iter().any(|f| f.path == name), "{name}");
            }
        }
    }
}
