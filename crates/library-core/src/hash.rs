//! The two cheap measurements a [`Fingerprint`](crate::book::Fingerprint) is built from.
//!
//! Here rather than in the shell crate that calls them so the library and the
//! walk that feeds it cannot drift.

use std::time::{SystemTime, UNIX_EPOCH};

/// Past a PDF's header and into its first objects, and small enough that scanning a two-thousand-file folder costs one partial read per file.
pub const HEAD_BYTES: usize = 8 * 1024;

/// FNV-1a over the leading bytes, 32-bit. Not cryptographic: its job is to
/// separate two files that happen to share a length and a modification stamp.
/// Chosen over a hash crate because it is eleven lines, allocation-free and
/// identical on every host.
pub fn head_hash(head: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in head.iter().take(HEAD_BYTES) {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// A file's modification time in milliseconds since the Unix epoch.
///
/// A stamp the clock cannot express lands on `0` rather than wrapping, which
/// would sort a 1904 file after a 2024 one.
pub fn mtime_ms(modified: Option<SystemTime>) -> u64 {
    modified
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_the_reference_fnv1a_vector() {
        assert_eq!(head_hash(b""), 0x811c_9dc5);
        assert_eq!(head_hash(b"a"), 0xe40c_292c);
        assert_eq!(head_hash(b"foobar"), 0xbf9c_f968);
    }

    #[test]
    fn the_hash_separates_books_that_share_a_length() {
        let a = b"PDF-1.7 one book, same length as its neighbour";
        let mut b = a.to_vec();
        b[12] = b'X';
        assert_eq!(a.len(), b.len());
        assert_ne!(head_hash(a), head_hash(&b));
    }

    #[test]
    fn only_the_head_is_read() {
        let mut long = vec![0u8; HEAD_BYTES + 64];
        let head_only = long[..HEAD_BYTES].to_vec();
        for slot in &mut long[HEAD_BYTES..] {
            *slot = 0xff;
        }
        assert_eq!(head_hash(&long), head_hash(&head_only));
    }

    #[test]
    fn an_unreadable_stamp_is_zero_not_a_wrap() {
        assert_eq!(mtime_ms(None), 0);
        assert_eq!(mtime_ms(Some(UNIX_EPOCH)), 0);
        assert_eq!(mtime_ms(Some(UNIX_EPOCH - std::time::Duration::from_secs(5))), 0);
        assert_eq!(
            mtime_ms(Some(UNIX_EPOCH + std::time::Duration::from_millis(1500))),
            1500
        );
    }
}
