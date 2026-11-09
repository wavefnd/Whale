use std::{path::PathBuf, process::Command};

#[test]
fn documented_storage_fixture_executes_and_reports_runtime_faults_through_the_cli() {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("bindings/rust/ir/tests/fixtures/runtime-storage-v6.wir");
    for (function, expected) in [("@f1", "u32 12\n"), ("@f3", "u32 42\n"), ("@f7", "i128 16\n")] {
        let output = Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["ir", "run"])
            .arg(&input)
            .args(["--function", function])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(output.stdout, expected.as_bytes());
    }
    for (function, reason) in
        [("@f4", "uninitialized byte"), ("@f5", "permission denied"), ("@f6", "lifetime has ended")]
    {
        let output = Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["ir", "run"])
            .arg(&input)
            .args(["--function", function])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(reason));
    }
}
