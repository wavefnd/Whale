#![cfg(feature = "asm-amd64")]

use std::{fs, path::Path, process::Command};

fn whale() -> Command {
    Command::new(env!("CARGO_BIN_EXE_whale"))
}

fn assemble(dir: &Path, name: &str, source: &str) -> std::path::PathBuf {
    let input = dir.join(format!("{name}.asm"));
    let out = dir.join(format!("{name}.o"));
    fs::write(&input, source).unwrap();
    let status = whale().args(["asm", "--amd64"]).arg(&input).arg("-o").arg(&out).output().unwrap();
    assert!(status.status.success(), "{}", String::from_utf8_lossy(&status.stderr));
    out
}

#[test]
fn link_arguments_and_failures_preserve_every_input_and_existing_output() {
    let dir = tempfile::tempdir().unwrap();
    let first = assemble(dir.path(), "first", "section .text\nglobal _start\n_start: ret\n");
    let second = assemble(dir.path(), "second", "section .data\ndb 1\n");
    let old = fs::read(&second).unwrap();
    let result =
        whale().arg("link").arg(&first).arg(&second).arg("-o").arg(&second).output().unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read(&second).unwrap(), old);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let alias = dir.path().join("alias");
        symlink(&second, &alias).unwrap();
        assert!(!whale()
            .arg("link")
            .arg(&first)
            .arg(&second)
            .arg("-o")
            .arg(&alias)
            .status()
            .unwrap()
            .success());
        assert_eq!(fs::read(&second).unwrap(), old);
    }
    let output = dir.path().join("app");
    fs::write(&output, b"old").unwrap();
    for flags in [
        vec!["--entry", "missing"],
        vec!["--max-output", "1"],
        vec!["--max-input", "1"],
        vec!["--unknown"],
    ] {
        let status =
            whale().arg("link").arg(&first).arg("-o").arg(&output).args(flags).output().unwrap();
        assert!(!status.status.success());
        assert_eq!(fs::read(&output).unwrap(), b"old");
    }
    assert!(!whale().args(["link", "-o"]).output().unwrap().status.success());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn two_disk_objects_link_and_execute_with_relative_call_and_data_reference() {
    let dir = tempfile::tempdir().unwrap();
    let a=assemble(dir.path(),"start","section .text\nglobal _start\nextern helper\nextern payload\n_start: lea rsi,[rel payload]\nmov eax,[rsi]\ncall helper\nmov edi,eax\nmov eax,60\nsyscall\n");
    let b = assemble(
        dir.path(),
        "helper",
        "section .text\nglobal helper\nhelper: ret\nsection .data\nglobal payload\npayload: dd 42\nsection .bss\nresb 64\n",
    );
    let out = dir.path().join("program with spaces");
    let result = whale().arg("link").arg(a).arg(b).arg("-o").arg(&out).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(Command::new(out).status().unwrap().code(), Some(42));
}

#[test]
fn cumulative_raw_section_budget_rejects_multiple_small_objects_and_preserves_output() {
    use object::*;
    let dir = tempfile::tempdir().unwrap();
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    object.add_section(".text", SectionKind::Text, 1);
    object.sections[0].data = vec![0xc3];
    for _ in 1..33_000 {
        object.add_section(".data", SectionKind::Data, 1);
    }
    object.symbols.push(ObjectSymbol {
        name: "_start".into(),
        section_index: Some(0),
        value: 0,
        size: 1,
        binding: SymbolBinding::Global,
        kind: SymbolKind::Function,
        visibility: SymbolVisibility::Default,
    });
    let first = dir.path().join("first.o");
    fs::write(&first, object.write().unwrap()).unwrap();
    object.symbols[0].name = "unused".into();
    let second = dir.path().join("second.o");
    fs::write(&second, object.write().unwrap()).unwrap();
    let output = dir.path().join("existing");
    fs::write(&output, b"old executable").unwrap();
    let result =
        whale().arg("link").arg(&first).arg(&second).arg("-o").arg(&output).output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Sections"), "{result:?}");
    assert_eq!(fs::read(&output).unwrap(), b"old executable");
}

#[test]
fn unsupported_user_address_extent_fails_before_output_publication() {
    let dir = tempfile::tempdir().unwrap();
    let input = assemble(
        dir.path(),
        "huge-bss",
        "section .text\nglobal _start\n_start: ret\nsection .bss\nresb 9223372036854775808\n",
    );
    let output = dir.path().join("existing");
    fs::write(&output, b"old executable").unwrap();
    let result = whale().arg("link").arg(input).arg("-o").arg(&output).output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("user address"), "{result:?}");
    assert_eq!(fs::read(&output).unwrap(), b"old executable");
}

#[test]
fn byte_limit_diagnostics_name_the_offending_flag_before_publication() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("existing");
    fs::write(&output, b"previous").unwrap();
    for flag in ["--max-input", "--max-output"] {
        for (value, message) in [
            ("nope", "requires a decimal byte count"),
            ("18446744073709551615", "byte count exceeds host capacity"),
        ] {
            let result = whale()
                .args(["link", "missing.o", "-o"])
                .arg(&output)
                .args([flag, value])
                .output()
                .unwrap();
            assert_eq!(result.status.code(), Some(1));
            assert!(
                String::from_utf8_lossy(&result.stderr).contains(&format!("{flag} {message}")),
                "{result:?}"
            );
            assert_eq!(fs::read(&output).unwrap(), b"previous");
        }
    }
    let input = assemble(dir.path(), "entry", "section .text\nglobal _start\n_start: ret\n");
    let result = whale()
        .arg("link")
        .arg(input)
        .arg("-o")
        .arg(&output)
        .args(["--max-output", "0"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let diagnostic = String::from_utf8_lossy(&result.stderr);
    assert!(!diagnostic.contains("decimal byte count"), "{diagnostic}");
    assert!(!diagnostic.contains("host capacity"), "{diagnostic}");
    assert_eq!(fs::read(output).unwrap(), b"previous");
}
