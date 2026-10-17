// SPDX-License-Identifier: MPL-2.0
//! Independent LLVM encodings retained for ordinary, tool-free Cargo tests.
use assembler::{extension::Assembler, AssemblyLimits};

#[test]
fn matches_retained_independent_encodings() {
    for line in include_str!("fixtures/encodings.tsv")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (hex, source) = line.split_once('\t').unwrap();
        let expected: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let output = loongarch64::LoongArch64
            .assemble("fixture.s", source, &AssemblyLimits::default())
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        assert_eq!(output.sections[0].data, expected, "{source}");
    }
}
