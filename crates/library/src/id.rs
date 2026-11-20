//! Ids: what a file holds, which book it is, and which device wrote.

use std::fmt;

use serde::{Deserialize, Serialize};

/// What a file holds: a hash of its size and bytes (`disk::measure`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct ContentId(pub u128);

/// One book in the library, kept through moves, renames and edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct BookId(pub u128);

/// The machine that made a change; random, made once per library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct DeviceId(pub u64);

/// A folder the library takes books from; local, like its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct FolderId(pub u64);

/// A shelf a reader made; random, so two machines never clash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct ShelfId(pub u128);

impl fmt::Display for ShelfId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl From<ShelfId> for String {
    fn from(id: ShelfId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for ShelfId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        parse_hex(&s, 32).map(Self)
    }
}

/// A marked word; random, so two machines never clash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct MarkId(pub u128);

impl fmt::Display for MarkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl From<MarkId> for String {
    fn from(id: MarkId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for MarkId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        parse_hex(&s, 32).map(Self)
    }
}

/// A page's AI result; random, so two machines never clash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct AiPageId(pub u128);

impl fmt::Display for AiPageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl From<AiPageId> for String {
    fn from(id: AiPageId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for AiPageId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        parse_hex(&s, 32).map(Self)
    }
}

/// A pointer row at a book or a shelf; random, like a shelf's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct LinkId(pub u128);

impl fmt::Display for LinkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl From<LinkId> for String {
    fn from(id: LinkId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for LinkId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        parse_hex(&s, 32).map(Self)
    }
}

/// A word studied, by lemma and class; the same on every machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct WordId(pub u128);

impl fmt::Display for WordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl From<WordId> for String {
    fn from(id: WordId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for WordId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        parse_hex(&s, 32).map(Self)
    }
}

/// A word met in one book; its book and word decide it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct SightingId(pub u128);

impl fmt::Display for SightingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl From<SightingId> for String {
    fn from(id: SightingId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for SightingId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        parse_hex(&s, 32).map(Self)
    }
}

impl BookId {
    /// Its first content's id: two machines agree unasked (`docs/sync.md`).
    pub fn original(content: ContentId) -> Self {
        Self(content.0)
    }
}

impl fmt::Display for BookId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// Fixed-width lowercase hex of `digits` digits, as ids are written.
pub(crate) fn parse_hex(s: &str, digits: usize) -> Result<u128, String> {
    let fits = s.len() == digits && s.bytes().all(|b| b.is_ascii_hexdigit());
    let value = fits.then(|| u128::from_str_radix(s, 16).ok()).flatten();
    value.ok_or_else(|| format!("not {digits} hex digits: {s:?}"))
}

impl From<BookId> for String {
    fn from(id: BookId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for BookId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        parse_hex(&s, 32).map(Self)
    }
}

impl From<ContentId> for String {
    fn from(id: ContentId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for ContentId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        parse_hex(&s, 32).map(Self)
    }
}

impl From<DeviceId> for String {
    fn from(id: DeviceId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for DeviceId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        let value = parse_hex(&s, 16)?;
        u64::try_from(value).map(Self).map_err(|e| e.to_string())
    }
}

impl fmt::Display for FolderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl From<FolderId> for String {
    fn from(id: FolderId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for FolderId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        let value = parse_hex(&s, 16)?;
        u64::try_from(value).map(Self).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_write_as_fixed_hex_and_read_back() {
        let id = BookId(0xabc);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"00000000000000000000000000000abc\"");
        assert_eq!(serde_json::from_str::<BookId>(&json).unwrap(), id);
        let device = DeviceId(u64::MAX);
        let json = serde_json::to_string(&device).unwrap();
        assert_eq!(serde_json::from_str::<DeviceId>(&json).unwrap(), device);
        // Short, long or non-hex text is refused, never guessed.
        assert!(serde_json::from_str::<BookId>("\"abc\"").is_err());
        assert!(serde_json::from_str::<DeviceId>("\"zzzzzzzzzzzzzzzz\"").is_err());
    }
}
