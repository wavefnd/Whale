use std::{fs, process::Command};

fn whale() -> Command {
    Command::new(env!("CARGO_BIN_EXE_whale"))
}

fn source(target: &str) -> String {
    format!(
        concat!(
            "module {{\n format_version 8\n semantics_version 1\n target \"{}\"\n",
            " datalayout {{ ptr=64, endian=little }}\n",
            " declare @f0 \"answer\": whale (i32) -> i32, linkage internal\n",
            " fn @f0 \"answer\"(%v7 \"argument\": i32) -> i32, entry %b9 {{\n %b9 \"entry\":\n",
            " %v11: i32 = const i32 2\n %v30: i32 = add i32 %v7, %v11\n",
            " ret i32 %v30\n }}\n}}\n"
        ),
        target
    )
}

#[test]
fn bytecode_argument_errors_precede_file_access() {
    for arguments in [
        vec!["bytecode", "run", "missing.wbc"],
        vec!["bytecode", "run", "missing.wbc", "--function", "@f-1"],
        vec!["bytecode", "verify", "missing.wbc", "--max-work", "-1"],
        vec!["bytecode", "verify", "missing.wbc", "--max-work", "1", "--max-work", "2"],
        vec!["bytecode", "verify", "missing.wbc", "--arg", "1"],
        vec!["ir", "emit", "missing.wir", "--target", "missing", "-o", "missing.wbc"],
    ] {
        let result = whale().args(arguments).output().unwrap();
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("failed to read"));
    }
}

#[test]
fn bytecode_compile_verify_run_and_limits_preserve_files() {
    if !ir::bytecode::available() {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("answer.wir");
    let output = directory.path().join("answer.wbc");
    fs::write(&input, source("whale-bytecode-v1")).unwrap();
    let emit = || {
        whale()
            .args(["ir", "emit"])
            .arg(&input)
            .args(["--target", "whale-bytecode-v1", "-o"])
            .arg(&output)
            .output()
            .unwrap()
    };
    let result = emit();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let bytes = fs::read(&output).unwrap();
    assert!(emit().status.success());
    assert_eq!(fs::read(&output).unwrap(), bytes);
    let verified = whale().args(["bytecode", "verify"]).arg(&output).output().unwrap();
    assert!(verified.status.success(), "{verified:?}");
    let run = whale()
        .args(["bytecode", "run"])
        .arg(&output)
        .args(["--function", "@f0", "--arg", "40"])
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    assert_eq!(run.stdout, b"i32 42\n");
    let constant_input = directory.path().join("fold.wir");
    let folded = directory.path().join("folded.wir");
    fs::write(
        &constant_input,
        source("whale-bytecode-v1")
            .replace("whale (i32)", "whale ()")
            .replace("%v7 \"argument\": i32", "")
            .replace("%v11: i32 = const", "%v7: i32 = const i32 40\n %v11: i32 = const"),
    )
    .unwrap();
    let fold =
        whale().args(["ir", "fold"]).arg(&constant_input).arg("-o").arg(&folded).output().unwrap();
    assert!(fold.status.success(), "{fold:?}");
    let folded_source = fs::read_to_string(&folded).unwrap();
    assert!(folded_source.contains("const i32 42"));
    assert!(!folded_source.contains("add i32"));
    let external = fs::read_to_string(&constant_input).unwrap()
        .replace("declare @f0", "declare @f1 \"foreign\": sysv64 () -> i32, linkage external, link_name \"foreign\"\n declare @f0")
        .replace("%v7: i32 = const i32 40", "%v7: i32 = call sysv64 i32 @f1()");
    fs::write(&constant_input, external).unwrap();
    let rejected =
        whale().args(["ir", "emit"]).arg(&constant_input).arg("-o").arg(&output).output().unwrap();
    assert!(!rejected.status.success());
    assert_eq!(fs::read(&output).unwrap(), bytes);
    for option in ["--max-steps", "--max-work", "--max-workspace", "--max-decoded", "--max-input"] {
        let run = whale()
            .args(["bytecode", "run"])
            .arg(&output)
            .args(["--function", "@f0", "--arg", "40", option, "0"])
            .output()
            .unwrap();
        assert!(!run.status.success(), "{option}");
        assert_eq!(fs::read(&output).unwrap(), bytes);
    }
    let limited = whale()
        .args(["ir", "emit"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["--max-output", "1"])
        .output()
        .unwrap();
    assert!(!limited.status.success());
    assert_eq!(fs::read(&output).unwrap(), bytes);
    let original = fs::read(&input).unwrap();
    let same = whale().args(["ir", "emit"]).arg(&input).arg("-o").arg(&input).output().unwrap();
    assert!(!same.status.success());
    assert_eq!(fs::read(&input).unwrap(), original);
    let mut damaged = bytes.clone();
    damaged[0] ^= 1;
    fs::write(&output, &damaged).unwrap();
    let run = whale()
        .args(["bytecode", "run"])
        .arg(&output)
        .args(["--function", "@f0", "--arg", "40"])
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert_eq!(fs::read(&output).unwrap(), damaged);
}

#[test]
fn emission_never_relabels_a_native_module() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("native.wir");
    let output = directory.path().join("keep.wbc");
    fs::write(&input, source("x86_64-whale-linux")).unwrap();
    fs::write(&output, b"previous").unwrap();
    let result = whale().args(["ir", "emit"]).arg(&input).arg("-o").arg(&output).output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("differs from --target"));
    assert_eq!(fs::read(output).unwrap(), b"previous");
}
