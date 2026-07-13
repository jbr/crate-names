//! Constants and pure functions that define wire format v1. Builder and
//! reader both depend on this module, so the format has a single source
//! of truth.

/// Artifact file name for the names/versions/ranks table.
pub const NAMES_FILE_V1: &str = "names-v1.tsv.zst";

/// Artifact file name for the descriptions table.
pub const DESCRIPTIONS_FILE_V1: &str = "descriptions-v1.tsv.zst";

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
