use amd64::AMD64;

use assembler::assembler::assemble;

#[test]
fn large_bss_and_section_reentry_keep_logical_label_offsets_without_payload() {
    let out = assemble(
        "section .bss\nstart:\nresb 1099511627776\nsection .text\nret\nsection .bss\nmiddle:\nresq 2\ndb 0,0\nend:\n",
        &AMD64,
    )
    .unwrap();
    let bss = out.sections.iter().find(|s| s.name == ".bss").unwrap();
    assert!(bss.data.is_empty());
    assert!(bss.relocs.is_empty());
    assert_eq!(bss.zero_fill, (1 << 40) + 18);
    for (name, offset) in [("start", 0), ("middle", 1 << 40), ("end", (1 << 40) + 18)] {
        assert_eq!(out.symbols.iter().find(|s| s.name == name).unwrap().offset, offset);
    }
    let text = out.sections.iter().find(|s| s.name == ".text").unwrap();
    assert_eq!(text.data, [0xc3]);
    assert_eq!(text.zero_fill, 0);
}

#[test]
fn bss_rejects_payloads_relocations_instructions_and_reservation_overflow() {
    for body in [
        "db 1",
        "dw -1",
        "extern x\ndq x",
        "ret",
        "resb -1",
        "resq 9223372036854775807",
        "resb 9223372036854775807\nresb 9223372036854775807\nresb 2",
    ] {
        assert!(assemble(&format!("section .bss\n{body}\n"), &AMD64).is_err(), "{body}");
    }
    let out = assemble("section .data\nresb 3\ndb 1\n", &AMD64).unwrap();
    assert_eq!(out.sections[1].data, [0, 0, 0, 1]);
    assert_eq!(out.sections[1].zero_fill, 0);
}
