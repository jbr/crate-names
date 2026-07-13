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
//! # Wire format (v2)
//!
//! Artifacts are zstd-compressed TSV, one crate per line, sorted by the
//! crate's *folded* name: ASCII-lowercased, with `-` and `_` treated as the
//! same character (see [`normalize`]). Names are stored as spelled; only the
//! ordering is folded. Crate names cannot contain tabs or newlines, and
//! description whitespace is flattened, so no escaping is required.
//!
//! Folding is what makes lookups work the way people type. crates.io will
//! not let a new crate take a name that folds onto an existing one, so the
//! folded key is unique across the registry, and the artifacts stay sorted
//! and unique under it — queries remain two binary searches, and `Tokio`,
//! `tokio` and `tokio_util` all find what you meant.
//!
//! - `names-v2.tsv.zst`: `name \t default_version \t rank` for every crate.
//!   `rank` is a log-quantized download count in `0..=255`; see
//!   [`rank_from_downloads`]. Ordering by rank is meaningful, arithmetic
//!   on it is not.
//! - `descriptions-v2.tsv.zst`: `name \t description` for every crate with
//!   a non-empty description, whitespace runs collapsed to single spaces.
//!
//! # Getting the artifacts
//!
//! Both are republished daily (built from that morning's crates.io database
//! dump) to a rolling GitHub release, at stable URLs also exposed as
//! [`NAMES_URL_V2`] and [`DESCRIPTIONS_URL_V2`]:
//!
//! - <https://github.com/jbr/crate-names/releases/download/artifacts/names-v2.tsv.zst>
//!   (~2 MB)
//! - <https://github.com/jbr/crate-names/releases/download/artifacts/descriptions-v2.tsv.zst>
//!   (~5.5 MB)
//!
//! The URLs redirect to the release asset, so follow redirects. Assets carry
//! ETags: revalidate with `If-None-Match` rather than re-downloading — the
//! content changes at most once a day.
//!
//! ```no_run
//! # fn fetch(url: &str) -> Vec<u8> { unimplemented!() }
//! let bytes = fetch(crate_names::NAMES_URL_V2);
//! let names = crate_names::CrateNames::from_zstd(&bytes)?;
//! let top_ten = names.typeahead("serd", 10);
//! # Ok::<(), crate_names::Error>(())
//! ```
#![forbid(unsafe_code)]
#![deny(
    clippy::dbg_macro,
    missing_copy_implementations,
    rustdoc::missing_crate_level_docs,
    missing_debug_implementations,
    missing_docs,
    nonstandard_style,
    unused_qualifications
)]

// Compile the README as a doctest so its examples stay in sync with the crate.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme {}

mod format;
mod read;

pub use format::{
    DESCRIPTIONS_FILE_V2, DESCRIPTIONS_URL_V2, NAMES_FILE_V2, NAMES_URL_V2, normalize,
    rank_from_downloads,
};
pub use read::{CrateNames, Descriptions, Entry, Error};

#[cfg(feature = "build")]
mod build;
#[cfg(feature = "build")]
pub use build::{BuildError, BuildOutput, build_from_dump};
