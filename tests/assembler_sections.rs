#![cfg(feature = "asm-amd64")]

use std::{collections::HashMap, fs, process::Command};

fn u32_at(b: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(b[n..n + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], n: usize) -> u64 {
    u64::from_le_bytes(b[n..n + 8].try_into().unwrap())
}

#[test]
fn explicit_section_identity_and_alignment_reach_elf_headers_and_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("sections.asm");
    let output = dir.path().join("sections.o");
    fs::write(&input,"section .code,text\nglobal entry\nentry:nop\nalign 32\nafter:ret\nsection .constants,rodata\ndb 7\nalign 16\nsection .writable,data\ndb 8\nalign 8\nsection .zero,bss\nresb 1099511627777\nalign 64\nzero_end:").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["asm", "--amd64"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let b = fs::read(output).unwrap();
    let shoff = u64_at(&b, 40) as usize;
    let count = u16::from_le_bytes(b[60..62].try_into().unwrap()) as usize;
    let names_index = u16::from_le_bytes(b[62..64].try_into().unwrap()) as usize;
    let strings = u64_at(&b, shoff + names_index * 64 + 24) as usize;
    let mut sections = HashMap::new();
    for n in 1..count {
        let h = shoff + n * 64;
        let p = strings + u32_at(&b, h) as usize;
        let end = b[p..].iter().position(|v| *v == 0).unwrap();
        let name = std::str::from_utf8(&b[p..p + end]).unwrap();
        sections.insert(name, h);
    }
    for (name, kind, flags, align, size) in [
        (".text", 1, 6, 16, 0),
        (".code", 1, 6, 32, 33),
        (".constants", 1, 2, 16, 16),
        (".writable", 1, 3, 8, 8),
        (".zero", 8, 3, 64, (1u64 << 40) + 64),
    ] {
        let h = sections[name];
        assert_eq!(u32_at(&b, h + 4), kind, "{name}");
        assert_eq!(u64_at(&b, h + 8), flags, "{name}");
        assert_eq!(u64_at(&b, h + 48), align, "{name}");
        assert_eq!(u64_at(&b, h + 32), size, "{name}");
        assert_eq!(u64_at(&b, h + 24) % align, 0, "{name}");
    }
    let code = u64_at(&b, sections[".code"] + 24) as usize;
    assert_eq!(&b[code..code + 33], [vec![0x90; 32], vec![0xc3]].concat());
    let symtab = sections[".symtab"];
    let symoff = u64_at(&b, symtab + 24) as usize;
    let symsize = u64_at(&b, symtab + 32) as usize;
    let linked = u32_at(&b, symtab + 40) as usize;
    let strtab = u64_at(&b, shoff + linked * 64 + 24) as usize;
    let mut values = HashMap::new();
    for s in b[symoff..symoff + symsize].chunks_exact(24) {
        let p = strtab + u32_at(s, 0) as usize;
        let end = b[p..].iter().position(|v| *v == 0).unwrap();
        values.insert(std::str::from_utf8(&b[p..p + end]).unwrap(), u64_at(s, 8));
    }
    assert_eq!(values["after"], 32);
    assert_eq!(values["zero_end"], (1 << 40) + 64);
    assert!(b.len() < 4096, "BSS must remain sparse");
}
