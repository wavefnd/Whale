use std::{fs, process::Command};

#[test]
fn macho_cli_selects_machine_and_preserves_files_on_unsupported_forms() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.s");
    let output = dir.path().join("out.o");
    for (arch, source, cpu) in [
        ("amd64", "global _entry\n_entry: ret\n", 0x01000007u32),
        ("aarch64", ".global _entry\n_entry: ret\n", 0x0100000c),
    ]
    .into_iter()
    .filter(|(arch, _, _)| {
        (cfg!(feature = "asm-amd64") && *arch == "amd64")
            || (cfg!(feature = "asm-aarch64") && *arch == "aarch64")
    }) {
        fs::write(&input, source).unwrap();
        let r = Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["asm", "--arch", arch, "--format", "macho"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let bytes = fs::read(&output).unwrap();
        assert_eq!(&bytes[..4], 0xfeedfacfu32.to_le_bytes());
        assert_eq!(&bytes[4..8], cpu.to_le_bytes());
    }
    for arch in ["riscv64", "loongarch64"] {
        fs::write(&output, b"existing artifact").unwrap();
        let r = Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["asm", "--arch", arch, "--format", "macho"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert!(!r.status.success());
        assert_eq!(fs::read(&output).unwrap(), b"existing artifact");
    }
    if cfg!(feature = "asm-aarch64") {
        fs::write(&input, ".text\n.global _entry\n.extern _other\n_entry: adr x0,_other\nret\n")
            .unwrap();
        let r = Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["asm", "--arch", "aarch64", "--format", "macho"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert!(!r.status.success());
        assert_eq!(fs::read(&output).unwrap(), b"existing artifact");
    }
}
