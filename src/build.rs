//! Build artifacts from a crates.io database dump tarball.

use crate::format::{ZSTD_LEVEL, flatten_whitespace, rank_from_downloads};
use std::collections::HashMap;
use std::fmt;
use std::io::Read;

/// Error returned when building artifacts from a dump.
#[derive(Debug)]
pub enum BuildError {
    /// io error reading the tarball
    Io(std::io::Error),
    /// a csv file in the dump could not be parsed
    Csv(csv::Error),
    /// an expected csv file was absent from the tarball
    MissingTable(&'static str),
    /// an expected column was absent from a csv header
    MissingColumn(&'static str, &'static str),
    /// a crate name contained bytes outside the crates.io charset,
    /// which would corrupt the line-oriented format
    InvalidName(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::Io(e) => write!(f, "io error reading dump: {e}"),
            BuildError::Csv(e) => write!(f, "csv error reading dump: {e}"),
            BuildError::MissingTable(t) => write!(f, "dump did not contain {t}"),
            BuildError::MissingColumn(t, c) => write!(f, "{t} did not contain column {c}"),
            BuildError::InvalidName(n) => write!(f, "unexpected crate name {n:?}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        BuildError::Io(e)
    }
}

impl From<csv::Error> for BuildError {
    fn from(e: csv::Error) -> Self {
        BuildError::Csv(e)
    }
}

/// The uncompressed artifacts plus build statistics.
#[derive(Debug)]
pub struct BuildOutput {
    /// the names artifact, uncompressed
    pub names_tsv: String,
    /// the descriptions artifact, uncompressed
    pub descriptions_tsv: String,
    /// crates included in the names artifact
    pub crate_count: usize,
    /// crates skipped because no default version could be resolved
    pub skipped_no_version: usize,
}

impl BuildOutput {
    /// The names artifact, compressed for publication.
    pub fn names_zst(&self) -> std::io::Result<Vec<u8>> {
        zstd::encode_all(self.names_tsv.as_bytes(), ZSTD_LEVEL)
    }

    /// The descriptions artifact, compressed for publication.
    pub fn descriptions_zst(&self) -> std::io::Result<Vec<u8>> {
        zstd::encode_all(self.descriptions_tsv.as_bytes(), ZSTD_LEVEL)
    }
}

/// Locates needed columns in a csv header by name, so we tolerate column
/// additions and reorderings in future dumps.
fn column_indexes<const N: usize>(
    table: &'static str,
    reader: &mut csv::Reader<impl Read>,
    columns: [&'static str; N],
) -> Result<[usize; N], BuildError> {
    let headers = reader.headers()?.clone();
    let mut indexes = [0; N];
    for (index, column) in columns.into_iter().enumerate() {
        indexes[index] = headers
            .iter()
            .position(|header| header == column)
            .ok_or(BuildError::MissingColumn(table, column))?;
    }
    Ok(indexes)
}

/// Stream a crates.io database dump tarball (`db-dump.tar.gz` as
/// downloaded, still gzipped) and produce the v1 artifacts.
///
/// Single pass; does not require seeking, so the tarball can be piped in
/// without touching disk. Buffers one version string per published
/// version, on the order of a few hundred MB for the 2026 registry.
pub fn build_from_dump(reader: impl Read) -> Result<BuildOutput, BuildError> {
    // crate_id -> all-time downloads
    let mut downloads: HashMap<u64, u64> = HashMap::new();
    // (name, crate_id, description)
    let mut crates: Vec<(String, u64, String)> = Vec::new();
    // version_id -> version string
    let mut version_num: HashMap<u64, String> = HashMap::new();
    // crate_id -> default version_id
    let mut default_version: HashMap<u64, u64> = HashMap::new();
    let mut seen = [None::<&'static str>; 4];

    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(reader));
    for entry in archive.entries()? {
        let entry = entry?;
        let path = entry.path()?;
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };

        match file_name.as_str() {
            "crate_downloads.csv" => {
                seen[0] = Some("crate_downloads.csv");
                let mut csv = csv::Reader::from_reader(entry);
                let [crate_id, downloads_col] =
                    column_indexes("crate_downloads.csv", &mut csv, ["crate_id", "downloads"])?;
                for record in csv.records() {
                    let record = record?;
                    if let (Some(id), Some(count)) = (
                        record.get(crate_id).and_then(|f| f.parse().ok()),
                        record.get(downloads_col).and_then(|f| f.parse().ok()),
                    ) {
                        downloads.insert(id, count);
                    }
                }
            }
            "crates.csv" => {
                seen[1] = Some("crates.csv");
                let mut csv = csv::Reader::from_reader(entry);
                let [id, name, description] =
                    column_indexes("crates.csv", &mut csv, ["id", "name", "description"])?;
                for record in csv.records() {
                    let record = record?;
                    let (Some(id), Some(name)) = (
                        record.get(id).and_then(|f| f.parse().ok()),
                        record.get(name),
                    ) else {
                        continue;
                    };
                    if !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    {
                        return Err(BuildError::InvalidName(name.to_owned()));
                    }
                    let description = record.get(description).unwrap_or_default();
                    crates.push((name.to_owned(), id, flatten_whitespace(description)));
                }
            }
            "versions.csv" => {
                seen[2] = Some("versions.csv");
                let mut csv = csv::Reader::from_reader(entry);
                let [id, num] = column_indexes("versions.csv", &mut csv, ["id", "num"])?;
                for record in csv.records() {
                    let record = record?;
                    if let (Some(id), Some(num)) =
                        (record.get(id).and_then(|f| f.parse().ok()), record.get(num))
                    {
                        version_num.insert(id, num.to_owned());
                    }
                }
            }
            "default_versions.csv" => {
                seen[3] = Some("default_versions.csv");
                let mut csv = csv::Reader::from_reader(entry);
                let [crate_id, version_id] =
                    column_indexes("default_versions.csv", &mut csv, ["crate_id", "version_id"])?;
                for record in csv.records() {
                    let record = record?;
                    if let (Some(crate_id), Some(version_id)) = (
                        record.get(crate_id).and_then(|f| f.parse().ok()),
                        record.get(version_id).and_then(|f| f.parse().ok()),
                    ) {
                        default_version.insert(crate_id, version_id);
                    }
                }
            }
            _ => {}
        }
    }

    for (index, table) in [
        "crate_downloads.csv",
        "crates.csv",
        "versions.csv",
        "default_versions.csv",
    ]
    .into_iter()
    .enumerate()
    {
        if seen[index].is_none() {
            return Err(BuildError::MissingTable(table));
        }
    }

    crates.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    let mut names_tsv = String::new();
    let mut descriptions_tsv = String::new();
    let mut skipped_no_version = 0;
    let mut crate_count = 0;
    for (name, id, description) in &crates {
        let Some(version) = default_version.get(id).and_then(|vid| version_num.get(vid)) else {
            skipped_no_version += 1;
            continue;
        };
        let rank = rank_from_downloads(downloads.get(id).copied().unwrap_or_default());
        names_tsv.push_str(&format!("{name}\t{version}\t{rank}\n"));
        if !description.is_empty() {
            descriptions_tsv.push_str(&format!("{name}\t{description}\n"));
        }
        crate_count += 1;
    }

    Ok(BuildOutput {
        names_tsv,
        descriptions_tsv,
        crate_count,
        skipped_no_version,
    })
}
