use amd64::AMD64;

use assembler::{
    assemble_named,
    tokens::{tokenize, TokenKind},
    traits::ISA,
    AsmError, SourceSpan,
};

fn diagnostic(src: &str) -> (String, SourceSpan, Option<SourceSpan>, String) {
    match assemble_named("source/한글.asm", src, &AMD64).err().expect("invalid source") {
        AsmError::Diagnostic { source_name, span, related, error } => {
            (source_name, span, related, error.to_string())
        },
        other => panic!("missing source diagnostic: {other}"),
    }
}

#[test]
fn encoder_errors_retain_file_unicode_byte_range_and_multiline_coordinates() {
    let src = "; 한글\r\n\r\nsection .text\r\nentry:\r\n  mov rax, eax ; mismatch\r\n";
    let (file, span, related, message) = diagnostic(src);
    let start = src.find("mov rax").unwrap();
    assert_eq!(file, "source/한글.asm");
    assert_eq!(span, SourceSpan { start, end: start + 12, line: 5, column: 3 });
    assert_eq!(&src[span.start..span.end], "mov rax, eax");
    assert!(related.is_none());
    assert!(message.contains("width mismatch"));
    let text = assemble_named("source/한글.asm", src, &AMD64).err().unwrap().to_string();
    assert!(text.contains("source/한글.asm: line 5, column 3"));
    assert!(text.contains(&format!("bytes {start}..{}", start + 12)));
    // Direct parse/encode users also retain spans, with the documented input name.
    let ast = AMD64.parse(&tokenize(src).unwrap()).unwrap();
    let error = AMD64.encode(&ast).err().unwrap().to_string();
    assert!(error.contains("<input>: line 5, column 3"));
}

#[test]
fn duplicate_and_conflicting_definitions_identify_both_exact_declaration_sites() {
    for src in [
        "; first\nfoo: nop\n\n; comment\nfoo: ret\n",
        "foo equ 1\n\nfoo equ 2\n",
        "foo equ 1\n\nextern foo\n",
        "extern foo\n\nfoo equ 1\n",
        "parent:\n.C equ 1\n\n.C equ 2\n",
        "foo: nop\nextern foo",
        "extern foo\nfoo: nop",
    ] {
        let (_, primary, previous, _) = diagnostic(src);
        let previous = previous.expect("both declaration sites");
        assert!(previous.start < primary.start, "{src}");
        assert!(previous.line < primary.line, "{src}");
        assert!(src.is_char_boundary(primary.start) && src.is_char_boundary(primary.end));
        assert!(src.is_char_boundary(previous.start) && src.is_char_boundary(previous.end));
        let display = assemble_named("source/한글.asm", src, &AMD64).err().unwrap().to_string();
        assert!(display.contains("previous declaration at source/한글.asm"), "{display}");
    }
    let src = "; first\nfoo: nop\n\n; comment\nfoo: ret\n";
    let (_, span, previous, _) = diagnostic(src);
    assert_eq!(span.line, 5);
    assert_eq!(previous.unwrap().line, 2);
    assert_eq!(&src[span.start..span.end], "foo:");
}

#[test]
fn deferred_symbol_constant_and_global_errors_point_to_their_source_statement() {
    for (src, fragment, line) in [
        ("; comment\n\ncall missing\n", "call missing", 3),
        ("section .data\n\ndq missing+4\n", "dq missing+4", 3),
        ("N equ M\n", "N equ M", 1),
        ("global missing\nnop\n", "global missing", 1),
        ("N equ M+1\nM equ 18446744073709551615\ndq N", "N equ M+1", 1),
    ] {
        let (_, span, _, _) = diagnostic(src);
        assert_eq!(&src[span.start..span.end], fragment);
        assert_eq!(span.line, line);
    }
}

#[test]
fn lexer_and_parser_failures_have_named_exact_ranges_and_tokens_keep_end_coordinates() {
    for (src, fragment, line, column) in [
        ("; é\n  @", "@", 2, 3),
        ("; é\nmov rax, ]", "]", 2, 10),
        ("; é\ndb \"é\\q\"", "\\q", 2, 6),
        ("push rax,", ",", 1, 9),
    ] {
        let (_, span, _, _) = diagnostic(src);
        assert_eq!(&src[span.start..span.end], fragment);
        assert_eq!((span.line, span.column), (line, column));
    }
    let src = "db \"é\\xFF\"\r\nnop";
    let tokens = tokenize(src).unwrap();
    for t in &tokens {
        assert!(src.is_char_boundary(t.position) && src.is_char_boundary(t.end_position));
    }
    let string = &tokens[1];
    assert_eq!(&src[string.position..string.end_position], "\"é\\xFF\"");
    assert!(matches!(string.kind, TokenKind::StringLiteral(_)));
    let nop = tokens.last().unwrap();
    assert_eq!((nop.line, nop.column, nop.end_line, nop.end_column), (2, 1, 2, 4));
    assert_eq!(nop.end_position, src.len());
}
