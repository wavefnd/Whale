// SPDX-License-Identifier: MPL-2.0
//! Independent LLVM encodings retained for ordinary, tool-free Cargo tests.
use assembler::{extension::Assembler, AssemblyLimits};

#[test]
fn matches_retained_independent_encodings() {
    assert_encodings(include_str!("fixtures/encodings.tsv"));
}

#[test]
fn mov_immediates_match_retained_independent_wide_move_encodings() {
    assert_encodings(include_str!("fixtures/mov_wide.tsv"));
}

fn assert_encodings(fixtures: &str) {
    for line in fixtures.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (hex, source) = line.split_once('\t').unwrap();
        let expected: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let output = aarch64::AArch64
            .assemble("fixture.s", source, &AssemblyLimits::default())
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        assert_eq!(output.sections[0].data, expected, "{source}");
    }
}

#[test]
fn mov_immediates_reject_multimove_values_and_width_overflow() {
    for source in [
        "mov w0,#0x12345678",
        "mov x0,#0x12345678",
        "mov x0,#0x0001000100010001",
        "mov x0,#0xffffffff00000000",
        "mov x0,#0x00ff00ff00ff00ff",
    ] {
        let error = aarch64::AArch64
            .assemble("mov.s", source, &AssemblyLimits::default())
            .err()
            .expect("multi-instruction MOV must be rejected");
        assert!(error.to_string().contains("single 16-bit MOVZ/MOVN"), "{source}: {error}");
    }
    for source in [
        "mov w0,#4294967296",
        "mov w0,#-2147483649",
        "mov w0,#0x100000000",
        "mov w0,#-0x100000000",
        "mov x0,#18446744073709551616",
        "mov x0,#-9223372036854775809",
    ] {
        let error = aarch64::AArch64
            .assemble("mov.s", source, &AssemblyLimits::default())
            .err()
            .expect("out-of-width MOV must be rejected");
        assert!(error.to_string().contains("destination register width"), "{source}: {error}");
    }
}

#[test]
fn mov_immediates_preserve_sp_and_zero_register_validation() {
    for source in [
        "mov sp,#65536",
        "mov wsp,#65536",
        "mov sp,xzr",
        "mov xzr,sp",
        "mov wsp,wzr",
        "mov wzr,wsp",
    ] {
        assert!(
            aarch64::AArch64.assemble("mov.s", source, &AssemblyLimits::default()).is_err(),
            "{source}"
        );
    }
}
