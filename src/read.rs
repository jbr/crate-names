//! Sans-io readers for the published artifacts.

use crate::format::{folded_cmp, folded_cmp_key, folded_starts_with, normalize};
use std::fmt;
use std::ops::Range;

/// Error returned when parsing an artifact.
#[derive(Debug)]
pub enum Error {
    /// zstd decompression failed
    Zstd(std::io::Error),
    /// decompressed artifact was not valid utf8
    Utf8(std::string::FromUtf8Error),
    /// artifact exceeds the 4GiB this reader supports
    TooLarge,
    /// lines were not sorted (or not unique) by crate name
    Unsorted {
        /// 0-indexed line number
        line: usize,
    },
    /// a line did not have the expected fields
    Malformed {
        /// 0-indexed line number
        line: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Zstd(e) => write!(f, "zstd decompression failed: {e}"),
            Error::Utf8(e) => write!(f, "artifact was not valid utf8: {e}"),
            Error::TooLarge => write!(f, "artifact exceeds supported size"),
            Error::Unsorted { line } => write!(f, "artifact not sorted by name at line {line}"),
            Error::Malformed { line } => write!(f, "malformed artifact line {line}"),
        }
    }
}

impl std::error::Error for Error {}

/// A single crate in the names artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    /// the crate's name
    pub name: &'a str,
    /// the crates.io "default version": the version the crates.io ui
    /// itself presents, typically the highest stable non-yanked release
    pub version: &'a str,
    /// log-quantized all-time downloads; see [`crate::rank_from_downloads`]
    pub rank: u8,
}

/// The decompressed text of an artifact plus line offsets. TSV sorted by
/// folded name needs no further indexing: prefix queries are two binary
/// searches.
struct SortedTsv {
    text: String,
    /// byte offset of each line start, plus a sentinel at `text.len()`
    offsets: Vec<u32>,
}

impl SortedTsv {
    fn from_zstd(bytes: &[u8]) -> Result<Self, Error> {
        let decompressed = zstd::decode_all(bytes).map_err(Error::Zstd)?;
        Self::from_text(String::from_utf8(decompressed).map_err(Error::Utf8)?)
    }

    fn from_text(text: String) -> Result<Self, Error> {
        if text.len() > u32::MAX as usize {
            return Err(Error::TooLarge);
        }
        let mut offsets = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                offsets.push(index as u32 + 1);
            }
        }
        if *offsets.last().unwrap() as usize != text.len() {
            offsets.push(text.len() as u32);
        } else {
            // trailing newline: the sentinel is already in place
        }
        let tsv = Self { text, offsets };
        for line in 1..tsv.len() {
            if folded_cmp(tsv.name(line - 1), tsv.name(line)).is_ge() {
                return Err(Error::Unsorted { line });
            }
        }
        Ok(tsv)
    }

    fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    fn line(&self, index: usize) -> &str {
        let start = self.offsets[index] as usize;
        let end = self.offsets[index + 1] as usize;
        self.text[start..end].trim_end_matches('\n')
    }

    fn name(&self, index: usize) -> &str {
        let line = self.line(index);
        line.split('\t').next().unwrap_or(line)
    }

    /// index of the first line for which `pred(name)` is false. `pred`
    /// must be monotonic (true for a prefix of lines) over the sorted names.
    fn partition_point(&self, pred: impl Fn(&str) -> bool) -> usize {
        let (mut low, mut high) = (0, self.len());
        while low < high {
            let mid = low + (high - low) / 2;
            if pred(self.name(mid)) {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        low
    }

    /// The lines whose folded name starts with the folded `prefix`. Folding
    /// the needle once here keeps every comparison in the binary search
    /// allocation-free.
    fn prefix_range(&self, prefix: &str) -> Range<usize> {
        let key = normalize(prefix);
        let start = self.partition_point(|name| folded_cmp_key(name, &key).is_lt());
        let end = self.partition_point(|name| {
            folded_cmp_key(name, &key).is_lt() || folded_starts_with(name, &key)
        });
        start..end
    }

    fn find(&self, name: &str) -> Option<usize> {
        let key = normalize(name);
        let index = self.partition_point(|candidate| folded_cmp_key(candidate, &key).is_lt());
        (index < self.len() && folded_cmp_key(self.name(index), &key).is_eq()).then_some(index)
    }
}

/// Reader for the names artifact (`names-v1.tsv.zst`): every crate name
/// with its default version and download rank.
pub struct CrateNames(SortedTsv);

impl fmt::Debug for CrateNames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CrateNames")
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

impl CrateNames {
    /// Parse a zstd-compressed names artifact.
    pub fn from_zstd(bytes: &[u8]) -> Result<Self, Error> {
        Self::validate(SortedTsv::from_zstd(bytes)?)
    }

    /// Parse an already-decompressed names artifact.
    pub fn from_tsv(text: String) -> Result<Self, Error> {
        Self::validate(SortedTsv::from_text(text)?)
    }

    fn validate(tsv: SortedTsv) -> Result<Self, Error> {
        let names = Self(tsv);
        for line in 0..names.len() {
            names.entry(line).ok_or(Error::Malformed { line })?;
        }
        Ok(names)
    }

    fn entry(&self, index: usize) -> Option<Entry<'_>> {
        let mut fields = self.0.line(index).split('\t');
        let name = fields.next()?;
        let version = fields.next()?;
        let rank = fields.next()?.parse().ok()?;
        fields.next().is_none().then_some(Entry {
            name,
            version,
            rank,
        })
    }

    /// Number of crates in the artifact.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the artifact contains no crates.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Name lookup, folded: case-insensitive, and `-` and `_` are the same
    /// character. `get("Tokio_Util")` finds `tokio-util`, because crates.io
    /// would not have let a second crate claim that name.
    pub fn get(&self, name: &str) -> Option<Entry<'_>> {
        self.entry(self.0.find(name)?)
    }

    /// The entry at line `index` (0-based, in the artifact's folded-name sort
    /// order), or `None` if out of range. Paired with [`len`](Self::len) this
    /// is the stable handle a caller needs to build a side index that refers
    /// back into the artifact by position rather than copying names.
    pub fn entry_at(&self, index: usize) -> Option<Entry<'_>> {
        (index < self.len()).then(|| self.entry(index)).flatten()
    }

    /// The half-open range of line indices whose folded name starts with
    /// `prefix` (folded as in [`get`](Self::get)) — the same range
    /// [`count`](Self::count) measures, exposed as positions so a caller can
    /// pair whole-name-prefix hits with [`entry_at`](Self::entry_at).
    pub fn prefix_indices(&self, prefix: &str) -> Range<usize> {
        self.0.prefix_range(prefix)
    }

    /// How many crate names start with `prefix`, folded as in [`get`](Self::get).
    /// Two binary searches — no enumeration — so this is as cheap for `"s"`
    /// as for `"trillium-"`.
    pub fn count(&self, prefix: &str) -> usize {
        self.0.prefix_range(prefix).len()
    }

    /// All crates whose name starts with `prefix`, folded as in
    /// [`get`](Self::get), in artifact order.
    pub fn prefix(&self, prefix: &str) -> impl Iterator<Item = Entry<'_>> {
        self.0
            .prefix_range(prefix)
            .map(|index| self.entry(index).expect("validated at construction"))
    }

    /// The `limit` highest-ranked crates whose name starts with `prefix`
    /// (folded as in [`get`](Self::get)), ties broken by name.
    pub fn typeahead(&self, prefix: &str, limit: usize) -> Vec<Entry<'_>> {
        let mut matches: Vec<Entry<'_>> = self.prefix(prefix).collect();
        matches.sort_unstable_by(|a, b| b.rank.cmp(&a.rank).then(a.name.cmp(b.name)));
        matches.truncate(limit);
        matches
    }
}

/// Reader for the descriptions artifact (`descriptions-v1.tsv.zst`).
pub struct Descriptions(SortedTsv);

impl fmt::Debug for Descriptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Descriptions")
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

impl Descriptions {
    /// Parse a zstd-compressed descriptions artifact.
    pub fn from_zstd(bytes: &[u8]) -> Result<Self, Error> {
        Ok(Self(SortedTsv::from_zstd(bytes)?))
    }

    /// Parse an already-decompressed descriptions artifact.
    pub fn from_tsv(text: String) -> Result<Self, Error> {
        Ok(Self(SortedTsv::from_text(text)?))
    }

    /// Number of crates in the artifact.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the artifact contains no crates.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The description for `name`, if that crate has one. Folded, as in
    /// [`CrateNames::get`].
    pub fn get(&self, name: &str) -> Option<&str> {
        let index = self.0.find(name)?;
        self.0.line(index).split_once('\t').map(|(_, desc)| desc)
    }

    /// All `(name, description)` pairs, in artifact order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        (0..self.len()).filter_map(|index| self.0.line(index).split_once('\t'))
    }
}
