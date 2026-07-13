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
            "crate_id,downloads\n1,1000000\n2,50\n3,999999999\n4,0\n",
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
            ),
        ),
        (
            "2026-01-01-000000/data/versions.csv",
            "crate_id,id,num,yanked\n1,10,1.0.219,f\n1,11,0.9.0,f\n2,20,0.2.1,f\n3,30,3.1.4,f\n4,40,0.0.1,f\n",
        ),
        (
            "2026-01-01-000000/data/default_versions.csv",
            "crate_id,num_versions,version_id\n1,2,10\n2,1,20\n3,1,30\n4,1,40\n",
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
    assert_eq!(output.crate_count, 4);
    assert_eq!(output.skipped_no_version, 0);

    let names = CrateNames::from_zstd(&output.names_zst().unwrap()).unwrap();
    assert_eq!(names.len(), 4);

    let serde = names.get("serde").unwrap();
    assert_eq!(serde.version, "1.0.219");
    assert_eq!(serde.rank, rank_from_downloads(1_000_000));
    assert!(names.get("nonexistent").is_none());

    // prefix iteration is in byte order
    let prefixed: Vec<&str> = names.prefix("ser").map(|e| e.name).collect();
    assert_eq!(prefixed, ["serde", "serve", "server-thing"]);
    assert_eq!(names.prefix("serde").count(), 1);
    assert_eq!(names.prefix("q").count(), 0);
    assert_eq!(names.prefix("").count(), 4);

    // typeahead is ordered by rank: serve (999M) > serde (1M) > server-thing (50)
    let typeahead: Vec<&str> = names.typeahead("ser", 2).iter().map(|e| e.name).collect();
    assert_eq!(typeahead, ["serve", "serde"]);

    let descriptions = Descriptions::from_zstd(&output.descriptions_zst().unwrap()).unwrap();
    // zzz-no-description is omitted
    assert_eq!(descriptions.len(), 3);
    assert_eq!(
        descriptions.get("serde").unwrap(),
        "Serialization framework"
    );
    // whitespace was flattened
    assert_eq!(descriptions.get("serve").unwrap(), "Multi line desc");
    assert!(descriptions.get("zzz-no-description").is_none());
}

#[test]
fn unsorted_artifacts_are_rejected() {
    let err = CrateNames::from_tsv("b\t1.0.0\t5\na\t1.0.0\t5\n".into()).unwrap_err();
    assert!(matches!(err, Error::Unsorted { line: 1 }));

    let err = CrateNames::from_tsv("a\t1.0.0\t5\na\t1.0.0\t5\n".into()).unwrap_err();
    assert!(matches!(err, Error::Unsorted { line: 1 }));
}

#[test]
fn malformed_artifacts_are_rejected() {
    let err = CrateNames::from_tsv("a\t1.0.0\n".into()).unwrap_err();
    assert!(matches!(err, Error::Malformed { line: 0 }));

    let err = CrateNames::from_tsv("a\t1.0.0\tnot-a-rank\n".into()).unwrap_err();
    assert!(matches!(err, Error::Malformed { line: 0 }));
}
