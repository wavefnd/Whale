//! Golden bytes come from the AMD64 opcode/ModRM tables, independently of the
//! implementation. Tests use the public assembler and need no external assembler.
use amd64::AMD64;

use assembler::{assemble, RelocKind};

fn bytes(src: &str) -> Vec<u8> {
    let out = assemble(src, &AMD64).unwrap_or_else(|e| panic!("{src}: {e}"));
    assert!(out.sections[0].relocs.is_empty(), "{src}");
    out.sections.into_iter().flat_map(|s| s.data).collect()
}

fn rejects(src: &str) -> String {
    assemble(src, &AMD64).err().unwrap_or_else(|| panic!("accepted {src}")).to_string()
}

#[test]
fn rex_only_low_bytes_cover_register_immediate_memory_and_extended_forms() {
    for (name, code) in [("spl", 4), ("bpl", 5), ("sil", 6), ("dil", 7)] {
        assert_eq!(bytes(&format!("mov {name}, al")), [0x40, 0x88, 0xc0 + code]);
        assert_eq!(bytes(&format!("mov al, {name}")), [0x40, 0x88, 0xc0 + (code << 3)]);
        assert_eq!(bytes(&format!("mov {name}, 255")), [0x40, 0xb0 + code, 0xff]);
        assert_eq!(bytes(&format!("add {name}, 1")), [0x40, 0x80, 0xc0 + code, 1]);
        assert_eq!(bytes(&format!("mov {name}, [rax]")), [0x40, 0x8a, code << 3]);
        assert_eq!(bytes(&format!("mov [rax], {name}")), [0x40, 0x88, code << 3]);
        assert_eq!(bytes(&format!("mov {name}, r8b")), [0x44, 0x88, 0xc0 + code]);
        assert_eq!(bytes(&format!("mov r8b, {name}")), [0x41, 0x88, 0xc0 + (code << 3)]);
        for other in ["ah", "ch", "dh", "bh"] {
            for src in [
                format!("mov {name}, {other}"),
                format!("mov {other}, {name}"),
                format!("test {name}, {other}"),
            ] {
                assert!(rejects(&src).contains("REX"));
            }
        }
    }
    for src in ["mov ah, r8b", "mov r8b, ah", "mov ah, [r8]", "mov [r8], ah", "add bh, [rbx+r12*2]"]
    {
        assert!(rejects(src).contains("REX"));
    }
    assert_eq!(bytes("mov ah, 1\nmov ah, al"), [0xb4, 1, 0x88, 0xc4]);
}

pub const LEA_FIXTURES: &[(&str, &[u8])] = &[
    ("lea rax, [rbx]", &[0x48, 0x8d, 0x03]),
    ("lea rax, [rbx+16]", &[0x48, 0x8d, 0x43, 0x10]),
    ("lea r8, [r9+rcx*4+32]", &[0x4d, 0x8d, 0x44, 0x89, 0x20]),
    ("lea eax, [rbp+8]", &[0x8d, 0x45, 0x08]),
    ("lea r9d, [r12+r10*8-128]", &[0x47, 0x8d, 0x4c, 0xd4, 0x80]),
    ("lea rax, [rsp]", &[0x48, 0x8d, 0x04, 0x24]),
    ("lea rax, [r12]", &[0x49, 0x8d, 0x04, 0x24]),
    ("lea rax, [rbp]", &[0x48, 0x8d, 0x45, 0]),
    ("lea rax, [r13]", &[0x49, 0x8d, 0x45, 0]),
    ("lea eax, [rbx-129]", &[0x8d, 0x83, 0x7f, 0xff, 0xff, 0xff]),
    ("lea rax, [rbx+127]", &[0x48, 0x8d, 0x43, 0x7f]),
    ("lea rax, [rbx+128]", &[0x48, 0x8d, 0x83, 0x80, 0, 0, 0]),
    ("lea rax, [rbx+r12*4]", &[0x4a, 0x8d, 0x04, 0xa3]),
];

#[test]
fn lea_uses_destination_width_and_shared_64_bit_addressing() {
    for (src, expected) in LEA_FIXTURES {
        assert_eq!(bytes(src), *expected, "{src}");
    }
    for src in [
        "lea",
        "lea rax",
        "lea rax, [rbx], 1",
        "lea [rbx], rax",
        "lea rax, rbx",
        "lea ax, [rbx]",
        "lea al, [rbx]",
        "lea rax, [symbol]",
        "lea eax, [ebx]",
        "lea rax, [rbx+2147483648]",
    ] {
        rejects(src);
    }
}

#[test]
fn unary_not_neg_div_idiv_cover_all_widths_and_prefixes() {
    for (mnemonic, ext) in [("not", 2), ("neg", 3), ("div", 6), ("idiv", 7)] {
        for (reg, prefix, opcode, code) in [
            ("bl", vec![], 0xf6, 3),
            ("bh", vec![], 0xf6, 7),
            ("bx", vec![0x66], 0xf7, 3),
            ("ebx", vec![], 0xf7, 3),
            ("rbx", vec![0x48], 0xf7, 3),
            ("r9b", vec![0x41], 0xf6, 1),
            ("r9w", vec![0x66, 0x41], 0xf7, 1),
            ("r9d", vec![0x41], 0xf7, 1),
            ("r9", vec![0x49], 0xf7, 1),
            ("spl", vec![0x40], 0xf6, 4),
        ] {
            let mut expected = prefix;
            expected.extend([opcode, 0xc0 + (ext << 3) + code]);
            assert_eq!(bytes(&format!("{mnemonic} {reg}")), expected);
        }
        for suffix in ["", " rax, rbx", " [rax]", " 1"] {
            rejects(&format!("{mnemonic}{suffix}"));
        }
    }
}

#[test]
fn shifts_select_one_immediate_cl_group_width_and_rex() {
    for (mnemonic, ext) in [("shl", 4), ("sal", 4), ("shr", 5), ("sar", 7)] {
        for (reg, prefix, byte, code) in [
            ("al", vec![], true, 0),
            ("ah", vec![], true, 4),
            ("ax", vec![0x66], false, 0),
            ("eax", vec![], false, 0),
            ("rax", vec![0x48], false, 0),
            ("r9b", vec![0x41], true, 1),
            ("r9w", vec![0x66, 0x41], false, 1),
            ("r9d", vec![0x41], false, 1),
            ("r9", vec![0x49], false, 1),
            ("dil", vec![0x40], true, 7),
        ] {
            for (count, opcode, imm) in [
                ("1", if byte { 0xd0 } else { 0xd1 }, None),
                ("cl", if byte { 0xd2 } else { 0xd3 }, None),
                ("0", if byte { 0xc0 } else { 0xc1 }, Some(0)),
                ("255", if byte { 0xc0 } else { 0xc1 }, Some(255)),
            ] {
                let mut expected = prefix.clone();
                expected.extend([opcode, 0xc0 + (ext << 3) + code]);
                if let Some(imm) = imm {
                    expected.push(imm);
                }
                assert_eq!(bytes(&format!("{mnemonic} {reg}, {count}")), expected);
            }
        }
        for count in ["-1", "256", "rcx", "ecx", "cx", "al", "r8b", "[rbx]"] {
            rejects(&format!("{mnemonic} rax, {count}"));
        }
        for suffix in ["", " rax", " rax, 1, 2", " [rax], 1"] {
            rejects(&format!("{mnemonic}{suffix}"));
        }
    }
}

#[test]
fn dividend_preparation_is_zero_operand_with_exact_prefixes() {
    for (mnemonic, expected) in
        [("cwd", vec![0x66, 0x99]), ("cdq", vec![0x99]), ("cqo", vec![0x48, 0x99])]
    {
        assert_eq!(bytes(mnemonic), expected);
        for operands in ["1", "rax", "rax, rbx"] {
            assert!(rejects(&format!("; comment\n{mnemonic} {operands}")).contains("line 2"));
        }
    }
}

#[test]
fn test_has_flags_only_opcodes_and_rejects_incompatible_registers() {
    for (src, expected) in [
        ("test al, bl", vec![0x84, 0xd8]),
        ("test ax, bx", vec![0x66, 0x85, 0xd8]),
        ("test eax, ebx", vec![0x85, 0xd8]),
        ("test rax, rbx", vec![0x48, 0x85, 0xd8]),
        ("test r8b, r9b", vec![0x45, 0x84, 0xc8]),
        ("test r8w, r9w", vec![0x66, 0x45, 0x85, 0xc8]),
        ("test r8d, r9d", vec![0x45, 0x85, 0xc8]),
        ("test r8, r9", vec![0x4d, 0x85, 0xc8]),
        ("test ah, bh", vec![0x84, 0xfc]),
        ("test sil, dil", vec![0x40, 0x84, 0xfe]),
    ] {
        assert_eq!(bytes(src), expected);
    }
    for src in [
        "test",
        "test rax",
        "test rax, rbx, rcx",
        "test rax, ebx",
        "test al, bx",
        "test ah, r8b",
        "test r8b, ah",
        "test ah, spl",
        "test rax, [rbx]",
        "test rax, 1",
    ] {
        rejects(src);
    }
}

#[test]
fn immediate_imul_checks_ranges_and_preserves_two_operand_forms() {
    for (src, expected) in [
        ("imul rax, rbx, -128", vec![0x48, 0x6b, 0xc3, 0x80]),
        ("imul rax, rbx, 127", vec![0x48, 0x6b, 0xc3, 0x7f]),
        ("imul rax, rbx, -129", vec![0x48, 0x69, 0xc3, 0x7f, 0xff, 0xff, 0xff]),
        ("imul rax, rbx, 128", vec![0x48, 0x69, 0xc3, 0x80, 0, 0, 0]),
        ("imul rax, rbx, -2147483648", vec![0x48, 0x69, 0xc3, 0, 0, 0, 0x80]),
        ("imul rax, rbx, 2147483647", vec![0x48, 0x69, 0xc3, 0xff, 0xff, 0xff, 0x7f]),
        ("imul ax, bx, -32768", vec![0x66, 0x69, 0xc3, 0, 0x80]),
        ("imul ax, bx, 65535", vec![0x66, 0x69, 0xc3, 0xff, 0xff]),
        ("imul eax, ebx, 4294967295", vec![0x69, 0xc3, 0xff, 0xff, 0xff, 0xff]),
        ("imul r8w, r9w, 127", vec![0x66, 0x45, 0x6b, 0xc1, 0x7f]),
        ("imul r8d, r9d, 128", vec![0x45, 0x69, 0xc1, 0x80, 0, 0, 0]),
        ("imul r8, r9, -1", vec![0x4d, 0x6b, 0xc1, 0xff]),
        ("imul rax, rbx", vec![0x48, 0x0f, 0xaf, 0xc3]),
        ("imul rax, [rbx]", vec![0x48, 0x0f, 0xaf, 0x03]),
    ] {
        assert_eq!(bytes(src), expected, "{src}");
    }
    for src in [
        "imul ax, bx, -32769",
        "imul ax, bx, 65536",
        "imul eax, ebx, -2147483649",
        "imul eax, ebx, 4294967296",
        "imul rax, rbx, -2147483649",
        "imul rax, rbx, 2147483648",
        "imul rax, ebx, 1",
        "imul al, bl, 1",
        "imul rax, [rbx], 1",
        "imul rax, rbx, rcx",
        "imul rax",
        "imul rax, 2",
        "imul rax, rbx, 1, 2",
    ] {
        rejects(src);
    }
}

const CONDITIONS: &[(&str, u8)] = &[
    ("jo", 0),
    ("jno", 1),
    ("jb", 2),
    ("jc", 2),
    ("jnae", 2),
    ("jae", 3),
    ("jnb", 3),
    ("jnc", 3),
    ("je", 4),
    ("jz", 4),
    ("jne", 5),
    ("jnz", 5),
    ("jbe", 6),
    ("jna", 6),
    ("ja", 7),
    ("jnbe", 7),
    ("js", 8),
    ("jns", 9),
    ("jp", 10),
    ("jpe", 10),
    ("jnp", 11),
    ("jpo", 11),
    ("jl", 12),
    ("jnge", 12),
    ("jge", 13),
    ("jnl", 13),
    ("jle", 14),
    ("jng", 14),
    ("jg", 15),
    ("jnle", 15),
];

#[test]
fn every_condition_alias_encodes_short_near_and_external_transfers() {
    for (mnemonic, code) in CONDITIONS {
        assert_eq!(bytes(&format!("{mnemonic} end\nnop\nend: ret")), [0x70 + code, 1, 0x90, 0xc3]);
        assert_eq!(bytes(&format!("start: nop\n{mnemonic} start")), [0x90, 0x70 + code, 0xfd]);
        let boundary = bytes(&format!("{mnemonic} end\nresb 127\nend: ret"));
        assert_eq!(&boundary[..2], &[0x70 + code, 127]);
        assert_eq!(2 + boundary[1] as usize, boundary.len() - 1);
        let backward = bytes(&format!("start: resb 126\n{mnemonic} start"));
        assert_eq!(&backward[126..], &[0x70 + code, 0x80]);
        let backward_near = bytes(&format!("start: resb 127\n{mnemonic} start"));
        assert_eq!(&backward_near[127..129], &[0x0f, 0x80 + code]);
        assert_eq!(
            133i64 + i32::from_le_bytes(backward_near[129..133].try_into().unwrap()) as i64,
            0
        );
        let forward = bytes(&format!("{mnemonic} end\nresb 128\nend: ret"));
        assert_eq!(&forward[..6], &[0x0f, 0x80 + code, 0x80, 0, 0, 0]);
        assert_eq!(
            6 + i32::from_le_bytes(forward[2..6].try_into().unwrap()) as usize,
            forward.len() - 1
        );
        let out = assemble(&format!("extern ext\n{mnemonic} ext"), &AMD64).unwrap();
        assert_eq!(out.sections[0].data, [0x0f, 0x80 + code, 0, 0, 0, 0]);
        let reloc = &out.sections[0].relocs[0];
        assert_eq!((reloc.offset, reloc.addend), (2, -4));
        assert_eq!(reloc.symbol, "ext");
        assert!(matches!(reloc.kind, RelocKind::Branch32));
        let cross =
            assemble(&format!("{mnemonic} other\nsection .other\nother: ret"), &AMD64).unwrap();
        assert_eq!(cross.sections[0].data, [0x0f, 0x80 + code, 0, 0, 0, 0]);
        let reloc = &cross.sections[0].relocs[0];
        assert_eq!((reloc.offset, reloc.addend), (2, -4));
        assert_eq!(reloc.symbol, "other");
        assert!(matches!(reloc.kind, RelocKind::Branch32));
        for suffix in ["", " rax", " end, end"] {
            rejects(&format!("{mnemonic}{suffix}"));
        }
    }
}

#[test]
fn mixed_forward_backward_branches_decode_against_final_symbols_deterministically() {
    let mut source = String::new();
    for n in 0..90 {
        let mnemonic = CONDITIONS[n % CONDITIONS.len()].0;
        let target = (n * 13 + 7) % 90;
        source.push_str(&format!("label{n}: {mnemonic} label{target}\nresb {}\n", (n % 7) * 21));
    }
    let out = assemble(&source, &AMD64).unwrap();
    let code = &out.sections[0].data;
    assert!(out.sections[0].relocs.is_empty());
    for n in 0..90 {
        let at =
            out.symbols.iter().find(|s| s.name == format!("label{n}")).unwrap().offset as usize;
        let target = out
            .symbols
            .iter()
            .find(|s| s.name == format!("label{}", (n * 13 + 7) % 90))
            .unwrap()
            .offset as i64;
        let cc = CONDITIONS[n % CONDITIONS.len()].1;
        let decoded = if code[at] == 0x70 + cc {
            at as i64 + 2 + code[at + 1] as i8 as i64
        } else {
            assert_eq!(&code[at..at + 2], &[0x0f, 0x80 + cc]);
            at as i64 + 6 + i32::from_le_bytes(code[at + 2..at + 6].try_into().unwrap()) as i64
        };
        assert_eq!(decoded, target, "branch {n}");
    }
    assert_eq!(&assemble(&source, &AMD64).unwrap().sections[0].data, code);
}
