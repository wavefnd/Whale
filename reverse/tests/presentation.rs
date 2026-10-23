// SPDX-License-Identifier: MPL-2.0
//! Rendering caller-supplied typed facts does not require a decoder backend.
use reverse::*;

fn call() -> Instruction {
    let mut bytes = [0; 15];
    bytes[..5].copy_from_slice(&[0xe8, 0, 0, 0, 0]);
    Instruction {
        offset: 0,
        length: 5,
        bytes,
        opcode: Opcode::Call,
        condition: None,
        operands: vec![Operand::Relative(Relative { displacement: 0, encoded_width: 32 })],
        encoding: Encoding {
            prefixes: vec![],
            rex: None,
            opcode_offset: 0,
            opcode_len: 1,
            modrm: None,
            sib: None,
            displacement: None,
            immediate: Some(EncodingField { offset: 1, width: 4, bits: 0 }),
        },
        flow: Flow::Call { relative: Some(0) },
    }
}

#[test]
fn symbolic_relocations_suppress_placeholder_call_targets() {
    let instruction = call();
    let annotation = image::Annotation {
        region: 0,
        offset: 1,
        width: 4,
        symbol: "external".into(),
        addend: -4,
        kind: object::RelocKind::PLT32,
    };
    let annotations = [&annotation];
    let report = format::instruction_json(&instruction, 0x400000, &annotations);
    assert!(report["target"].is_null());
    assert_eq!(report["relocations"][0]["symbol"], "external");
    assert_eq!(report["encoding"]["immediate"]["width_bytes"], 4);
    let text = format::instruction_text(&instruction, 0x400000, &annotations);
    assert!(text.contains("call reloc(external-4)"), "{text}");
    assert!(!text.contains("call 0x400005"));
}

#[test]
fn checked_target_and_address_rendering_preserves_overflow() {
    let mut instruction = call();
    assert_eq!(instruction.relative_target(0x1000), Some(0x1005));
    assert_eq!(instruction.relative_target(u64::MAX), None);
    instruction.offset = 1;
    let report = format::instruction_json(&instruction, u64::MAX, &[]);
    assert!(report["address"].is_null());
    assert!(report["target"].is_null());
    assert!(format::instruction_text(&instruction, u64::MAX, &[]).starts_with("overflow"));
    assert_eq!(instruction.original_bytes(), [0xe8, 0, 0, 0, 0]);
}

#[test]
fn register_and_immediate_widths_are_presentation_facts() {
    for (index, width, high8, name) in [
        (0, 8, true, "ah"),
        (4, 8, false, "spl"),
        (9, 16, false, "r9w"),
        (15, 32, false, "r15d"),
        (8, 64, false, "r8"),
    ] {
        let register = Register { class: RegisterClass::General, index, width, high8 };
        assert_eq!(register.name(), name);
    }
    let immediate = Immediate { value: 0xff, width: 64, encoded_width: 8, sign_extended: true };
    assert_eq!(immediate.signed_value(), -1);
    assert_eq!(format::operand_text(&Operand::Immediate(immediate), &call(), 0), "-1");
}
