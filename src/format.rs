//! Constants and pure functions that define wire format v2. Builder and
//! reader both depend on this module, so the format has a single source
//! of truth.

use std::cmp::Ordering;

/// Artifact file name for the names/versions/ranks table.
pub const NAMES_FILE_V2: &str = "names-v2.tsv.zst";

/// Artifact file name for the descriptions table.
pub const DESCRIPTIONS_FILE_V2: &str = "descriptions-v2.tsv.zst";

/// Canonical public URL of the names artifact, republished daily by this
/// repository's scheduled workflow. Redirects (GitHub release asset), so
/// fetch with redirect-following enabled.
pub const NAMES_URL_V2: &str =
    "https://github.com/jbr/crate-names/releases/download/artifacts/names-v2.tsv.zst";

/// Canonical public URL of the descriptions artifact; see [`NAMES_URL_V2`].
pub const DESCRIPTIONS_URL_V2: &str =
    "https://github.com/jbr/crate-names/releases/download/artifacts/descriptions-v2.tsv.zst";

/// Fold one byte of a crate name the way crates.io folds names when it
/// decides whether two of them collide: ASCII-case-insensitively, with `-`
/// and `_` equivalent.
const fn fold(byte: u8) -> u8 {
    match byte {
        b'_' => b'-',
        other => other.to_ascii_lowercase(),
    }
}

/// The key a crate name is stored and searched under: ASCII-lowercased,
/// with `-` and `_` folded together — the same way crates.io decides
/// whether two names collide.
///
/// crates.io rejects a new crate whose folded name matches an existing one,
/// so this key is unique across the registry — which is what lets the
/// artifacts be sorted by it and still be searched with a binary search.
/// Names keep their original spelling in the artifact; only the *ordering*
/// and the comparisons are folded.
pub fn normalize(name: &str) -> String {
    name.bytes().map(fold).map(char::from).collect()
}

/// Order two crate names by their folded keys, without allocating.
pub(crate) fn folded_cmp(left: &str, right: &str) -> Ordering {
    left.bytes().map(fold).cmp(right.bytes().map(fold))
}

/// Order a crate name against an already-folded needle, without allocating.
pub(crate) fn folded_cmp_key(name: &str, key: &str) -> Ordering {
    name.bytes().map(fold).cmp(key.bytes())
}

/// Whether `name`'s folded key begins with the already-folded `key`.
pub(crate) fn folded_starts_with(name: &str, key: &str) -> bool {
    name.len() >= key.len()
        && name
            .bytes()
            .map(fold)
            .zip(key.bytes())
            .all(|(name_byte, key_byte)| name_byte == key_byte)
}

/// Compression level used when producing artifacts.
#[cfg(feature = "build")]
pub(crate) const ZSTD_LEVEL: i32 = 19;

/// Quantize an all-time download count into a rank in `0..=255`.
///
/// Eight sub-buckets per doubling of downloads (`floor(8·log₂(n+1))`),
/// which preserves ordering at every scale while compressing to almost
/// nothing. Saturates around 4×10⁹ downloads, comfortably above the most
/// downloaded crate.
pub fn rank_from_downloads(downloads: u64) -> u8 {
    let rank = (8.0 * (downloads.saturating_add(1) as f64).log2()).floor();
    if rank >= 255.0 { 255 } else { rank as u8 }
}

/// Collapse all whitespace runs (including newlines and tabs) to single
/// spaces so descriptions fit in one TSV field.
#[cfg(feature = "build")]
pub(crate) fn flatten_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_is_monotonic_and_bounded() {
        assert_eq!(rank_from_downloads(0), 0);
        let mut prev = 0;
        for downloads in [1, 5, 100, 1_000, 1_000_000, 500_000_000, u64::MAX] {
            let rank = rank_from_downloads(downloads);
            assert!(rank >= prev, "rank must not decrease");
            prev = rank;
        }
        assert_eq!(rank_from_downloads(u64::MAX), 255);
        // half a billion downloads (serde territory) is still well under saturation
        assert!(rank_from_downloads(500_000_000) < 255);
    }
}
