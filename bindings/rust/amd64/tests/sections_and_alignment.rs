use amd64::AMD64;

use assembler::{assemble, assemble_with_limits, AsmSectionKind, AssemblyLimits};

fn error(src: &str) -> String {
    assemble(src, &AMD64).err().unwrap_or_else(|| panic!("accepted {src}")).to_string()
}

#[test]
fn explicit_kinds_survive_reentry_and_custom_bss_never_allocates_payloads() {
    let out=assemble("section .code,text\nnop\nalign 32\nend:ret\nsection .readonly,rodata\ndb 7\nalign 16\nsection .mutable,data\ndb 8\nalign 8\nsection .sparse,bss\nresb 1099511627777\nalign 64\nbss_end:\nsection .code\nret",&AMD64).unwrap();
    let sec = |n: &str| out.sections.iter().find(|s| s.name == n).unwrap();
    assert_eq!((sec(".code").kind, sec(".code").align), (AsmSectionKind::Text, 32));
    assert_eq!(sec(".code").data, [vec![0x90; 32], vec![0xc3, 0xc3]].concat());
    assert_eq!(sec(".readonly").kind, AsmSectionKind::ReadOnlyData);
    assert_eq!(sec(".readonly").data, [vec![7], vec![0; 15]].concat());
    assert_eq!(sec(".mutable").data, [8, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        (sec(".sparse").kind, sec(".sparse").align, sec(".sparse").zero_fill),
        (AsmSectionKind::Bss, 64, (1 << 40) + 64)
    );
    assert!(sec(".sparse").data.is_empty());
    assert_eq!(out.symbols.iter().find(|s| s.name == "end").unwrap().offset, 32);
    assert_eq!(out.symbols.iter().find(|s| s.name == "bss_end").unwrap().offset, (1 << 40) + 64);
    for src in [
        "section .x,text\nsection .x,data",
        "section .x,bss\nsection .x,rodata",
        "section .text,bss",
        "section .bss,text",
        "section .x,text,bss",
        "section .x,unknown",
        "section .x,bss\nnop",
        "section .x,bss\ndb 1",
        "section .x,bss\nextern y\ndq y",
    ] {
        error(src);
    }
}

#[test]
fn alignment_validation_and_budgets_precede_materialization() {
    for value in ["0", "3", "-1", "18446744073709551615", "\"8\"", "missing", "8,16"] {
        error(&format!("align {value}"));
    }
    assert!(error("section .sparse,bss\nresb 18446744073709551615\nalign 2").contains("overflow"));
    let limit = AssemblyLimits { max_output_bytes: 8, ..Default::default() };
    assert!(assemble_with_limits("nop\nalign 16", &AMD64, &limit)
        .err()
        .unwrap()
        .to_string()
        .contains("output limit"));
    let out = assemble_with_limits(
        "section .sparse,bss\nresb 1\nalign 9223372036854775808",
        &AMD64,
        &limit,
    )
    .unwrap();
    assert_eq!(out.sections[1].zero_fill, 1 << 63);
    assert!(out.sections[1].data.is_empty());
    assert_eq!(assemble("A equ 16\ndb 1\nalign A\nx:", &AMD64).unwrap().symbols[0].offset, 16);
}

fn target(data: &[u8], at: usize) -> usize {
    match data[at] {
        0xeb | 0x74 => (at as i64 + 2 + data[at + 1] as i8 as i64) as usize,
        0xe9 => {
            (at as i64 + 5 + i32::from_le_bytes(data[at + 1..at + 5].try_into().unwrap()) as i64)
                as usize
        },
        0x0f => {
            (at as i64 + 6 + i32::from_le_bytes(data[at + 2..at + 6].try_into().unwrap()) as i64)
                as usize
        },
        _ => panic!("branch expected"),
    }
}

#[test]
fn aligned_forward_and_backward_branches_converge_to_independently_decoded_targets() {
    for mnemonic in ["jmp", "je"] {
        for align in [2, 4, 8, 16, 32, 64, 128, 256] {
            for padding in 120..132 {
                let src=format!("start:{mnemonic} end\nresb {padding}\nalign {align}\nresb 2\nend:{mnemonic} start");
                let out = assemble(&src, &AMD64).unwrap_or_else(|e| panic!("{src}: {e}"));
                let data = &out.sections[0].data;
                let end = out.symbols.iter().find(|s| s.name == "end").unwrap().offset as usize;
                assert_eq!(target(data, 0), end);
                assert_eq!(target(data, end), 0);
                assert!(out.sections[0].relocs.is_empty());
                assert_eq!(&assemble(&src, &AMD64).unwrap().sections[0].data, data);
            }
        }
    }
    // Shrinking this branch does not move its aligned target. It must settle near,
    // rather than alternating forever between a two-byte and five-byte encoding.
    let out = assemble("jmp end\nresb 123\nalign 128\nresb 2\nend:ret", &AMD64).unwrap();
    assert_eq!(out.sections[0].data[0], 0xe9);
    assert_eq!(target(&out.sections[0].data, 0), 130);
}
