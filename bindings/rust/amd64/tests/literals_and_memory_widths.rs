use amd64::AMD64;

use assembler::{assemble, RelocKind};

fn bytes(source: &str) -> Vec<u8> {
    assemble(source, &AMD64)
        .unwrap_or_else(|e| panic!("{source}: {e}"))
        .sections
        .into_iter()
        .flat_map(|s| s.data)
        .collect()
}

fn error(source: &str) -> String {
    assemble(source, &AMD64).err().unwrap_or_else(|| panic!("accepted {source}")).to_string()
}

#[test]
fn all_64_bit_literal_patterns_and_signed_minimum_are_lossless() {
    for literal in [
        "18446744073709551615",
        "0xffffffffffffffff",
        "0XFFFFFFFFFFFFFFFF",
        "0b1111111111111111111111111111111111111111111111111111111111111111",
    ] {
        assert_eq!(bytes(&format!("dq {literal}")), u64::MAX.to_le_bytes());
        assert_eq!(
            bytes(&format!("mov rax, {literal}")),
            [vec![0x48, 0xb8], u64::MAX.to_le_bytes().to_vec()].concat()
        );
        assert_eq!(
            bytes(&format!("N equ 0\nM equ N + {literal}\ndq M\nmov rax, N + {literal}")),
            [u64::MAX.to_le_bytes().to_vec(), vec![0x48, 0xb8], u64::MAX.to_le_bytes().to_vec()]
                .concat()
        );
    }
    assert_eq!(
        bytes("dq -9223372036854775808, 9223372036854775808"),
        [i64::MIN.to_le_bytes(), (1u64 << 63).to_le_bytes()].concat()
    );
    for src in [
        "dq 18446744073709551616",
        "dq 0x10000000000000000",
        "dq -9223372036854775809",
        "dq 18446744073709551615 + 1",
        "mov eax, 4294967296",
        "dw 65536",
        "db 256",
        "add rax, 4294967295",
        "extern x\ndq x + 18446744073709551615",
        "extern x\nmov rax, x + 18446744073709551615",
    ] {
        error(src);
    }
}

#[test]
fn scaled_first_register_becomes_index_and_displacements_remain_signed() {
    for (src, expected) in [
        ("mov rax, [rcx*4]", vec![0x48, 0x8b, 0x04, 0x8d, 0, 0, 0, 0]),
        ("mov rax, [r9*8+16]", vec![0x4a, 0x8b, 0x04, 0xcd, 16, 0, 0, 0]),
        ("mov rax, [r12*2-128]", vec![0x4a, 0x8b, 0x04, 0x65, 0x80, 0xff, 0xff, 0xff]),
        ("mov rax, [rcx*4 + rbx]", vec![0x48, 0x8b, 0x04, 0x8b]),
        ("mov rax, [rbx+rcx*4]", vec![0x48, 0x8b, 0x04, 0x8b]),
    ] {
        assert_eq!(bytes(src), expected, "{src}");
    }
    for expression in [
        "rcx*0",
        "rcx*3",
        "rcx*16",
        "rsp*4",
        "rcx*4 + rdx*2",
        "rcx*4 + rax + rbx",
        "r9*8+2147483648",
    ] {
        error(&format!("mov rax, [{expression}]"));
    }
    assert!(error("mov rax, [rax - 9223372036854775808]").contains("displacement"));
}

#[test]
fn explicit_width_stores_and_memory_arithmetic_use_exact_prefix_and_immediate_sizes() {
    for (src, expected) in [
        ("mov byte [rax], 18", vec![0xc6, 0x00, 0x12]),
        ("mov word [r8], 4660", vec![0x66, 0x41, 0xc7, 0x00, 0x34, 0x12]),
        ("mov dword [rsp+8], 4294967295", vec![0xc7, 0x44, 0x24, 8, 0xff, 0xff, 0xff, 0xff]),
        ("mov qword [r13], -2147483648", vec![0x49, 0xc7, 0x45, 0, 0, 0, 0, 0x80]),
        ("add byte [r9+rcx*4-1], 255", vec![0x41, 0x80, 0x44, 0x89, 0xff, 0xff]),
        ("add qword [r12], 127", vec![0x49, 0x83, 0x04, 0x24, 0x7f]),
        ("sub word [rsp], 128", vec![0x66, 0x81, 0x2c, 0x24, 0x80, 0]),
        ("xor dword [rbp], -129", vec![0x81, 0x75, 0, 0x7f, 0xff, 0xff, 0xff]),
        ("cmp qword [r15+127], -128", vec![0x49, 0x83, 0x7f, 0x7f, 0x80]),
        ("mov eax, dword [rbx]", vec![0x8b, 0x03]),
        ("mov byte [r8], spl", vec![0x41, 0x88, 0x20]),
    ] {
        assert_eq!(bytes(src), expected, "{src}");
    }
    for src in [
        "mov [rax], 1",
        "add [rax], 1",
        "mov byte [rax],256",
        "mov word [rax],65536",
        "mov dword [rax],4294967296",
        "mov qword [rax],2147483648",
        "mov qword [rax],18446744073709551615",
        "add qword [rax],-2147483649",
        "mov rax,dword [rbx]",
        "mov word [rax],eax",
        "add eax,byte [rbx]",
        "imul ax, qword [rbx]",
        "mov byte rax,1",
    ] {
        error(src);
    }
}

#[test]
fn rip_relative_immediate_memory_relocations_include_trailing_immediate_bytes() {
    let out = assemble(
        "extern x\nmov byte [x+2],1\nmov word [x],128\nadd qword [x],1\nmov qword [x],-1",
        &AMD64,
    )
    .unwrap();
    assert_eq!(
        out.sections[0].data,
        [
            0xc6, 0x05, 0, 0, 0, 0, 1, 0x66, 0xc7, 0x05, 0, 0, 0, 0, 0x80, 0, 0x48, 0x83, 0x05, 0,
            0, 0, 0, 1, 0x48, 0xc7, 0x05, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff
        ]
    );
    let relocs = &out.sections[0].relocs;
    assert_eq!(
        relocs.iter().map(|r| (r.offset, r.addend)).collect::<Vec<_>>(),
        [(2, -3), (10, -6), (19, -5), (27, -8)]
    );
    assert!(relocs.iter().all(|r| r.symbol == "x" && matches!(r.kind, RelocKind::Relative32)));
}

#[test]
fn db_escapes_are_bytes_while_source_text_is_utf8() {
    assert_eq!(
        bytes(r#"db "é\n\r\t\0\"\\\x00\xff""#),
        [0xc3, 0xa9, 10, 13, 9, 0, b'"', b'\\', 0, 255]
    );
    assert_eq!(bytes(r#"db "a", "", "\\n", 255"#), [b'a', b'\\', b'n', 255]);
    for src in [r#"db "\q""#, r#"db "\x""#, r#"db "\x0""#, r#"db "\xGG""#, "db \"unfinished\\"] {
        let e = error(src);
        assert!(e.contains("escape") && e.contains("column"), "{e}");
    }
    assert!(error("; comment\ndb \"é\\q\"").contains("line 2, column 6"));
    assert!(error(r#"dw "\x00""#).contains("numeric expressions"));
}

#[test]
fn full_width_intermediates_narrow_only_after_the_expression() {
    for max in ["18446744073709551615", "0xffffffffffffffff"] {
        for expression in [
            format!("{max} - {max}"),
            format!("-{max} + {max}"),
            format!("0 - {max} + {max}"),
            format!("{max} + {max} - {max} - {max}"),
            "-9223372036854775808 - -9223372036854775808".into(),
        ] {
            assert_eq!(bytes(&format!("dq {expression}")), 0u64.to_le_bytes(), "{expression}");
        }
        assert_eq!(bytes(&format!("dq {max} + 1 - 1")), u64::MAX.to_le_bytes());
        assert_eq!(bytes(&format!("N equ {max}\nM equ N - {max}\ndq M")), 0u64.to_le_bytes());
        assert_eq!(
            bytes(&format!("N equ {max}\nM equ N - {max} - {max} + {max}\ndq M")),
            0u64.to_le_bytes()
        );
        assert_eq!(
            bytes(&format!("N equ -9223372036854775808\nM equ N + {max}\ndq M")),
            i64::MAX.to_le_bytes()
        );
        assert_eq!(
            bytes(&format!("N equ {max}\nM equ N - 9223372036854775807\ndq M")),
            (1u64 << 63).to_le_bytes()
        );
    }
}

#[test]
fn wider_expression_intermediates_do_not_relax_final_or_address_ranges() {
    for source in [
        "dq -18446744073709551615",
        "dq 18446744073709551615 + 1",
        "dq -9223372036854775808 - 1",
        "N equ 0\nM equ N - 18446744073709551615\ndq M",
        "extern x\ndq x + 18446744073709551615",
        "extern x\ndq x - 18446744073709551615",
        "extern x\ndq x + 9223372036854775808",
        "mov rax,[rbx+2147483648]",
        "mov rax,[rbx-9223372036854775808]",
        "extern x\ncall x - 9223372036854775808",
    ] {
        error(source);
    }
    for (addend, expected) in
        [("9223372036854775807", i64::MAX), ("-9223372036854775808", i64::MIN)]
    {
        let source =
            format!("extern x\ndq x + 18446744073709551615 - 18446744073709551615 + {addend}");
        let output = assemble(&source, &AMD64).unwrap();
        assert_eq!(output.sections[0].relocs[0].addend, expected);
    }
}
