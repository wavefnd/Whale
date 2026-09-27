#![cfg(feature = "socket-cli")]

use std::{fs, process::Command};

#[test]
fn unsupported_targets_fail_before_reading_input_or_replacing_output() {
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("missing.json");
    let output = scratch.path().join("output.wir");
    fs::write(&output, b"previous output").unwrap();
    for valid_input in [false, true] {
        if valid_input {
            fs::write(&input, br#"{"format_version":2,"semantics_version":1,"features":[],"program":{"declarations":[],"globals":[],"functions":[]}}"#).unwrap();
        }
        for target in [
            "banana",
            "aarch64-whale-linux",
            "x86_64-whale-windows",
            "x86_64-unknown-linux-gnu",
            "",
        ] {
            let result = Command::new(env!("CARGO_BIN_EXE_whale"))
                .args(["ir", "lower"])
                .arg(&input)
                .args(["--target", target, "--no-verify", "-o"])
                .arg(&output)
                .output()
                .unwrap();
            assert!(!result.status.success());
            let error = String::from_utf8_lossy(&result.stderr);
            assert!(error.contains("unsupported target"), "{error}");
            assert!(error.contains("x86_64-whale-linux"), "{error}");
            assert_eq!(fs::read(&output).unwrap(), b"previous output");
        }
    }
}

#[test]
fn explicit_and_default_targets_print_the_same_output_layout() {
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("input.json");
    fs::write(&input, br#"{"format_version":2,"semantics_version":1,"features":[],"program":{"declarations":[],"globals":[],"functions":[]}}"#).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["ir", "lower"])
            .arg(&input)
            .args(args)
            .output()
            .unwrap()
    };
    let default = run(&[]);
    let explicit = run(&["--target", "x86_64-whale-linux"]);
    assert!(default.status.success());
    assert!(explicit.status.success());
    assert_eq!(default.stdout, explicit.stdout);
    let printed = String::from_utf8(default.stdout).unwrap();
    assert!(printed.contains("x86_64-whale-linux"));
    assert!(printed.contains("ptr=64, endian=little"), "{printed}");
}
