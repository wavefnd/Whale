use amd64::AMD64;

use assembler::{assemble, assemble_with_limits, AssemblyLimits, RelocKind};

fn limited(
    src: &str,
    bytes: usize,
    passes: usize,
) -> Result<assembler::AssemblerOutput, assembler::error::AsmError> {
    assemble_with_limits(
        src,
        &AMD64,
        &AssemblyLimits { max_output_bytes: bytes, max_relax_iterations: passes },
    )
}

#[test]
fn output_budget_is_checked_before_reservation_and_is_aggregate() {
    assert!(assemble("resb 9223372036854775807", &AMD64)
        .err()
        .unwrap()
        .to_string()
        .contains("output limit"));
    for src in [
        "resb 9",
        "resw 5",
        "resd 3",
        "resq 2",
        "db \"123456789\"",
        "section .text\nresb 5\nsection .data\nresb 4",
        "resb 8\nnop",
    ] {
        assert!(limited(src, 8, 64).err().unwrap().to_string().contains("output limit"), "{src}");
    }
    assert_eq!(
        limited("resb 7\nnop", 8, 64).unwrap().sections[0].data,
        [0, 0, 0, 0, 0, 0, 0, 0x90]
    );
    let out = limited("section .bss\nresb 1099511627776\nsection .text\nret", 1, 64).unwrap();
    assert_eq!(out.sections[1].zero_fill, 1 << 40);
    assert!(out.sections[1].data.is_empty());
    assert_eq!(limited("resb 0", 0, 64).unwrap().sections[0].data, []);
}

#[test]
fn global_declarations_are_defined_or_explicitly_external() {
    assert!(assemble("global missing\nnop", &AMD64)
        .err()
        .unwrap()
        .to_string()
        .contains("Global symbol 'missing'"));
    for src in ["global entry\nentry: ret", "entry: ret\nglobal entry"] {
        let out = assemble(src, &AMD64).unwrap();
        let symbol = out.symbols.iter().find(|s| s.name == "entry").unwrap();
        assert!(symbol.is_global);
        assert_eq!(symbol.section_index, Some(0));
        assert_eq!(symbol.offset, 0);
    }
    for src in ["global ext\nextern ext\ncall ext", "extern ext\nglobal ext\ncall ext"] {
        let out = assemble(src, &AMD64).unwrap();
        assert_eq!(out.symbols.len(), 1);
        assert!(out.symbols[0].is_global);
        assert_eq!(out.symbols[0].section_index, None);
        assert!(matches!(out.sections[0].relocs[0].kind, RelocKind::Branch32));
    }
}

fn decoded_target(bytes: &[u8], at: usize) -> (usize, usize) {
    match bytes[at] {
        0xeb | 0x74 | 0xe2 => (2, (at as i64 + 2 + bytes[at + 1] as i8 as i64) as usize),
        0xe9 => (
            5,
            (at as i64 + 5 + i32::from_le_bytes(bytes[at + 1..at + 5].try_into().unwrap()) as i64)
                as usize,
        ),
        0x0f => (
            6,
            (at as i64 + 6 + i32::from_le_bytes(bytes[at + 2..at + 6].try_into().unwrap()) as i64)
                as usize,
        ),
        op => panic!("unexpected branch {op:x}"),
    }
}

#[test]
fn relaxation_never_returns_a_changed_layout_at_the_pass_limit() {
    assert!(limited("start: jmp end\nnop\nend: ret", 100, 1)
        .err()
        .unwrap()
        .to_string()
        .contains("did not converge"));
    assert!(limited("nop", 100, 0).is_err());
    // Thirty forward branches shrink in cascading steps, well beyond the old
    // fixed eight-pass horizon. Every final displacement is independently read.
    let mut source = String::new();
    for n in 0..30 {
        source.push_str(&format!("from{n}: jmp target{n}\n"));
        if n > 0 {
            source.push_str(&format!("target{}:\n", n - 1));
        }
        source.push_str(if n == 29 { "resb 119\n" } else { "resb 125\n" });
    }
    source.push_str("target29: ret");
    assert!(limited(&source, 10000, 8).err().unwrap().to_string().contains("did not converge"));
    let out = assemble(&source, &AMD64).unwrap();
    let bytes = &out.sections[0].data;
    for n in 0..30 {
        let at = out.symbols.iter().find(|s| s.name == format!("from{n}")).unwrap().offset as usize;
        let target =
            out.symbols.iter().find(|s| s.name == format!("target{n}")).unwrap().offset as usize;
        assert_eq!(decoded_target(bytes, at).1, target);
    }
    assert!(out.sections[0].relocs.is_empty());
}

#[test]
fn forward_backward_displacement_boundaries_decode_to_the_final_labels() {
    for mnemonic in ["jmp", "je"] {
        for padding in [123, 124, 125, 126, 127, 128, 129] {
            let source = format!("start: {mnemonic} end\nresb {padding}\nend: {mnemonic} start");
            let out = assemble(&source, &AMD64).unwrap();
            let bytes = &out.sections[0].data;
            let end = out.symbols.iter().find(|s| s.name == "end").unwrap().offset as usize;
            assert_eq!(decoded_target(bytes, 0).1, end);
            assert_eq!(decoded_target(bytes, end).1, 0);
            assert!(out.sections[0].relocs.is_empty());
        }
    }
}

#[test]
fn loop_range_checks_use_the_converged_forward_and_backward_layout() {
    for (mnemonic, opcode, padding) in
        [("jmp", 0xeb, 124), ("jmp", 0xeb, 125), ("je", 0x74, 124), ("je", 0x74, 125)]
    {
        let source = format!("entry: loop .done\n{mnemonic} .done\nresb {padding}\n.done: ret");
        let out = assemble(&source, &AMD64).unwrap();
        let bytes = &out.sections[0].data;
        let done = out.symbols.iter().find(|s| s.name == "entry.done").unwrap().offset as usize;
        assert_eq!(&bytes[..4], &[0xe2, (padding + 2) as u8, opcode, padding as u8]);
        assert_eq!(decoded_target(bytes, 0).1, done);
        assert_eq!(decoded_target(bytes, 2).1, done);
        assert!(out.sections[0].relocs.is_empty());
        assert!(limited(&source, 1024, 2).err().unwrap().to_string().contains("did not converge"));
        assert_eq!(limited(&source, 1024, 3).unwrap().sections[0].data, *bytes);
    }
    for padding in [121, 122, 123] {
        let source = format!("start: nop\njmp done\nresb {padding}\nback: loop start\ndone: ret");
        let out = assemble(&source, &AMD64).unwrap();
        let bytes = &out.sections[0].data;
        let back = out.symbols.iter().find(|s| s.name == "back").unwrap().offset as usize;
        let done = out.symbols.iter().find(|s| s.name == "done").unwrap().offset as usize;
        assert_eq!(back, padding + 3);
        assert_eq!(decoded_target(bytes, back).1, 0);
        assert_eq!(decoded_target(bytes, 1).1, done);
        assert!(out.sections[0].relocs.is_empty());
    }
}

#[test]
fn loop_limits_and_alignment_keep_precise_diagnostics_and_final_targets() {
    for (source, line) in [
        ("loop done\njmp done\nresb 126\ndone: ret", "line 1,"),
        ("start: nop\njmp done\nresb 124\nloop start\ndone: ret", "line 4,"),
        ("extern ext\nlocal: loop local\ncall ext\nresb 128\nloop local", "line 5,"),
    ] {
        let error = assembler::assemble_named("loop.asm", source, &AMD64).err().unwrap();
        let text = error.to_string();
        assert!(text.contains("out of 8-bit range"), "{text}");
        assert!(text.contains("loop.asm"), "{text}");
        assert!(text.contains(line), "{text}");
    }
    for source in [
        "loop done\njmp done\nresb 120\nalign 16\ndone: ret",
        "jmp done\nagain: loop again\nresb 121\nalign 128\nresb 2\ndone: ret",
    ] {
        let out = assemble(source, &AMD64).unwrap();
        let bytes = &out.sections[0].data;
        let done = out.symbols.iter().find(|s| s.name == "done").unwrap().offset as usize;
        let loop_at = if source.starts_with("loop") { 0 } else { 5 };
        let loop_target = if loop_at == 0 { done } else { loop_at };
        assert_eq!(decoded_target(bytes, loop_at).1, loop_target);
        assert_eq!(decoded_target(bytes, if loop_at == 0 { 2 } else { 0 }).1, done);
        assert!(out.sections[0].relocs.is_empty());
        assert_eq!(assemble(source, &AMD64).unwrap().sections[0].data, *bytes);
    }
}

#[test]
fn loop_resolution_preserves_other_relocations_and_cross_section_pc8() {
    let source = "extern ext\nloop local\ncall ext\ndq ext\nlocal: loop ext\nloop other\nsection .other,text\nother: ret";
    let out = assemble(source, &AMD64).unwrap();
    let text = &out.sections[0];
    assert_eq!(decoded_target(&text.data, 0).1, 15);
    assert_eq!(text.relocs.len(), 4);
    for (r, offset, symbol) in [
        (&text.relocs[0], 3, "ext"),
        (&text.relocs[1], 7, "ext"),
        (&text.relocs[2], 16, "ext"),
        (&text.relocs[3], 18, "other"),
    ] {
        assert_eq!(r.offset, offset);
        assert_eq!(r.symbol, symbol);
    }
    assert!(matches!(text.relocs[0].kind, RelocKind::Branch32));
    assert!(matches!(text.relocs[1].kind, RelocKind::Absolute64));
    for r in &text.relocs[2..] {
        assert!(matches!(r.kind, RelocKind::Relative8));
        assert_eq!(r.addend, -1);
        assert_eq!(text.data[r.offset], 0);
    }
}
