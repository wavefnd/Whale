use amd64::AMD64;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn manifest(source: &str) -> Result<BTreeMap<&str, (&str, &str, &str)>, String> {
    let mut entries = BTreeMap::new();
    for line in source.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let columns: Vec<_> = line.split('\t').collect();
        if columns.len() != 4 || columns.iter().any(|c| c.is_empty()) {
            return Err("fixture manifest requires four nonempty columns".into());
        }
        let (path, outcome, diagnostic, purpose) = (columns[0], columns[1], columns[2], columns[3]);
        if Path::new(path).is_absolute()
            || path.split('/').any(|p| p == "..")
            || !path.ends_with(".asm")
        {
            return Err("invalid fixture path".into());
        }
        match outcome {
            "supported-positive" if diagnostic == "-" => {},
            "intended-negative" if diagnostic != "-" => {},
            "future-syntax" if diagnostic != "-" && purpose.contains('#') => {},
            _ => return Err("invalid fixture expectation or missing feature issue".into()),
        }
        if entries.insert(path, (outcome, diagnostic, purpose)).is_some() {
            return Err("duplicate fixture manifest entry".into());
        }
    }
    if entries.is_empty() {
        return Err("empty fixture manifest".into());
    }
    Ok(entries)
}

fn discover(root: &Path, at: &Path, out: &mut BTreeSet<String>) {
    for e in std::fs::read_dir(at).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            discover(root, &p, out);
        } else if p.extension().is_some_and(|e| e == "asm") {
            out.insert(p.strip_prefix(root).unwrap().to_str().unwrap().replace('\\', "/"));
        }
    }
}

#[test]
fn every_historical_fixture_has_an_executed_classification() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test");
    let source =
        std::fs::read_to_string(root.join("fixtures.tsv")).expect("required fixture manifest");
    let entries = manifest(&source).unwrap();
    let mut files = BTreeSet::new();
    discover(&root, &root, &mut files);
    assert_eq!(
        files,
        entries.keys().map(|p| p.to_string()).collect(),
        "unclassified or missing assembly fixture"
    );
    for (path, (outcome, diagnostic, _)) in entries {
        let input = std::fs::read_to_string(root.join(path)).unwrap();
        let result = assembler::assemble(&input, &AMD64);
        match outcome {
            "supported-positive" => assert!(result.is_ok(), "{path}: {}", result.err().unwrap()),
            _ => assert!(
                result
                    .err()
                    .expect("negative/future fixture unexpectedly accepted")
                    .to_string()
                    .contains(diagnostic),
                "{path}"
            ),
        }
    }
}

#[test]
fn empty_duplicate_skipped_and_undefined_manifest_contracts_are_rejected() {
    for source in [
        "",
        "# header\n",
        "a.asm\tsupported-positive\t-",
        "a.asm\tunknown\t-\tpurpose",
        "a.asm\tintended-negative\t-\tpurpose",
        "a.asm\tfuture-syntax\terror\tno issue",
        "../a.asm\tsupported-positive\t-\tpurpose",
        "a.asm\tsupported-positive\t-\tpurpose\na.asm\tsupported-positive\t-\tpurpose",
    ] {
        assert!(manifest(source).is_err(), "{source}");
    }
}
