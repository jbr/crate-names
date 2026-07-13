# crate-names

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

## The library

```rust
let names = crate_names::CrateNames::from_zstd(&bytes)?;

names.typeahead("serd", 10);   // top crates by rank for a prefix
names.get("serde");            // exact lookup: version + rank
names.prefix("serde");         // all matches, in name order
```

The reader is sans-io: fetch the artifact however you like and hand over
the bytes. A sorted list needs no materialized index — construction
records line offsets and validates sortedness, and queries are binary
searches over the decompressed text.

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
