use std::{path::PathBuf, process::Command};

#[test]
fn documented_float_comparisons_report_boolean_results() {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("bindings/rust/ir/tests/fixtures/float-comparisons-v7.wir");
    for (function, a, b, expected) in [
        ("@f0", "0x7ff0000000000001", "0x3ff0000000000000", "bool true\n"),
        ("@f0", "0x3ff0000000000000", "0xfff8123456789abc", "bool true\n"),
        ("@f1", "0x7ff0000000000001", "0x3ff0000000000000", "bool false\n"),
        ("@f1", "0x3ff0000000000000", "0xfff8123456789abc", "bool false\n"),
        ("@f0", "0x7ff0000000000000", "0x7ff0000000000000", "bool false\n"),
        ("@f2", "0x8000", "0x0000", "bool true\n"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["ir", "run"])
            .arg(&input)
            .args(["--function", function, "--arg", a, "--arg", b])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(output.stdout, expected.as_bytes());
    }
}
