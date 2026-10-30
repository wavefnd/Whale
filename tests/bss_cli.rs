#![cfg(feature = "asm-amd64")]

use std::{fs, process::Command};

#[test]
fn assembler_cli_preserves_huge_bss_size_in_a_small_object() {
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("bss.asm");
    let output = scratch.path().join("bss.o");
    fs::write(
        &input,
        "section .bss\nglobal buffer\nbuffer:\nresb 1099511627776\nend:\nsection .text\nret\n",
    )
    .unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["asm", "--amd64"])
        .arg(input)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let bytes = fs::read(output).unwrap();
    assert!(bytes.len() < 2048);
    let table = u64::from_le_bytes(bytes[40..48].try_into().unwrap()) as usize;
    // .text is assembler section 0; .bss is section 1, after the ELF null section.
    let bss = table + 2 * 64;
    assert_eq!(u32::from_le_bytes(bytes[bss + 4..bss + 8].try_into().unwrap()), 8);
    assert_eq!(u64::from_le_bytes(bytes[bss + 32..bss + 40].try_into().unwrap()), 1 << 40);
}
