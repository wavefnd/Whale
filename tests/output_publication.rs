use std::{fs, path::Path, process::Command};

fn run(mode: &str, input: &Path, output: &Path) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_whale"));
    match mode {
        "asm" => {
            command.args(["asm", "--amd64"]);
        }
        "ir" => {
            command.args(["ir", "lower"]);
        }
        _ => {
            command.arg("object");
        }
    }
    command.arg(input).arg("-o").arg(output).output().unwrap()
}

fn cases() -> Vec<(&'static str, &'static [u8])> {
    #[allow(unused_mut)]
    let mut cases: Vec<(&str, &[u8])> = vec![("asm", b"section .text\nret\n"), ("object", b"\xc3")];
    #[cfg(feature = "socket-cli")]
    cases.push(("ir", br#"{"format_version":2,"semantics_version":1,"features":[],"program":{"declarations":[],"globals":[],"functions":[]}}"#));
    cases
}

#[test]
fn every_writer_rejects_source_aliases_and_preserves_source_bytes() {
    let scratch = tempfile::tempdir().unwrap();
    for (mode, source) in cases() {
        let input = scratch.path().join(format!("{mode} source.o"));
        fs::write(&input, source).unwrap();
        let hard = scratch.path().join(format!("{mode} hardlink.o"));
        fs::hard_link(&input, &hard).unwrap();
        #[allow(unused_mut)]
        let mut outputs = vec![input.clone(), hard];
        #[cfg(unix)]
        {
            let link = scratch.path().join(format!("{mode} symlink.o"));
            std::os::unix::fs::symlink(&input, &link).unwrap();
            outputs.push(link);
        }
        for output in outputs {
            let result = run(mode, &input, &output);
            assert!(!result.status.success(), "{mode}: accepted source alias");
            assert!(
                String::from_utf8_lossy(&result.stderr).contains("same file"),
                "{result:?}"
            );
            assert_eq!(fs::read(&input).unwrap(), source);
            assert_eq!(fs::read(&output).unwrap(), source);
        }
    }
}

#[test]
fn every_writer_replaces_outputs_with_spaces_and_cleans_temporary_files() {
    let scratch = tempfile::tempdir().unwrap();
    for (mode, source) in cases() {
        let input = scratch.path().join("input file");
        let output = scratch.path().join("output file.o");
        fs::write(&input, source).unwrap();
        fs::write(&output, b"prior output").unwrap();
        let result = run(mode, &input, &output);
        assert!(result.status.success(), "{result:?}");
        let bytes = fs::read(&output).unwrap();
        assert!(bytes.starts_with(if mode == "ir" { b"module" } else { b"\x7fELF" }));
        assert_eq!(fs::read(&input).unwrap(), source);
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 2);
    }
}

#[test]
fn output_errors_are_diagnostics_without_panics_or_leftovers() {
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("input");
    let output = scratch.path().join("directory.o");
    fs::create_dir(&output).unwrap();
    for (mode, source) in cases() {
        fs::write(&input, source).unwrap();
        let result = run(mode, &input, &output);
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("panicked"));
        assert!(output.is_dir());
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 2);
    }
}
