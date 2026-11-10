use amd64::AMD64;

use assembler::assemble;

fn bytes(source: &str) -> Vec<u8> {
    assemble(source, &AMD64).unwrap_or_else(|e| panic!("{source}: {e}")).sections.remove(0).data
}

fn rejects(source: &str) {
    assert!(assemble(source, &AMD64).is_err(), "accepted {source}");
}

#[test]
fn explicit_numeric_rel_abs_addresses_have_independent_exact_encodings() {
    for (source, expected) in [
        ("mov eax,[rel 0]", vec![0x8b, 0x05, 0, 0, 0, 0]),
        ("mov r9,[rel -128]", vec![0x4c, 0x8b, 0x0d, 0x80, 0xff, 0xff, 0xff]),
        ("mov eax,[abs 0]", vec![0x8b, 0x04, 0x25, 0, 0, 0, 0]),
        ("mov r9,[abs -128]", vec![0x4c, 0x8b, 0x0c, 0x25, 0x80, 0xff, 0xff, 0xff]),
        ("mov word [abs 4660],ax", vec![0x66, 0x89, 0x04, 0x25, 0x34, 0x12, 0, 0]),
        ("mov byte [rel 0],1", vec![0xc6, 0x05, 0, 0, 0, 0, 1]),
        ("lea rax,[abs -1]", vec![0x48, 0x8d, 0x04, 0x25, 0xff, 0xff, 0xff, 0xff]),
        ("lea rax,[rel 8]", vec![0x48, 0x8d, 0x05, 8, 0, 0, 0]),
        ("N equ 4660\nmov eax,[abs N]", vec![0x8b, 0x04, 0x25, 0x34, 0x12, 0, 0]),
    ] {
        assert_eq!(bytes(source), expected, "{source}");
    }
    for mode in ["rel", "abs"] {
        for value in [i32::MIN, i32::MAX] {
            let b = bytes(&format!("mov eax,[{mode} {value}]"));
            assert_eq!(&b[b.len() - 4..], value.to_le_bytes());
        }
        for bad in ["2147483648", "-2147483649", "4294967295", "18446744073709551615"] {
            rejects(&format!("mov eax,[{mode} {bad}]"));
        }
    }
}

#[test]
fn defaults_remain_unchanged_and_mode_qualifiers_do_not_eat_symbol_names() {
    assert_eq!(bytes("mov eax,[rbx]"), [0x8b, 0x03]);
    assert_eq!(bytes("mov eax,[rcx*4]"), [0x8b, 0x04, 0x8d, 0, 0, 0, 0]);
    for source in [
        "mov eax,[123]",
        "mov eax,[rel rax]",
        "mov eax,[abs rax+8]",
        "mov eax,[rel rcx*4]",
        "mov eax,[rel]",
        "mov eax,[abs]",
        "mov eax,[rel abs 0]",
        "mov eax,[abs rel 0]",
    ] {
        rejects(source);
    }
    for symbol in ["rel", "abs"] {
        let o = assemble(&format!("extern {symbol}\nmov eax,[{symbol}+4]"), &AMD64).unwrap();
        assert_eq!(o.sections[0].data, [0x8b, 0x05, 0, 0, 0, 0]);
        assert_eq!(o.sections[0].relocs[0].symbol, symbol);
    }
}

#[test]
fn symbol_addresses_keep_signed_absolute_and_pc_relative_intent_and_field_offsets() {
    use assembler::RelocKind;
    for (source, expected, offset, addend) in [
        ("mov rax,[abs external+8]", vec![0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0], 4, 8),
        ("mov [abs external-8],r9", vec![0x4c, 0x89, 0x0c, 0x25, 0, 0, 0, 0], 4, -8),
        ("mov word [abs external],128", vec![0x66, 0xc7, 0x04, 0x25, 0, 0, 0, 0, 0x80, 0], 4, 0),
        ("add qword [abs external+7],1", vec![0x48, 0x83, 0x04, 0x25, 0, 0, 0, 0, 1], 4, 7),
        ("imul r9d,[abs external]", vec![0x44, 0x0f, 0xaf, 0x0c, 0x25, 0, 0, 0, 0], 5, 0),
        ("movsx r9,byte [abs external-1]", vec![0x4c, 0x0f, 0xbe, 0x0c, 0x25, 0, 0, 0, 0], 5, -1),
        ("setne byte [abs external]", vec![0x0f, 0x95, 0x04, 0x25, 0, 0, 0, 0], 4, 0),
        ("call qword [abs external]", vec![0xff, 0x14, 0x25, 0, 0, 0, 0], 3, 0),
        ("jmp qword [abs external+4]", vec![0xff, 0x24, 0x25, 0, 0, 0, 0], 3, 4),
        ("lea r9,[abs external+16]", vec![0x4c, 0x8d, 0x0c, 0x25, 0, 0, 0, 0], 4, 16),
    ] {
        let output = assemble(&format!("extern external\n{source}"), &AMD64).unwrap();
        assert_eq!(output.sections[0].data, expected, "{source}");
        let reloc = &output.sections[0].relocs[0];
        assert_eq!(
            (reloc.offset, reloc.addend, reloc.symbol.as_str()),
            (offset, addend, "external"),
            "{source}"
        );
        assert!(matches!(reloc.kind, RelocKind::SignedAbsolute32), "{source}");
    }
    for (source, expected, offset, addend) in [
        ("mov rax,[external+8]", vec![0x48, 0x8b, 0x05, 0, 0, 0, 0], 3, 4),
        ("mov rax,[rel external+8]", vec![0x48, 0x8b, 0x05, 0, 0, 0, 0], 3, 4),
        ("mov byte [rel external+2],1", vec![0xc6, 0x05, 0, 0, 0, 0, 1], 2, -3),
        ("mov word [rel external],128", vec![0x66, 0xc7, 0x05, 0, 0, 0, 0, 0x80, 0], 3, -6),
        ("lea r9,[rel external]", vec![0x4c, 0x8d, 0x0d, 0, 0, 0, 0], 3, -4),
        ("call qword [rel external]", vec![0xff, 0x15, 0, 0, 0, 0], 2, -4),
    ] {
        let output = assemble(&format!("extern external\n{source}"), &AMD64).unwrap();
        assert_eq!(output.sections[0].data, expected, "{source}");
        let reloc = &output.sections[0].relocs[0];
        assert_eq!((reloc.offset, reloc.addend), (offset, addend), "{source}");
        assert!(matches!(reloc.kind, RelocKind::Relative32));
    }
    // A known local symbol still requires a relocation in an address, even when
    // its section-relative value happens to fit the displacement field.
    let output =
        assemble("target: ret\nmov eax,[abs target]\nmov eax,[rel target]", &AMD64).unwrap();
    assert!(matches!(output.sections[0].relocs[0].kind, RelocKind::SignedAbsolute32));
    assert!(matches!(output.sections[0].relocs[1].kind, RelocKind::Relative32));
}

#[test]
fn relocation_addends_keep_i64_range_and_pc_relative_tail_checks() {
    for addend in [i64::MIN, i64::MAX] {
        let operator = if addend < 0 { "-" } else { "+" };
        let magnitude = addend.unsigned_abs();
        let o =
            assemble(&format!("extern x\nmov eax,[abs x{operator}{magnitude}]"), &AMD64).unwrap();
        assert_eq!(o.sections[0].relocs[0].addend, addend);
    }
    for source in [
        "extern x\nmov eax,[abs x+9223372036854775808]",
        "extern x\nmov byte [rel x-9223372036854775804],1",
        "extern x\nmov eax,[abs x+rax]",
        "extern x\nmov eax,[rel x+rax]",
        "extern x\nmov eax,[x+rax]",
        "mov ah,[abs 2147483648]",
    ] {
        rejects(source);
    }
}
