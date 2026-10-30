#![cfg(feature = "asm-amd64")]

use std::{fs, process::Command};

#[test]
fn cli_reports_real_filename_both_symbol_sites_and_preserves_existing_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source-한글.asm");
    let output = dir.path().join("output.o");
    fs::write(&input, "; first\nfoo:nop\n\n; other\nfoo:ret\n").unwrap();
    fs::write(&output, b"existing artifact").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["asm", "--amd64"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(error.contains(input.to_str().unwrap()), "{error}");
    assert!(error.contains("line 5, column 1"), "{error}");
    assert!(
        error.contains("previous declaration") && error.contains("line 2, column 1"),
        "{error}"
    );
    assert!(error.contains("bytes"), "{error}");
    assert_eq!(fs::read(&output).unwrap(), b"existing artifact");
}
