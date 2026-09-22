pub mod assembler;
pub mod ast;
pub mod error;
pub mod isa;
pub mod tokens;
pub mod traits;

pub use assembler::*;

#[cfg(test)]
mod tests {
    use crate::assemble;
    use crate::isa::AMD64;
    use crate::tokens::tokenize;
    use crate::RelocKind;

    #[test]
    fn resolves_local_labels_to_parent_scope() {
        let src = r#"
section .text
global _start
_start:
    jmp .L1
.L1:
    ret
"#;
        let out = assemble(src, &AMD64).expect("assemble should succeed");
        let has_scoped = out.symbols.iter().any(|s| s.name == "_start.L1");
        assert!(has_scoped, "expected scoped local label _start.L1");
    }

    #[test]
    fn rejects_duplicate_label_definition() {
        let src = r#"
section .text
foo:
    nop
foo:
    ret
"#;
        let res = assemble(src, &AMD64);
        let msg = match res {
            Ok(_) => panic!("duplicate labels must fail"),
            Err(e) => format!("{e}"),
        };
        assert!(msg.contains("Duplicate label definition"), "{msg}");
    }

    #[test]
    fn rejects_undefined_symbol_without_extern() {
        let src = r#"
section .text
global _start
_start:
    call missing_symbol
    ret
"#;
        let res = assemble(src, &AMD64);
        let msg = match res {
            Ok(_) => panic!("undefined symbols must fail"),
            Err(e) => format!("{e}"),
        };
        assert!(msg.contains("Undefined symbol 'missing_symbol'"), "{msg}");
    }

    #[test]
    fn allows_undefined_symbol_with_extern() {
        let src = r#"
section .text
extern ext
global _start
_start:
    call ext
    ret
"#;
        let out = assemble(src, &AMD64).expect("extern symbols should be allowed");
        let has_ext = out
            .symbols
            .iter()
            .any(|s| s.name == "ext" && s.section_index.is_none());
        assert!(has_ext, "extern symbol should be present as undefined");
    }

    #[test]
    fn lexer_error_contains_line_and_column() {
        let src = "section .text\n@";
        let err = tokenize(src).expect_err("lexer must fail on '@'");
        let msg = format!("{err}");
        assert!(msg.contains("line 2, column 1"), "{msg}");
    }

    #[test]
    fn parser_error_contains_line_and_column() {
        let src = "section .text\nmov rax, ]";
        let res = assemble(src, &AMD64);
        let msg = match res {
            Ok(_) => panic!("parser must fail on invalid operand"),
            Err(e) => format!("{e}"),
        };
        assert!(msg.contains("line 2"), "{msg}");
    }

    #[test]
    fn equ_expression_resolves_in_instruction_immediate() {
        let src = r#"
section .text
global _start
VAL equ 5
_start:
    mov eax, VAL + 3
    ret
"#;
        let out = assemble(src, &AMD64).expect("assemble should succeed");
        let text = out
            .sections
            .iter()
            .find(|s| s.name == ".text")
            .expect("text section");
        assert_eq!(text.data, vec![0xB8, 0x08, 0x00, 0x00, 0x00, 0xC3]);
        assert!(
            text.relocs.is_empty(),
            "equ immediate must not create relocations"
        );
    }

    #[test]
    fn dd_symbol_addend_emits_absolute32_relocation() {
        let src = r#"
section .data
extern extdata
ptr32:
    dd extdata + 4
"#;
        let out = assemble(src, &AMD64).expect("assemble should succeed");
        let data = out
            .sections
            .iter()
            .find(|s| s.name == ".data")
            .expect("data section");
        assert_eq!(data.data, vec![0, 0, 0, 0]);
        assert_eq!(data.relocs.len(), 1);
        let r = &data.relocs[0];
        assert_eq!(r.offset, 0);
        assert_eq!(r.symbol, "extdata");
        assert_eq!(r.addend, 4);
        assert!(matches!(r.kind, RelocKind::Absolute32));
    }

    #[test]
    fn jump_relaxation_chooses_near_and_short_without_relocations() {
        let src = r#"
section .text
global _start
_start:
    jmp far_target
    dq 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    dq 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
near_src:
    je near_dst
    nop
near_dst:
    loop near_src
far_target:
    ret
"#;
        let out = assemble(src, &AMD64).expect("assemble should succeed");
        let text = out
            .sections
            .iter()
            .find(|s| s.name == ".text")
            .expect("text section");

        assert!(
            text.relocs.is_empty(),
            "local same-section jumps must not use relocations"
        );
        assert_eq!(text.data[0], 0xE9, "jmp must be near opcode");
        let jmp_disp = i32::from_le_bytes([text.data[1], text.data[2], text.data[3], text.data[4]]);
        assert_eq!(jmp_disp, 165);

        assert_eq!(text.data[165], 0x74, "je must relax to short");
        assert_eq!(text.data[166], 0x01);
        assert_eq!(text.data[168], 0xE2);
        assert_eq!(text.data[169], 0xFB);
        assert_eq!(text.data[170], 0xC3);
    }

    fn eval_imm32(expr: &str) -> i32 {
        let src = format!("section .text\nglobal _start\n_start:\n    mov eax, {expr}\n    ret\n");
        let out = assemble(&src, &AMD64).expect("assemble should succeed");
        let text = out
            .sections
            .iter()
            .find(|s| s.name == ".text")
            .expect("text section");
        i32::from_le_bytes([text.data[1], text.data[2], text.data[3], text.data[4]])
    }

    #[test]
    fn unary_plus_after_infix_minus_subtracts() {
        assert_eq!(eval_imm32("10 - +2"), 8);
    }

    #[test]
    fn unary_minus_after_infix_minus_adds() {
        assert_eq!(eval_imm32("10 - -2"), 12);
    }

    #[test]
    fn double_unary_minus_is_positive() {
        assert_eq!(eval_imm32("--2"), 2);
    }

    #[test]
    fn unary_minus_then_plus_is_negative() {
        assert_eq!(eval_imm32("-+2"), -2);
    }

    #[test]
    fn leading_unary_minus_negates() {
        assert_eq!(eval_imm32("-3"), -3);
    }

    #[test]
    fn unary_chain_following_infix_plus() {
        assert_eq!(eval_imm32("10 + -+2"), 8);
        assert_eq!(eval_imm32("10 + --2"), 12);
        assert_eq!(eval_imm32("10 + -2"), 8);
    }

    #[test]
    fn dd_unary_sign_composition_emits_expected_bytes() {
        let src = r#"
section .data
dd 10 - +2
dd 10 - -2
dd 10 + -+2
dd 10 + --2
"#;
        let out = assemble(src, &AMD64).expect("assemble should succeed");
        let data = out
            .sections
            .iter()
            .find(|s| s.name == ".data")
            .expect("data section");
        assert_eq!(
            data.data,
            vec![
                0x08, 0x00, 0x00, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x0C, 0x00,
                0x00, 0x00,
            ]
        );
        assert!(
            data.relocs.is_empty(),
            "dd constants must not produce relocations"
        );
    }

    #[test]
    fn equ_unary_sign_composition() {
        let src = r#"
section .text
global _start
VAL_SUB_PLUS  equ 10 - +2
VAL_SUB_MINUS equ 10 - -2
VAL_ADD_CHAIN equ 10 + -+2
_start:
    mov eax, VAL_SUB_PLUS
    mov ebx, VAL_SUB_MINUS
    mov ecx, VAL_ADD_CHAIN
    ret
"#;
        let out = assemble(src, &AMD64).expect("assemble should succeed");
        let text = out
            .sections
            .iter()
            .find(|s| s.name == ".text")
            .expect("text section");
        assert_eq!(
            text.data,
            vec![
                0xB8, 0x08, 0x00, 0x00, 0x00, 0xBB, 0x0C, 0x00, 0x00, 0x00, 0xB9, 0x08, 0x00, 0x00,
                0x00, 0xC3,
            ]
        );
        assert!(
            text.relocs.is_empty(),
            "equ immediate must not create relocations"
        );
    }

    #[test]
    fn trailing_operator_and_missing_term_errors() {
        let res1 = assemble("section .text\n_start:\n    mov eax, 10 +\n", &AMD64);
        match res1 {
            Err(e) => assert!(e
                .to_string()
                .contains("Expression cannot end with operator")),
            Ok(_) => panic!("expected error on trailing '+'"),
        }

        let res2 = assemble("section .text\n_start:\n    mov eax, 10 -\n", &AMD64);
        match res2 {
            Err(e) => assert!(e
                .to_string()
                .contains("Expression cannot end with operator")),
            Ok(_) => panic!("expected error on trailing '-'"),
        }

        let res3 = assemble("section .text\n_start:\n    mov eax, 10 - +\n", &AMD64);
        match res3 {
            Err(e) => assert!(e
                .to_string()
                .contains("Expression cannot end with operator")),
            Ok(_) => panic!("expected error on trailing '- +'"),
        }

        let res4 = assemble("section .data\n    dd 10 +\n", &AMD64);
        match res4 {
            Err(e) => assert!(e
                .to_string()
                .contains("Expression cannot end with operator")),
            Ok(_) => panic!("expected error on trailing '+' in data directive"),
        }
    }
}
