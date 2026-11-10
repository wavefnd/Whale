use amd64::AMD64;

use diagnostic::{Context, ToDiagnostic};

#[test]
fn stable_assembler_codes_preserve_primary_and_related_utf8_ranges() {
    let input = "section .text\nname:\n ret\nname:\n ret\n";
    let error = assembler::assemble_named("경로/input.asm", input, &AMD64).err().unwrap();
    let diagnostic = error.diagnostic();
    assert_eq!(
        (diagnostic.component, diagnostic.phase, diagnostic.code),
        ("assembler", "symbols", "assembler.symbol")
    );
    let locations = diagnostic
        .contexts
        .iter()
        .filter_map(|c| if let Context::Source(s) = c { Some(s) } else { None })
        .collect::<Vec<_>>();
    assert_eq!(locations.len(), 2);
    assert_eq!(locations[0].source, "경로/input.asm");
    assert_eq!((locations[0].line, locations[1].line), (4, 2));
    for s in locations {
        assert_eq!(s.column, 1);
        assert!(s.start_byte.unwrap() < s.end_byte.unwrap());
    }
    assert!(error.to_string().contains("previous declaration"));
    let syntax =
        assembler::assemble_named("bad.asm", "mov rax, ]", &AMD64).err().unwrap().diagnostic();
    assert_eq!(syntax.phase, "parse");
    assert!(syntax.contexts.iter().any(|c| matches!(c,Context::Source(s)if s.source=="bad.asm")));
}
