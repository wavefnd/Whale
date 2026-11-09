//! Bytes follow the architectural opcode/prefix/ModRM tables, independent of
//! an external assembler or Whale's instruction tables.
use amd64::AMD64;

use assembler::{assemble, RelocKind};

fn bytes(src: &str) -> Vec<u8> {
    let out = assemble(src, &AMD64).unwrap_or_else(|e| panic!("{src}: {e}"));
    assert!(out.sections.iter().all(|s| s.relocs.is_empty()));
    out.sections.into_iter().flat_map(|s| s.data).collect()
}

fn reject(src: &str) {
    assert!(assemble(src, &AMD64).is_err(), "accepted {src}");
}

#[test]
fn register_and_memory_extensions_select_source_and_destination_widths() {
    for (src, expected) in [
        ("movzx ax,al", vec![0x66, 0x0f, 0xb6, 0xc0]),
        ("movzx eax,ah", vec![0x0f, 0xb6, 0xc4]),
        ("movzx rax,bl", vec![0x48, 0x0f, 0xb6, 0xc3]),
        ("movzx eax,bx", vec![0x0f, 0xb7, 0xc3]),
        ("movzx r8,r9w", vec![0x4d, 0x0f, 0xb7, 0xc1]),
        ("movzx eax,spl", vec![0x40, 0x0f, 0xb6, 0xc4]),
        ("movsx ax,ah", vec![0x66, 0x0f, 0xbe, 0xc4]),
        ("movsx eax,bl", vec![0x0f, 0xbe, 0xc3]),
        ("movsx r8,r9b", vec![0x4d, 0x0f, 0xbe, 0xc1]),
        ("movsx rax,bx", vec![0x48, 0x0f, 0xbf, 0xc3]),
        ("movsxd rax,ebx", vec![0x48, 0x63, 0xc3]),
        ("movsxd r8,r9d", vec![0x4d, 0x63, 0xc1]),
        ("movzx ax,byte [rsp]", vec![0x66, 0x0f, 0xb6, 0x04, 0x24]),
        ("movzx r8,word [r9+r12*2-128]", vec![0x4f, 0x0f, 0xb7, 0x44, 0x61, 0x80]),
        ("movsx eax,byte [rbp]", vec![0x0f, 0xbe, 0x45, 0]),
        ("movsxd rax,dword [r13]", vec![0x49, 0x63, 0x45, 0]),
    ] {
        assert_eq!(bytes(src), expected, "{src}");
    }
    for src in [
        "movzx",
        "movzx rax",
        "movzx rax,al,bl",
        "movzx al,bl",
        "movzx ax,bx",
        "movzx eax,ebx",
        "movsx rax,rbx",
        "movsx rax,ah",
        "movzx r8d,ah",
        "movsx r8w,bh",
        "movzx eax,[rax]",
        "movzx eax,qword [rax]",
        "movsxd eax,ebx",
        "movsxd rax,bx",
        "movsxd rax,qword [rax]",
        "movsxd [rax],eax",
        "movzx eax,1",
    ] {
        reject(src);
    }
}

const CONDITIONS: &[(&str, u8)] = &[
    ("o", 0),
    ("no", 1),
    ("b", 2),
    ("c", 2),
    ("nae", 2),
    ("ae", 3),
    ("nb", 3),
    ("nc", 3),
    ("e", 4),
    ("z", 4),
    ("ne", 5),
    ("nz", 5),
    ("be", 6),
    ("na", 6),
    ("a", 7),
    ("nbe", 7),
    ("s", 8),
    ("ns", 9),
    ("p", 10),
    ("pe", 10),
    ("np", 11),
    ("po", 11),
    ("l", 12),
    ("nge", 12),
    ("ge", 13),
    ("nl", 13),
    ("le", 14),
    ("ng", 14),
    ("g", 15),
    ("nle", 15),
];

#[test]
fn every_setcc_alias_encodes_byte_register_and_memory_results() {
    for (suffix, cc) in CONDITIONS {
        for (reg, prefix, code) in [
            ("al", vec![], 0),
            ("ah", vec![], 4),
            ("spl", vec![0x40], 4),
            ("dil", vec![0x40], 7),
            ("r8b", vec![0x41], 0),
            ("r15b", vec![0x41], 7),
        ] {
            let mut expected = prefix;
            expected.extend([0x0f, 0x90 + cc, 0xc0 + code]);
            assert_eq!(bytes(&format!("set{suffix} {reg}")), expected);
        }
        assert_eq!(
            bytes(&format!("set{suffix} byte [r12+r9*4+128]")),
            [0x43, 0x0f, 0x90 + cc, 0x84, 0x8c, 0x80, 0, 0, 0]
        );
        assert_eq!(bytes(&format!("set{suffix} [rbp]")), [0x0f, 0x90 + cc, 0x45, 0]);
        for operand in ["ax", "eax", "rax", "word [rax]", "qword [rax]", "[r8+ah]", "al,bl", ""] {
            reject(&format!("set{suffix} {operand}"));
        }
    }
}

#[test]
fn every_cmovcc_alias_encodes_matching_wide_registers_only() {
    for (suffix, cc) in CONDITIONS {
        for (dst, src, prefix, modrm) in [
            ("ax", "bx", vec![0x66], 0xc3),
            ("eax", "ebx", vec![], 0xc3),
            ("rax", "rbx", vec![0x48], 0xc3),
            ("r8w", "r9w", vec![0x66, 0x45], 0xc1),
            ("r8d", "r9d", vec![0x45], 0xc1),
            ("r8", "r9", vec![0x4d], 0xc1),
        ] {
            let mut expected = prefix;
            expected.extend([0x0f, 0x40 + cc, modrm]);
            assert_eq!(bytes(&format!("cmov{suffix} {dst},{src}")), expected);
        }
        for operands in [
            "al,bl",
            "rax,ebx",
            "eax,bx",
            "rax,[rbx]",
            "[rax],rbx",
            "rax,1",
            "rax",
            "rax,rbx,rcx",
            "",
        ] {
            reject(&format!("cmov{suffix} {operands}"));
        }
    }
}

#[test]
fn indirect_calls_and_jumps_use_long_mode_default_width_and_shared_addresses() {
    for (mnemonic, ext) in [("call", 2), ("jmp", 4)] {
        for (reg, rex, code) in
            [("rax", None, 0), ("rbp", None, 5), ("r8", Some(0x41), 0), ("r15", Some(0x41), 7)]
        {
            let mut expected = Vec::new();
            if let Some(rex) = rex {
                expected.push(rex);
            }
            expected.extend([0xff, 0xc0 + (ext << 3) + code]);
            assert_eq!(bytes(&format!("{mnemonic} {reg}")), expected);
        }
        assert_eq!(bytes(&format!("{mnemonic} qword [r12]")), [0x41, 0xff, (ext << 3) + 4, 0x24]);
        assert_eq!(
            bytes(&format!("{mnemonic} [r9+r12*8-1]")),
            [0x43, 0xff, 0x40 + (ext << 3) + 4, 0xe1, 0xff]
        );
        for operand in [
            "eax",
            "ax",
            "al",
            "dword [rax]",
            "word [rax]",
            "byte [rax]",
            "[rax+2147483648]",
            "rax,rbx",
            "",
        ] {
            reject(&format!("{mnemonic} {operand}"));
        }
    }
    assert_eq!(
        bytes("start:call local\njmp end\nlocal:ret\nend:ret"),
        [0xe8, 2, 0, 0, 0, 0xeb, 1, 0xc3, 0xc3]
    );
}

#[test]
fn new_symbol_memory_forms_keep_data_reference_relocation_intent() {
    let out=assemble("extern x\nmovzx eax,byte [x+2]\nmovsx r8,word [x]\nmovsxd rax,dword [x]\nsete byte [x]\ncall qword [x]\njmp [x]\ncall x",&AMD64).unwrap();
    let relocs = &out.sections[0].relocs;
    assert_eq!(
        relocs.iter().map(|r| (r.offset, r.addend)).collect::<Vec<_>>(),
        [(3, -2), (11, -4), (18, -4), (25, -4), (31, -4), (37, -4), (42, -4)]
    );
    assert!(relocs[..6].iter().all(|r| matches!(r.kind, RelocKind::Relative32)));
    assert!(matches!(relocs[6].kind, RelocKind::Branch32));
    for src in ["extern x\ncall [x+rax]", "extern x\nmovzx eax,byte [x+rax]"] {
        reject(src);
    }
}

#[test]
fn immediate_push_boundaries_sign_extend_without_truncating_or_changing_register_forms() {
    for (imm, expected) in [
        ("-128", vec![0x6a, 0x80]),
        ("127", vec![0x6a, 0x7f]),
        ("-129", vec![0x68, 0x7f, 0xff, 0xff, 0xff]),
        ("128", vec![0x68, 0x80, 0, 0, 0]),
        ("-2147483648", vec![0x68, 0, 0, 0, 0x80]),
        ("2147483647", vec![0x68, 0xff, 0xff, 0xff, 0x7f]),
    ] {
        assert_eq!(bytes(&format!("push {imm}")), expected);
    }
    assert_eq!(bytes("push ax\npush r8\npop r8"), [0x66, 0x50, 0x41, 0x50, 0x41, 0x58]);
    for src in [
        "push -2147483649",
        "push 2147483648",
        "push 4294967295",
        "push 18446744073709551615",
        "push 1,2",
        "push word 1",
        "push [rax]",
        "pop 1",
    ] {
        reject(src);
    }
}

#[test]
fn adc_and_sbb_use_carry_input_opcodes_for_all_scalar_widths() {
    for (mnemonic, byte, wide) in [("adc", 0x10, 0x11), ("sbb", 0x18, 0x19)] {
        for (dst, src, prefix, opcode, modrm) in [
            ("al", "bl", vec![], byte, 0xd8),
            ("ah", "bh", vec![], byte, 0xfc),
            ("ax", "bx", vec![0x66], wide, 0xd8),
            ("eax", "ebx", vec![], wide, 0xd8),
            ("rax", "rbx", vec![0x48], wide, 0xd8),
            ("spl", "dil", vec![0x40], byte, 0xfc),
            ("r8b", "r9b", vec![0x45], byte, 0xc8),
            ("r8w", "r9w", vec![0x66, 0x45], wide, 0xc8),
            ("r8d", "r9d", vec![0x45], wide, 0xc8),
            ("r8", "r9", vec![0x4d], wide, 0xc8),
        ] {
            let mut expected = prefix;
            expected.extend([opcode, modrm]);
            assert_eq!(bytes(&format!("{mnemonic} {dst},{src}")), expected);
        }
        for operands in [
            "rax,ebx",
            "al,bx",
            "ah,r8b",
            "spl,ah",
            "rax,1",
            "rax,[rbx]",
            "[rax],rbx",
            "rax",
            "rax,rbx,rcx",
            "",
        ] {
            reject(&format!("{mnemonic} {operands}"));
        }
    }
}
