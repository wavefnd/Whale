#![cfg(feature = "asm-amd64")]

#[path = "../object/tests/common/mod.rs"]
mod elf;

use std::{fs, process::Command};

#[test]
fn cli_bridge_preserves_metadata_and_legacy_labels() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("metadata.asm");
    let output = dir.path().join("metadata.o");
    fs::write(&input, include_str!("fixtures/symbol-metadata.asm")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["asm", "--amd64"])
        .arg(input)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let bytes = fs::read(output).unwrap();
    let secs = elf::sections(&bytes);
    let symbols = elf::symbols(&bytes, &secs);
    for (name, kind, size, visibility, section) in [
        ("entry", 2, 2, 2, 1),
        ("entry.local", 0, 1, 0, 1),
        ("payload", 1, 4, 2, 2),
        ("imported", 2, 0, 0, 0),
    ] {
        let symbol = symbols.iter().find(|s| s.name == name).unwrap();
        assert_eq!(
            (symbol.kind, symbol.size, symbol.visibility, symbol.section),
            (kind, size, visibility, section)
        );
    }
}

#[test]
#[ignore = "optional independent GNU readelf fixture; run with --ignored --nocapture"]
fn readelf_fixture_confirms_symbol_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("metadata.asm");
    let output = dir.path().join("metadata.o");
    fs::write(&input, include_str!("fixtures/symbol-metadata.asm")).unwrap();
    assert!(Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["asm", "--amd64"])
        .arg(input)
        .arg("-o")
        .arg(&output)
        .status()
        .unwrap()
        .success());
    let result = Command::new("readelf")
        .args(["--symbols", "--wide"])
        .arg(output)
        .output()
        .expect("install GNU binutils readelf for this opt-in check");
    assert!(result.status.success());
    let text = String::from_utf8(result.stdout).unwrap();
    for (name, kind, visibility) in [
        ("entry", "FUNC", "HIDDEN"),
        ("payload", "OBJECT", "HIDDEN"),
        ("imported", "FUNC", "DEFAULT"),
    ] {
        let line = text.lines().find(|l| l.split_whitespace().last() == Some(name)).unwrap();
        assert!(line.contains(kind) && line.contains(visibility), "{line}");
    }
    println!("{text}");
}
