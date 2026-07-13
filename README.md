# crate-names

[![ci][ci-badge]][ci]
[![crates.io version][version-badge]][crate]
[![docs.rs][docs-badge]][docs]
[![codecov][codecov-badge]][codecov]

[ci]: https://github.com/jbr/crate-names/actions?query=workflow%3ACI
[ci-badge]: https://github.com/jbr/crate-names/workflows/CI/badge.svg
[version-badge]: https://img.shields.io/crates/v/crate-names.svg?style=flat-square
[crate]: https://crates.io/crates/crate-names
[docs-badge]: https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square
[docs]: https://docs.rs/crate-names
[codecov-badge]: https://codecov.io/gh/jbr/crate-names/graph/badge.svg
[codecov]: https://codecov.io/gh/jbr/crate-names

Compact, transparent artifacts of every crate name on crates.io, rebuilt
daily from the [official database dump], for typeahead and default-version
lookup. Plus a small sans-io Rust library for reading them.

[official database dump]: https://crates.io/data-access#database-dumps

## The artifacts

Published daily to the rolling [`artifacts` release]:

| file | ~size | contents |
|---|---|---|
| `names-v1.tsv.zst` | 1.8 MB | `name \t default_version \t rank` for every crate |
| `descriptions-v1.tsv.zst` | 5.5 MB | `name \t description` for every crate with one |

[`artifacts` release]: https://github.com/jbr/crate-names/releases/tag/artifacts

Both are zstd-compressed TSV, one crate per line, sorted byte-wise by
name, guaranteed sorted and (for names) three-fields-per-line by the
builder. Because crate names cannot contain tabs or newlines, and
description whitespace is flattened to single spaces, there is no
escaping: the format is fully processable with standard tools.

```console
$ curl -sL https://github.com/jbr/crate-names/releases/download/artifacts/names-v1.tsv.zst \
    | zstd -d | grep '^serde\b'
serde	1.0.228	240
```

- `default_version` is crates.io's own notion of the version to present,
  from the dump's `default_versions` table.
- `rank` is the all-time download count quantized to `floor(8·log₂(n+1))`,
  saturating at 255. Ordering is meaningful; arithmetic is not.
- Freshness: artifacts lag crates.io by up to ~27 hours (daily dump +
  daily build). Consumers that need same-day version resolution should
  fall through to the [sparse index](https://index.crates.io) per crate.

## Example

The reader is sans-io: fetch the artifact however you like and hand over
the bytes. A sorted list needs no materialized index — construction
records line offsets and validates sortedness, and queries are binary
searches over the decompressed text.

```rust
use crate_names::{CrateNames, Error};

fn typeahead_demo(artifact: &[u8]) -> Result<(), Error> {
    let names = CrateNames::from_zstd(artifact)?;

    // top crates by rank for a prefix
    for entry in names.typeahead("serd", 10) {
        println!("{}\t{}", entry.name, entry.version);
    }

    // exact lookup
    if let Some(serde) = names.get("serde") {
        println!("serde's default version is {}", serde.version);
    }

    Ok(())
}
```

The `build` feature (not needed by consumers) enables `build_from_dump`,
which streams a `db-dump.tar.gz` and produces the artifacts; the daily
[workflow](.github/workflows/artifacts.yml) is a thin wrapper around it:

```console
$ curl -sL https://static.crates.io/db-dump.tar.gz | crate-names build --out out
```

## Versioning

The wire format and the library are versioned independently: the `-v1`
in the file names is the format contract, while the crate follows semver
as usual. If the format ever changes incompatibly it will be published
as new `-v2` files alongside the `-v1` ones, so existing consumers keep
working.

## Safety

This crate uses `#![forbid(unsafe_code)]`.

## License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

<br/>

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
</sub>
