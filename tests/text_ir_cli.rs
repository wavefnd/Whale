use std::{fs, process::Command};
fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn typed_ir_commands_work_without_socket_and_publish_only_verified_output() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("input.wir");
    let output = tmp.path().join("output.wir");
    let input = input.to_str().unwrap();
    let output = output.to_str().unwrap();
    let good = include_str!("../ir/tests/fixtures/wave-control-v3.wir");
    fs::write(input, good).unwrap();
    let result = run(&["ir", "verify", input]);
    assert!(result.status.success(), "{result:?}");
    let result = run(&["ir", "print", input]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, good.as_bytes());
    let result = run(&["ir", "print", input, "-o", output]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(fs::read_to_string(output).unwrap(), good);
    for (source, args) in [
        (good.to_string(), vec!["ir", "print", input, "-o", input]),
        (
            good.replace("format_version 3", "format_version 2"),
            vec!["ir", "print", input, "-o", output],
        ),
        (
            good.replace("cbr bool", "unknown bool"),
            vec!["ir", "print", input, "-o", output],
        ),
    ] {
        fs::write(input, &source).unwrap();
        fs::write(output, "previous").unwrap();
        let result = run(&args);
        assert!(!result.status.success());
        assert_eq!(fs::read_to_string(input).unwrap(), source);
        assert_eq!(fs::read(output).unwrap(), b"previous");
    }
    for args in [
        vec!["ir", "missing", input],
        vec!["ir", "verify", input, "-o", output],
        vec!["ir", "print", input, "--no-verify"],
        vec!["ir", "print", input, "-o"],
        vec!["ir", "verify", input, "extra"],
    ] {
        assert!(!run(&args).status.success());
    }
}
#[test]
fn hardlink_output_cannot_overwrite_ir_input() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("input.wir");
    let alias = tmp.path().join("alias.wir");
    let good = include_str!("../ir/tests/fixtures/calls-v3.wir");
    fs::write(&input, good).unwrap();
    fs::hard_link(&input, &alias).unwrap();
    assert!(!run(&[
        "ir",
        "print",
        input.to_str().unwrap(),
        "-o",
        alias.to_str().unwrap()
    ])
    .status
    .success());
    assert_eq!(fs::read_to_string(input).unwrap(), good);
}
