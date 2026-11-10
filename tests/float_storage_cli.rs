use std::{path::PathBuf, process::Command};

fn run(function: &str, literal: Option<&str>) -> std::process::Output {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("bindings/rust/ir/tests/fixtures/float-storage-v7.wir");
    let mut command = Command::new(env!("CARGO_BIN_EXE_whale"));
    command.args(["ir", "run"]).arg(input).args(["--function", function]);
    if let Some(value) = literal {
        command.args(["--arg", value]);
    }
    command.output().unwrap()
}

#[test]
fn float_arguments_results_and_storage_use_exact_width_hexadecimal_bits() {
    for (function, literal, expected) in [
        ("@f0", Some("0x7c01"), "f16 0x7c01\n"),
        ("@f1", Some("0x80000000"), "f32 0x80000000\n"),
        ("@f2", Some("0xfff0000000000123"), "f64 0xfff0000000000123\n"),
        ("@f3", None, "u64 9218868437227405603\n"),
    ] {
        let output = run(function, literal);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(output.stdout, expected.as_bytes());
    }
}

#[test]
fn malformed_float_arguments_fail_without_panicking_or_printing_a_value() {
    for literal in ["0x0000", "0x000000000", "0xgggggggg", "NaN", "-0.0", "0X80000000"] {
        let output = run("@f1", Some(literal));
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("floats exact-width 0x bits"), "{stderr}");
        assert!(!stderr.contains("panicked"));
    }
}
