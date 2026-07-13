//! Compact, transparent artifacts of every crate name on crates.io, for
//! typeahead and default-version lookup.
//!
//! This crate has two halves:
//!
//! - **Reading** (always available): [`CrateNames`] and [`Descriptions`]
//!   parse the published artifacts. The reader is sans-io: hand it bytes
//!   you fetched however you like.
//! - **Building** (behind the `build` feature): [`build_from_dump`] streams
//!   a crates.io database dump tarball and produces the artifacts. Used by
//!   the scheduled GitHub Action in this repository; consumers normally
//!   never need it.
//!
//! # Wire format (v1)
//!
//! Artifacts are zstd-compressed TSV, sorted byte-wise by crate name, one
//! crate per line. Crate names cannot contain tabs or newlines, and
//! description whitespace is flattened, so no escaping is required.
//!
//! - `names-v1.tsv.zst`: `name \t default_version \t rank` for every crate.
//!   `rank` is a log-quantized download count in `0..=255`; see
//!   [`rank_from_downloads`]. Ordering by rank is meaningful, arithmetic
//!   on it is not.
//! - `descriptions-v1.tsv.zst`: `name \t description` for every crate with
//!   a non-empty description, whitespace runs collapsed to single spaces.

mod format;
mod read;

pub use format::{DESCRIPTIONS_FILE_V1, NAMES_FILE_V1, rank_from_downloads};
pub use read::{CrateNames, Descriptions, Entry, Error};

#[cfg(feature = "build")]
mod build;
#[cfg(feature = "build")]
pub use build::{BuildError, BuildOutput, build_from_dump};
