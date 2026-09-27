#[cfg(feature = "socket-cli")]
use std::fs;
use std::process::Command;

#[test]
fn disabled_feature_reports_recovery_without_output() {
    if cfg!(feature = "socket-cli") {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("out.wir");
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["ir", "lower", "missing.json", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("--features socket-cli"));
    assert!(!output.exists());
}

#[test]
#[cfg(feature = "socket-cli")]
fn schema_and_lowering_errors_preserve_existing_output_even_without_verify() {
    let valid = include_str!("../ir/tests/fixtures/ast-v2-u128.json");
    let calls = include_str!("../ir/tests/fixtures/ast-v2-calls.json");
    let cases = [
        (
            calls.replace("\"Direct\": \"emit\"", "\"Direct\": \"missing\""),
            "UnknownFunctionName",
        ),
        (
            calls.replace("\"convention\": \"Whale\"", "\"convention\": \"Unknown\""),
            "AST envelope/schema",
        ),
        (
            "{\"globals\":[],\"functions\":[]}".to_string(),
            "AST envelope/schema",
        ),
        (
            valid.replace("\"format_version\": 2", "\"format_version\": 9"),
            "unsupported AST format_version",
        ),
        (
            valid.replace("\"name\": \"answer\"", "\"name\": \"a\", \"name\": \"b\""),
            "duplicate JSON key",
        ),
        (
            valid.replace(&u128::MAX.to_string(), "-1"),
            "NumericLiteral",
        ),
    ];
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("input.json");
    let output = tmp.path().join("output.wir");
    for (source, expected) in cases {
        fs::write(&input, source).unwrap();
        for exists in [false, true] {
            if exists {
                fs::write(&output, "previous").unwrap();
            }
            let result = Command::new(env!("CARGO_BIN_EXE_whale"))
                .args(["ir", "lower"])
                .arg(&input)
                .args(["--no-verify", "-o"])
                .arg(&output)
                .output()
                .unwrap();
            assert!(!result.status.success());
            assert!(
                String::from_utf8_lossy(&result.stderr).contains(expected),
                "{result:?}"
            );
            if exists {
                assert_eq!(fs::read(&output).unwrap(), b"previous");
                fs::remove_file(&output).unwrap();
            } else {
                assert!(!output.exists());
            }
        }
    }
}
