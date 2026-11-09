use std::{path::PathBuf, process::Command};

#[test]
fn cli_reports_original_source_and_both_direct_and_indirect_callers_without_reading_source_files() {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("bindings/rust/ir/tests/fixtures/runtime-diagnostics-v7.wir");
    let output = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["ir", "run"])
        .arg(input)
        .args(["--function", "@f0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("division by zero"));
    assert!(error.contains("original.wave:11:13"));
    assert!(error.contains("@f2 %b0 instruction 1"));
    assert_eq!(error.matches("called from").count(), 2);
}
