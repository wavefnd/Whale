use amd64::AMD64;

use assembler::{assemble, RelocKind};

fn bytes(source: &str) -> Vec<u8> {
    assemble(source, &AMD64).unwrap_or_else(|e| panic!("{source}: {e}")).sections.remove(0).data
}

fn rejects(source: &str) {
    assert!(assemble(source, &AMD64).is_err(), "accepted {source}");
}

#[test]
fn scalar_moves_have_exact_mandatory_prefixes_directions_and_rex_extensions() {
    for (source, expected) in [
        ("movss xmm1,xmm2", vec![0xf3, 0x0f, 0x10, 0xca]),
        ("movsd xmm1,xmm2", vec![0xf2, 0x0f, 0x10, 0xca]),
        ("movss xmm9,xmm10", vec![0xf3, 0x45, 0x0f, 0x10, 0xca]),
        ("movsd xmm9,xmm10", vec![0xf2, 0x45, 0x0f, 0x10, 0xca]),
        ("movss xmm0,[rax]", vec![0xf3, 0x0f, 0x10, 0x00]),
        ("movsd xmm0,[rax]", vec![0xf2, 0x0f, 0x10, 0x00]),
        ("movss [rax],xmm1", vec![0xf3, 0x0f, 0x11, 0x08]),
        ("movsd [rax],xmm1", vec![0xf2, 0x0f, 0x11, 0x08]),
        ("movss xmm15,dword [r13+8]", vec![0xf3, 0x45, 0x0f, 0x10, 0x7d, 8]),
        ("movsd qword [r13+8],xmm15", vec![0xf2, 0x45, 0x0f, 0x11, 0x7d, 8]),
        ("movsd xmm8,[r12+r9*8-128]", vec![0xf2, 0x47, 0x0f, 0x10, 0x44, 0xcc, 0x80]),
        ("movss [rcx*4],xmm9", vec![0xf3, 0x44, 0x0f, 0x11, 0x0c, 0x8d, 0, 0, 0, 0]),
    ] {
        assert_eq!(bytes(source), expected, "{source}");
    }
}

#[test]
fn scalar_arithmetic_covers_every_operation_width_pair_with_reg_and_memory_sources() {
    for (mnemonic, expected_register, expected_memory) in [
        (
            "addss",
            vec![0xf3, 0x45, 0x0f, 0x58, 0xca],
            vec![0xf3, 0x47, 0x0f, 0x58, 0x4c, 0xcc, 0x80],
        ),
        (
            "addsd",
            vec![0xf2, 0x45, 0x0f, 0x58, 0xca],
            vec![0xf2, 0x47, 0x0f, 0x58, 0x4c, 0xcc, 0x80],
        ),
        (
            "subss",
            vec![0xf3, 0x45, 0x0f, 0x5c, 0xca],
            vec![0xf3, 0x47, 0x0f, 0x5c, 0x4c, 0xcc, 0x80],
        ),
        (
            "subsd",
            vec![0xf2, 0x45, 0x0f, 0x5c, 0xca],
            vec![0xf2, 0x47, 0x0f, 0x5c, 0x4c, 0xcc, 0x80],
        ),
        (
            "mulss",
            vec![0xf3, 0x45, 0x0f, 0x59, 0xca],
            vec![0xf3, 0x47, 0x0f, 0x59, 0x4c, 0xcc, 0x80],
        ),
        (
            "mulsd",
            vec![0xf2, 0x45, 0x0f, 0x59, 0xca],
            vec![0xf2, 0x47, 0x0f, 0x59, 0x4c, 0xcc, 0x80],
        ),
        (
            "divss",
            vec![0xf3, 0x45, 0x0f, 0x5e, 0xca],
            vec![0xf3, 0x47, 0x0f, 0x5e, 0x4c, 0xcc, 0x80],
        ),
        (
            "divsd",
            vec![0xf2, 0x45, 0x0f, 0x5e, 0xca],
            vec![0xf2, 0x47, 0x0f, 0x5e, 0x4c, 0xcc, 0x80],
        ),
    ] {
        assert_eq!(bytes(&format!("{mnemonic} xmm9,xmm10")), expected_register, "{mnemonic}");
        assert_eq!(
            bytes(&format!("{mnemonic} xmm9,[r12+r9*8-128]")),
            expected_memory,
            "{mnemonic}"
        );
    }
}

#[test]
fn wrong_classes_widths_and_unselected_vector_forms_are_rejected() {
    for mnemonic in
        ["movss", "movsd", "addss", "addsd", "subss", "subsd", "mulss", "mulsd", "divss", "divsd"]
    {
        for operands in [
            "",
            " xmm0",
            " xmm0,xmm1,xmm2",
            " eax,xmm1",
            " xmm0,rax",
            " xmm0,1",
            " xmm0,xmm16",
            " xmm16,xmm0",
            " [rax],[rbx]",
            " xmm0,[xmm1]",
            " xmm0,[eax]",
            " xmm0,word [rax]",
        ] {
            rejects(&format!("{mnemonic}{operands}"));
        }
    }
    for source in [
        "movss xmm0,qword [rax]",
        "movsd xmm0,dword [rax]",
        "movss qword [rax],xmm0",
        "movsd dword [rax],xmm0",
        "addss [rax],xmm0",
        "addsd xmm0,dword [rax]",
        "movss xmm0,oword [rax]",
        "movaps xmm0,xmm1",
        "addps xmm0,xmm1",
        "vaddss xmm0,xmm1,xmm2",
        "movd xmm0,eax",
    ] {
        rejects(source);
    }
}

#[test]
fn scalar_memory_addresses_keep_relocation_fields_and_signed_absolute_intent() {
    for (source, expected, offset, addend, absolute) in [
        ("movss xmm0,[external]", vec![0xf3, 0x0f, 0x10, 0x05, 0, 0, 0, 0], 4, -4, false),
        (
            "movsd xmm8,[rel external+8]",
            vec![0xf2, 0x44, 0x0f, 0x10, 0x05, 0, 0, 0, 0],
            5,
            4,
            false,
        ),
        (
            "movss [abs external+4],xmm9",
            vec![0xf3, 0x44, 0x0f, 0x11, 0x0c, 0x25, 0, 0, 0, 0],
            6,
            4,
            true,
        ),
        (
            "movsd xmm15,[abs external-8]",
            vec![0xf2, 0x44, 0x0f, 0x10, 0x3c, 0x25, 0, 0, 0, 0],
            6,
            -8,
            true,
        ),
        ("divsd xmm9,[rel external]", vec![0xf2, 0x44, 0x0f, 0x5e, 0x0d, 0, 0, 0, 0], 5, -4, false),
        ("addss xmm1,[abs external]", vec![0xf3, 0x0f, 0x58, 0x0c, 0x25, 0, 0, 0, 0], 5, 0, true),
    ] {
        let o = assemble(&format!("extern external\n{source}"), &AMD64).unwrap();
        assert_eq!(o.sections[0].data, expected, "{source}");
        assert_eq!(o.sections[0].relocs.len(), 1);
        let r = &o.sections[0].relocs[0];
        assert_eq!(
            (r.offset, r.addend, r.symbol.as_str()),
            (offset, addend, "external"),
            "{source}"
        );
        assert!(
            if absolute {
                matches!(r.kind, RelocKind::SignedAbsolute32)
            } else {
                matches!(r.kind, RelocKind::Relative32)
            },
            "{source}"
        );
    }
    assert_eq!(bytes("movss xmm0,[rel -4]"), [0xf3, 0x0f, 0x10, 0x05, 0xfc, 0xff, 0xff, 0xff]);
    assert_eq!(bytes("movsd [abs 4660],xmm0"), [0xf2, 0x0f, 0x11, 0x04, 0x25, 0x34, 0x12, 0, 0]);
    for source in [
        "movss xmm0,[abs 2147483648]",
        "movsd xmm0,[rel -2147483649]",
        "extern x\nmovss xmm0,[abs x+rax]",
    ] {
        rejects(source);
    }
}

#[test]
fn every_xmm_name_has_its_own_register_code_without_hijacking_adjacent_identifiers() {
    // Architectural /r register numbers for all sixteen XMM names. This also
    // checks both directions of the REX.R/REX.B extension boundary.
    for (name, load, source) in [
        ("xmm0", vec![0xf3, 0x0f, 0x10, 0xc0], vec![0xf3, 0x0f, 0x10, 0xc0]),
        ("xmm1", vec![0xf3, 0x0f, 0x10, 0xc8], vec![0xf3, 0x0f, 0x10, 0xc1]),
        ("xmm2", vec![0xf3, 0x0f, 0x10, 0xd0], vec![0xf3, 0x0f, 0x10, 0xc2]),
        ("xmm3", vec![0xf3, 0x0f, 0x10, 0xd8], vec![0xf3, 0x0f, 0x10, 0xc3]),
        ("xmm4", vec![0xf3, 0x0f, 0x10, 0xe0], vec![0xf3, 0x0f, 0x10, 0xc4]),
        ("xmm5", vec![0xf3, 0x0f, 0x10, 0xe8], vec![0xf3, 0x0f, 0x10, 0xc5]),
        ("xmm6", vec![0xf3, 0x0f, 0x10, 0xf0], vec![0xf3, 0x0f, 0x10, 0xc6]),
        ("xmm7", vec![0xf3, 0x0f, 0x10, 0xf8], vec![0xf3, 0x0f, 0x10, 0xc7]),
        ("xmm8", vec![0xf3, 0x44, 0x0f, 0x10, 0xc0], vec![0xf3, 0x41, 0x0f, 0x10, 0xc0]),
        ("xmm9", vec![0xf3, 0x44, 0x0f, 0x10, 0xc8], vec![0xf3, 0x41, 0x0f, 0x10, 0xc1]),
        ("xmm10", vec![0xf3, 0x44, 0x0f, 0x10, 0xd0], vec![0xf3, 0x41, 0x0f, 0x10, 0xc2]),
        ("xmm11", vec![0xf3, 0x44, 0x0f, 0x10, 0xd8], vec![0xf3, 0x41, 0x0f, 0x10, 0xc3]),
        ("xmm12", vec![0xf3, 0x44, 0x0f, 0x10, 0xe0], vec![0xf3, 0x41, 0x0f, 0x10, 0xc4]),
        ("xmm13", vec![0xf3, 0x44, 0x0f, 0x10, 0xe8], vec![0xf3, 0x41, 0x0f, 0x10, 0xc5]),
        ("xmm14", vec![0xf3, 0x44, 0x0f, 0x10, 0xf0], vec![0xf3, 0x41, 0x0f, 0x10, 0xc6]),
        ("xmm15", vec![0xf3, 0x44, 0x0f, 0x10, 0xf8], vec![0xf3, 0x41, 0x0f, 0x10, 0xc7]),
    ] {
        assert_eq!(bytes(&format!("movss {name},xmm0")), load, "{name}");
        assert_eq!(bytes(&format!("movss xmm0,{name}")), source, "{name}");
    }
    assert_eq!(bytes("MOVSD XMM0,XMM1"), [0xf2, 0x0f, 0x10, 0xc1]);
    assert_eq!(
        bytes("xmm_data equ 7\nxmm16 equ 9\nxmm0 equ 11\ndq xmm_data,xmm16,xmm0"),
        [7u64, 9, 11].into_iter().flat_map(u64::to_le_bytes).collect::<Vec<_>>()
    );
    let o = assemble("extern xmm16\nmov eax,xmm16", &AMD64).unwrap();
    assert_eq!(o.sections[0].data, [0xb8, 0, 0, 0, 0]);
    assert!(matches!(o.sections[0].relocs[0].kind, RelocKind::Absolute32));
    assert_eq!(o.sections[0].relocs[0].symbol, "xmm16");
    for source in
        ["mov xmm0,xmm1", "mov rax,xmm0", "add xmm0,xmm1", "movss xmm0,xmm01", "movss xmm-1,xmm0"]
    {
        rejects(source);
    }
}
