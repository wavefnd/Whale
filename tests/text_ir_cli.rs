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

#[test]
fn run_executes_exact_arguments_and_reports_traps_fuel_and_unsupported_operations() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("loop input.wir");
    let input = path.to_str().unwrap();
    let source = include_str!("../ir/tests/fixtures/interpreter-loop-v3.wir");
    fs::write(input, source).unwrap();
    let result = run(&["ir", "run", input, "--function", "@f7", "--arg", "3"]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"i32 22\n");
    assert!(result.stderr.is_empty());
    for (args, message) in [
        (
            vec!["--function", "@f7", "--arg", "0", "--max-steps", "0"],
            "step limit 0",
        ),
        (vec!["--function", "@f7", "--arg", "-1"], "argument 0"),
        (
            vec!["--function", "@f7", "--arg", "4294967296"],
            "argument 0",
        ),
        (vec!["--function", "@f7"], "expected 1 arguments"),
        (vec!["--function", "@f9"], "no body"),
        (vec![], "requires --function"),
        (vec!["--function", "@f7", "--function", "@f7"], "duplicate"),
        (
            vec!["--function", "@f7", "--max-steps", "1", "--max-steps", "1"],
            "duplicate",
        ),
        (
            vec!["--function", "@f7", "--max-steps", "+1"],
            "decimal u64",
        ),
        (vec!["--function", "7"], "requires @fN"),
        (vec!["--function", "@f4294967296"], "u32 ID"),
        (
            vec!["--function", "@f7", "-o", input],
            "unexpected argument",
        ),
        (
            vec!["--function", "@f7", "--no-verify"],
            "unexpected argument",
        ),
        (vec!["--function", "@f7", "--arg"], "requires a literal"),
    ] {
        let mut arguments = vec!["ir", "run", input];
        arguments.extend(args);
        let result = run(&arguments);
        assert!(!result.status.success(), "{arguments:?}");
        assert!(result.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(message),
            "{result:?}"
        );
        assert_eq!(fs::read_to_string(input).unwrap(), source);
    }
    let trap = "module { format_version 3 semantics_version 1 target \"x86_64-whale-linux\" datalayout { ptr=64, endian=little } declare @f7 \"div\": whale (i128, i128) -> i128, linkage internal fn @f7 \"div\"(%v0 \"a\": i128, %v1 \"b\": i128) -> i128, entry %b11 { %b11 \"entry\": %v8: i128 = sdiv i128 %v0, %v1 ret i128 %v8 } }";
    fs::write(input, trap).unwrap();
    let result = run(&[
        "ir",
        "run",
        input,
        "--function",
        "@f7",
        "--arg",
        "-170141183460469231731687303715884105728",
        "--arg",
        "-1",
    ]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        result.stdout,
        b"i128 -170141183460469231731687303715884105728\n"
    );
    let result = run(&[
        "ir",
        "run",
        input,
        "--function",
        "@f7",
        "--arg",
        "1",
        "--arg",
        "0",
    ]);
    assert_eq!(result.status.code(), Some(1));
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(
        error.contains("division by zero") && error.contains("@f7 %b11 instruction 0 (%v8)"),
        "{error}"
    );
    assert!(error.contains(input));
    fs::write(
        input,
        include_str!("../ir/tests/fixtures/wave-casts-v3.wir"),
    )
    .unwrap();
    let result = run(&["ir", "run", input, "--function", "@f0"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr)
        .contains("unsupported interpreter operation alloca"));
}
