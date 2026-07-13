//! Build artifacts from a synthetic dump tarball and read them back.
#![cfg(feature = "build")]

use crate_names::{CrateNames, Descriptions, Error, build_from_dump, rank_from_downloads};
use flate2::{Compression, write::GzEncoder};

/// Construct an in-memory db-dump.tar.gz with the four tables the builder
/// consumes, exercising header-order independence and extra columns.
fn synthetic_dump() -> Vec<u8> {
    let files = [
        (
            "2026-01-01-000000/data/crate_downloads.csv",
            "crate_id,downloads\n1,1000000\n2,50\n3,999999999\n4,0\n5,2000\n",
        ),
        (
            // extra columns and reordered relative to the real dump are fine
            "2026-01-01-000000/data/crates.csv",
            concat!(
                "created_at,description,id,name,readme\n",
                "2020,\"Serialization framework\",1,serde,ignored\n",
                "2021,\"A web server\",2,server-thing,ignored\n",
                "2022,\"Multi\nline\t desc\",3,serve,ignored\n",
                "2023,,4,zzz-no-description,ignored\n",
                // uppercase and an underscore: byte order would sort this
                // before every lowercase name, folded order puts it where a
                // reader looking for "serde-json" would expect it
                "2024,\"Json\",5,SERDE_json,ignored\n",
            ),
        ),
        (
            "2026-01-01-000000/data/versions.csv",
            "crate_id,id,num,yanked\n1,10,1.0.219,f\n1,11,0.9.0,f\n2,20,0.2.1,f\n3,30,3.1.4,f\n4,40,0.0.1,f\n5,50,1.1.1,f\n",
        ),
        (
            "2026-01-01-000000/data/default_versions.csv",
            "crate_id,num_versions,version_id\n1,2,10\n2,1,20\n3,1,30\n4,1,40\n5,1,50\n",
        ),
        ("2026-01-01-000000/data/unrelated.csv", "a,b\n1,2\n"),
    ];

    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for (path, contents) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, path, contents.as_bytes())
            .unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

#[test]
fn roundtrip() {
    let output = build_from_dump(&synthetic_dump()[..]).unwrap();
    assert_eq!(output.crate_count, 5);
    assert_eq!(output.skipped_no_version, 0);

    let names = CrateNames::from_zstd(&output.names_zst().unwrap()).unwrap();
    assert_eq!(names.len(), 5);

    let serde = names.get("serde").unwrap();
    assert_eq!(serde.version, "1.0.219");
    assert_eq!(serde.rank, rank_from_downloads(1_000_000));
    assert!(names.get("nonexistent").is_none());

    // prefix iteration is in folded order, so SERDE_json sits between serde and
    // serve rather than ahead of every lowercase name
    let prefixed: Vec<&str> = names.prefix("ser").map(|e| e.name).collect();
    assert_eq!(prefixed, ["serde", "SERDE_json", "serve", "server-thing"]);
    assert_eq!(names.prefix("serde").count(), 2);
    assert_eq!(names.prefix("q").count(), 0);
    assert_eq!(names.prefix("").count(), 5);

    // count agrees with prefix() without enumerating
    assert_eq!(names.count("ser"), 4);
    assert_eq!(names.count("serde"), 2);
    assert_eq!(names.count("q"), 0);
    assert_eq!(names.count(""), 5);

    // typeahead is ordered by rank: serve (999M) > serde (1M) > server-thing (50)
    let typeahead: Vec<&str> = names.typeahead("ser", 2).iter().map(|e| e.name).collect();
    assert_eq!(typeahead, ["serve", "serde"]);

    let descriptions = Descriptions::from_zstd(&output.descriptions_zst().unwrap()).unwrap();
    // zzz-no-description is omitted
    assert_eq!(descriptions.len(), 4);
    assert_eq!(
        descriptions.get("serde").unwrap(),
        "Serialization framework"
    );
    // whitespace was flattened
    assert_eq!(descriptions.get("serve").unwrap(), "Multi line desc");
    assert!(descriptions.get("zzz-no-description").is_none());
}

/// The point of v2: a name is found however the user spelled it, and the
/// entry still reports the crate's real spelling.
#[test]
fn lookups_fold_case_and_separators() {
    let output = build_from_dump(&synthetic_dump()[..]).unwrap();
    let names = CrateNames::from_zstd(&output.names_zst().unwrap()).unwrap();

    // every spelling of SERDE_json finds it, and it keeps its own
    for spelling in ["SERDE_json", "serde_json", "serde-json", "SeRdE-JsOn"] {
        let entry = names.get(spelling).unwrap_or_else(|| panic!("{spelling}"));
        assert_eq!(entry.name, "SERDE_json");
        assert_eq!(entry.version, "1.1.1");
    }

    // and so does a prefix of any spelling — this is what mobile
    // auto-capitalization was breaking
    for prefix in ["Serde", "SERDE", "serde"] {
        let found: Vec<&str> = names.typeahead(prefix, 10).iter().map(|e| e.name).collect();
        assert_eq!(found, ["serde", "SERDE_json"], "{prefix}");
    }
    assert_eq!(names.count("serde_"), 1);
    assert_eq!(names.count("SERDE-"), 1);

    let descriptions = Descriptions::from_zstd(&output.descriptions_zst().unwrap()).unwrap();
    assert_eq!(descriptions.get("serde-json").unwrap(), "Json");
}

#[test]
fn unsorted_artifacts_are_rejected() {
    let err = CrateNames::from_tsv("b\t1.0.0\t5\na\t1.0.0\t5\n".into()).unwrap_err();
    assert!(matches!(err, Error::Unsorted { line: 1 }));

    let err = CrateNames::from_tsv("a\t1.0.0\t5\na\t1.0.0\t5\n".into()).unwrap_err();
    assert!(matches!(err, Error::Unsorted { line: 1 }));

    // byte-sorted (v1) artifacts are not folded-sorted: an old artifact is
    // rejected rather than silently mis-searched
    let err = CrateNames::from_tsv("Zed\t1.0.0\t5\nabc\t1.0.0\t5\n".into()).unwrap_err();
    assert!(matches!(err, Error::Unsorted { line: 1 }));

    // names that differ only by fold are duplicates, not an ordering
    let err = CrateNames::from_tsv("a_b\t1.0.0\t5\nA-B\t1.0.0\t5\n".into()).unwrap_err();
    assert!(matches!(err, Error::Unsorted { line: 1 }));
}

#[test]
fn malformed_artifacts_are_rejected() {
    let err = CrateNames::from_tsv("a\t1.0.0\n".into()).unwrap_err();
    assert!(matches!(err, Error::Malformed { line: 0 }));

    let err = CrateNames::from_tsv("a\t1.0.0\tnot-a-rank\n".into()).unwrap_err();
    assert!(matches!(err, Error::Malformed { line: 0 }));
}
