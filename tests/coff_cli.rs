use std::{fs, process::Command};

#[test]
fn coff_cli_uses_explicit_machine_and_preserves_output_on_invalid_options() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.s");
    let output = dir.path().join("out.obj");
    for (arch, source, machine) in [
        ("amd64", "global entry\nentry: ret\n", 0x8664u16),
        ("aarch64", ".global entry\nentry: ret\n", 0xaa64),
    ]
    .into_iter()
    .filter(|(arch, _, _)| {
        (cfg!(feature = "asm-amd64") && *arch == "amd64")
            || (cfg!(feature = "asm-aarch64") && *arch == "aarch64")
    }) {
        fs::write(&input, source).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["asm", "--arch", arch, "--format", "coff"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        assert_eq!(&fs::read(&output).unwrap()[..2], machine.to_le_bytes());
    }
    fs::write(&output, b"previous").unwrap();
    for options in [
        vec!["--format", "unknown"],
        vec!["--format", "coff", "--format", "elf"],
        vec!["--riscv64", "--format", "coff"],
        vec!["--format"],
    ] {
        let r = Command::new(env!("CARGO_BIN_EXE_whale"))
            .arg("asm")
            .args(options)
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert!(!r.status.success());
        assert_eq!(fs::read(&output).unwrap(), b"previous");
    }
}
