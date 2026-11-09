use amd64::AMD64;

use assembler::{
    assemble,
    tokens::{tokenize, TokenKind},
};

fn bytes(src: &str) -> Vec<u8> {
    assemble(src, &AMD64).unwrap().sections.into_iter().flat_map(|s| s.data).collect()
}

fn error(src: &str) -> String {
    assemble(src, &AMD64).err().expect("must reject").to_string()
}

#[test]
fn instruction_commas_require_an_operand_and_statements_require_a_boundary() {
    for suffix in ["", "\n", " ; comment\n"] {
        for instruction in ["push rax,", "mov rax, rbx,"] {
            let e = error(&format!("{instruction}{suffix}"));
            assert!(e.contains("operand after comma") && e.contains("line 1"), "{e}");
        }
    }
    for src in [
        "mov rax, rbx nop",
        "pop rax nop",
        "section .text nop",
        "global entry nop",
        "extern foo nop",
    ] {
        let e = error(src);
        assert!(e.contains("end of statement") && e.contains("column"), "{e}");
    }
    assert_eq!(
        bytes("entry: push rax\nmov rax, rbx ; comment\n\nret"),
        [0x50, 0x48, 0x89, 0xd8, 0xc3]
    );
}

#[test]
fn data_operands_have_single_separators_and_cannot_be_omitted() {
    for name in ["db", "dw", "dd", "dq"] {
        for operands in [",1", "1,,2", "1,", "\"a\" \"b\""] {
            assert!(error(&format!("{name} {operands}")).contains("Parser error"));
        }
        for suffix in ["", "\n", " ; comment\n"] {
            assert!(error(&format!("{name}{suffix}")).contains("Expected directive value"));
        }
    }
    assert_eq!(bytes("db \"\""), []);
    assert_eq!(bytes("db \"é\", 0, \"x\", 255"), [0xc3, 0xa9, 0, b'x', 255]);
    assert!(error("db \"a\" \"b\"").contains("column 8"));
}

#[test]
fn every_line_ending_preserves_comments_code_and_coordinates() {
    for nl in ["\n", "\r", "\r\n"] {
        assert_eq!(bytes(&format!("; é comment{nl}nop{nl}")), [0x90]);
        assert!(error(&format!("; é{nl}@")).contains("line 2, column 1"));
        assert!(error(&format!("; é{nl}push rax,")).contains("line 2, column 9"));
        assert_eq!(bytes(&format!("db \"a{nl}b\"")), format!("a{nl}b").as_bytes());
        let tokens = tokenize(&format!("db \"a{nl}b\"{nl}nop")).unwrap();
        let nop = tokens.last().unwrap();
        assert_eq!((nop.line, nop.column), (3, 1));
    }
}

#[test]
fn utf8_positions_are_byte_offsets_while_columns_count_scalars() {
    for source in ["db \"é\"\nnop", "; 한글\nnop", "étiquette: nop", "db \"é\" ; 한\r\nnop"] {
        let tokens = tokenize(source).unwrap();
        for token in &tokens {
            assert!(source.is_char_boundary(token.position));
        }
        let nop = tokens
            .iter()
            .find(|t| matches!(&t.kind, TokenKind::Identifier(s) if s == "nop"))
            .unwrap();
        assert_eq!(nop.position, source.find("nop").unwrap());
    }
    let tokens = tokenize("nop\nret").unwrap();
    assert_eq!(tokens.last().unwrap().position, 4);
    assert!(error("db \"é\"\n  @").contains("line 2, column 3"));
    assert!(error("db \"é\" @").contains("line 1, column 8"));
}
