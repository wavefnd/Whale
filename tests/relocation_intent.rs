#![cfg(feature = "asm-amd64")]

use std::{fs, process::Command};

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn relocations(source: &str) -> Vec<(u64, u32, i64)> {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.asm");
    let output = dir.path().join("output.o");
    fs::write(&input, source).unwrap();
    let command = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["asm", "--amd64"])
        .arg(input)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(command.status.success(), "{command:?}");
    let bytes = fs::read(output).unwrap();
    assert_eq!(&bytes[..4], b"\x7fELF");
    let shoff = u64_at(&bytes, 40) as usize;
    let count = u16::from_le_bytes(bytes[60..62].try_into().unwrap()) as usize;
    let mut result = vec![];
    for index in 0..count {
        let header = shoff + index * 64;
        if u32_at(&bytes, header + 4) != 4 {
            continue;
        }
        let offset = u64_at(&bytes, header + 24) as usize;
        let size = u64_at(&bytes, header + 32) as usize;
        assert_eq!(u64_at(&bytes, header + 56), 24);
        for record in bytes[offset..offset + size].chunks_exact(24) {
            result.push((
                u64_at(record, 0),
                u64_at(record, 8) as u32,
                i64::from_le_bytes(record[16..24].try_into().unwrap()),
            ));
        }
    }
    result
}

#[test]
fn undefined_data_references_are_pc_relative_and_branches_keep_plt_intent() {
    let entries =
        relocations("extern ext\nmov rax, [ext]\nmov [ext], rax\ncall ext\njmp ext\nje ext");
    assert_eq!(entries, vec![(3, 2, -4), (10, 2, -4), (15, 4, -4), (20, 4, -4), (26, 4, -4)]);
}

#[test]
fn defined_cross_section_branches_are_not_plt_references() {
    let entries = relocations("section .text\ncall target\nsection .text.other\ntarget: ret");
    assert_eq!(entries, vec![(1, 2, -4)]);
}
