use amd64::AMD64;

use assembler::ast::MemoryOperand;

use amd64::encoding::encode_address;

use assembler::assemble;

fn text(source: &str) -> Vec<u8> {
    assemble(source, &AMD64)
        .expect("valid assembly")
        .sections
        .into_iter()
        .find(|s| s.name == ".text")
        .unwrap()
        .data
}

fn error(source: &str) -> String {
    assemble(source, &AMD64).err().expect("invalid assembly must fail").to_string()
}

#[test]
fn r12_sib_indices_keep_rex_x_for_every_scale_and_extended_bases() {
    for (scale, bits) in [(1, 0), (2, 1), (4, 2), (8, 3)] {
        for (base, rex) in [("rbx", 0x4a), ("r11", 0x4b)] {
            assert_eq!(
                text(&format!("mov rax, [{base} + r12*{scale}]")),
                [rex, 0x8b, 0x04, (bits << 6) | 0x23]
            );
        }
        assert!(error(&format!("mov rax, [rbx + rsp*{scale}]")).contains("index register"));
    }
}

#[test]
fn displacements_preserve_signed_boundaries_and_encoding_transitions() {
    for (disp, expected) in [
        (0, vec![0x48, 0x8b, 0x03]),
        (-128, vec![0x48, 0x8b, 0x43, 0x80]),
        (127, vec![0x48, 0x8b, 0x43, 0x7f]),
        (-129, vec![0x48, 0x8b, 0x83, 0x7f, 0xff, 0xff, 0xff]),
        (128, vec![0x48, 0x8b, 0x83, 0x80, 0, 0, 0]),
        (i32::MIN as i64, vec![0x48, 0x8b, 0x83, 0, 0, 0, 0x80]),
        (i32::MAX as i64, vec![0x48, 0x8b, 0x83, 0xff, 0xff, 0xff, 0x7f]),
    ] {
        let expression =
            if disp < 0 { format!("rbx - {}", -disp) } else { format!("rbx + {disp}") };
        assert_eq!(text(&format!("mov rax, [{expression}]")), expected);
    }
    for disp in [i32::MIN as i64 - 1, i32::MAX as i64 + 1, 4294967296] {
        let expression =
            if disp < 0 { format!("rbx - {}", -disp) } else { format!("rbx + {disp}") };
        assert!(error(&format!("mov rax, [{expression}]")).contains("displacement"));
        // The public address encoder also accepts base-less SIB operands.
        let mem = MemoryOperand {
            address_mode: Default::default(),
            width: None,
            base: None,
            index: Some("rcx".into()),
            scale: 4,
            disp,
            symbol: None,
        };
        assert!(encode_address(&mem, 64).err().unwrap().to_string().contains("displacement"));
    }
}

#[test]
fn unclosed_memory_at_eof_and_newline_has_a_located_error() {
    for source in ["mov rax, [rbx", "mov rax, [rbx\n", "mov rax, ["] {
        let message = error(source);
        assert!(message.contains("closing ']'"), "{message}");
        assert!(message.contains("line 1"), "{message}");
    }
    assert_eq!(text("mov rax, [rbx]"), text("mov rax, [rbx]\n"));
}

#[test]
fn unexpected_top_level_tokens_are_not_skipped() {
    for token in ["]", ",", "123", "+", "["] {
        let message = error(&format!("; comment\n{token}\nnop\n"));
        assert!(message.contains("Unexpected token"), "{message}");
        assert!(message.contains("line 2, column 1"), "{message}");
    }
    assert_eq!(text("; comment\n\nstart: nop\n; last\n"), [0x90]);
}

#[test]
fn memory_terms_must_have_valid_separators_and_representable_signs() {
    for expression in [
        "rbx - rcx",
        "rbx - symbol",
        "-rbx",
        "-symbol",
        "rbx +",
        "rbx -",
        "rbx ++ 8",
        "rbx -- 8",
        "rbx + -8",
        "rbx rcx",
        "rbx 8",
        "",
        "rbx + rcx*3",
        "rbx + rcx*",
        "rbx + rcx + rdx",
    ] {
        let message = error(&format!("mov rax, [{expression}]"));
        assert!(message.contains("Parser error"), "{expression}: {message}");
    }
    for (expression, expected) in [
        ("rbx + rcx*4 - 8", vec![0x48, 0x8b, 0x44, 0x8b, 0xf8]),
        ("rbx + rcx*4 + 8", vec![0x48, 0x8b, 0x44, 0x8b, 0x08]),
        ("-8 + rbx + rcx*4", vec![0x48, 0x8b, 0x44, 0x8b, 0xf8]),
        ("rbx - 8 + 16", vec![0x48, 0x8b, 0x43, 0x08]),
    ] {
        assert_eq!(text(&format!("mov rax, [{expression}]")), expected);
    }
    assert!(error("mov rax, [rbx + 9223372036854775807 + 1]").contains("overflow"));
    assert!(error("mov rax, [rbx - 9223372036854775807 - 2]").contains("overflow"));
}

#[test]
fn fixed_instructions_reject_operands_instead_of_discarding_them() {
    for (mnemonic, bytes) in [
        ("nop", vec![0x90]),
        ("ret", vec![0xc3]),
        ("syscall", vec![0x0f, 0x05]),
        ("int3", vec![0xcc]),
    ] {
        assert_eq!(text(mnemonic), bytes);
        for operand in ["1", "rax", "rax, rbx"] {
            assert!(error(&format!("{mnemonic} {operand}"))
                .contains(&format!("{mnemonic} expects 0 operands")));
        }
    }
}
