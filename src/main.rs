use crate_names::{CrateNames, DESCRIPTIONS_FILE_V2, NAMES_FILE_V2, build_from_dump};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
usage:
  crate-names build [--input <db-dump.tar.gz>] [--out <dir>]
      build artifacts from a crates.io database dump (stdin if no --input)
  crate-names typeahead <prefix> [--names <names-v2.tsv.zst>] [--limit <n>]
      query a names artifact (smoke test)";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("build") => build(args),
        Some("typeahead") => typeahead(args),
        _ => Err(USAGE.into()),
    }
}

fn flag(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{name} requires a value"))
}

fn build(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut input = None;
    let mut out = PathBuf::from(".");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => input = Some(PathBuf::from(flag(&mut args, "--input")?)),
            "--out" => out = PathBuf::from(flag(&mut args, "--out")?),
            other => return Err(format!("unexpected argument {other}\n{USAGE}")),
        }
    }

    let reader: Box<dyn Read> = match &input {
        Some(path) => {
            Box::new(fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?)
        }
        None => Box::new(std::io::stdin().lock()),
    };

    let output = build_from_dump(reader).map_err(|e| e.to_string())?;
    eprintln!(
        "{} crates ({} skipped without a resolvable version)",
        output.crate_count, output.skipped_no_version
    );

    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    for (file, bytes) in [
        (NAMES_FILE_V2, output.names_zst()),
        (DESCRIPTIONS_FILE_V2, output.descriptions_zst()),
    ] {
        let bytes = bytes.map_err(|e| format!("compressing {file}: {e}"))?;
        let path = out.join(file);
        fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        eprintln!("{} ({} bytes)", path.display(), bytes.len());
    }
    Ok(())
}

fn typeahead(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut prefix = None;
    let mut names_path = PathBuf::from(NAMES_FILE_V2);
    let mut limit = 10;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--names" => names_path = PathBuf::from(flag(&mut args, "--names")?),
            "--limit" => {
                limit = flag(&mut args, "--limit")?
                    .parse()
                    .map_err(|e| format!("--limit: {e}"))?
            }
            other if prefix.is_none() => prefix = Some(other.to_owned()),
            other => return Err(format!("unexpected argument {other}\n{USAGE}")),
        }
    }
    let prefix = prefix.ok_or(USAGE)?;

    let bytes = fs::read(&names_path).map_err(|e| format!("{}: {e}", names_path.display()))?;
    let names = CrateNames::from_zstd(&bytes).map_err(|e| e.to_string())?;
    for entry in names.typeahead(&prefix, limit) {
        println!("{}\t{}\t{}", entry.name, entry.version, entry.rank);
    }
    Ok(())
}
