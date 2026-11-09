use assembler::{extension::Assembler, AsmSectionKind, AssemblyLimits};

use riscv64::RiscV64;

#[test]
fn named_standard_sections_reuse_canonical_sections_and_keep_their_kinds() {
    let source = ".text\nfirst: nop\n.section .text\nsecond: nop\n.data\n.word 1\n.section .data\n.word 2\n.rodata\n.byte 3\n.section .rodata\n.byte 4\n.bss\n.zero 5\n.section .bss\n.zero 7\n";
    let output = RiscV64.assemble("sections.s", source, &AssemblyLimits::default()).unwrap();
    assert_eq!(output.sections.len(), 4);
    for (section, name, kind, bytes, zero) in [
        (&output.sections[0], ".text", AsmSectionKind::Text, 8, 0),
        (&output.sections[1], ".data", AsmSectionKind::Data, 8, 0),
        (&output.sections[2], ".rodata", AsmSectionKind::ReadOnlyData, 2, 0),
        (&output.sections[3], ".bss", AsmSectionKind::Bss, 0, 12),
    ] {
        assert_eq!(section.name, name);
        assert_eq!(section.kind, kind);
        assert_eq!(section.data.len(), bytes);
        assert_eq!(section.zero_fill, zero);
    }
    assert_eq!(output.sections[0].data, [0x13, 0, 0, 0, 0x13, 0, 0, 0]);
    assert_eq!(output.sections[1].data, [1, 0, 0, 0, 2, 0, 0, 0]);
}
