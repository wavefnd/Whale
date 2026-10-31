// SPDX-License-Identifier: MPL-2.0
//! LoongArch64 assembler entry point; instruction handling lives in the ISA modules.
use assembler::{
    fixed::{Context, FixedIsa, Instruction},
    AsmError, AssemblerOutput, AssemblyLimits,
};

#[cfg(not(whale_wave_loongarch64))]
mod encoder;

#[cfg(all(test, whale_wave_loongarch64))]
#[path = "encoder.rs"]
mod reference_encoder;

#[cfg(whale_wave_loongarch64)]
mod wave;

#[cfg(not(whale_wave_loongarch64))]
use assembler::fixed;

#[cfg(any(test, not(whale_wave_loongarch64)))]
mod encoding;

#[cfg(any(test, not(whale_wave_loongarch64)))]
mod parser;

#[cfg(any(test, not(whale_wave_loongarch64)))]
mod tables;

pub struct LoongArch64;

impl assembler::extension::Assembler for LoongArch64 {
    fn id(&self) -> &str {
        "loongarch64"
    }

    fn object_target(&self) -> Option<target_spec::ObjectTarget> {
        Some(target_spec::ObjectTarget::elf64(target_spec::Machine::LoongArch64))
    }

    fn assemble(
        &self,
        name: &str,
        source: &str,
        limits: &AssemblyLimits,
    ) -> Result<AssemblerOutput, AsmError> {
        #[cfg(whale_wave_loongarch64)]
        {
            wave::assemble(name, source, limits)
        }
        #[cfg(not(whale_wave_loongarch64))]
        {
            fixed::assemble(self, name, source, limits)
        }
    }
}

impl FixedIsa for LoongArch64 {
    fn id(&self) -> &'static str {
        "loongarch64"
    }

    #[cfg(whale_wave_loongarch64)]
    fn native_assemble(
        &self,
        name: &str,
        source: &str,
        limits: &AssemblyLimits,
    ) -> Option<Result<AssemblerOutput, AsmError>> {
        Some(wave::assemble(name, source, limits))
    }

    fn encode(
        &self,
        instruction: &Instruction,
        context: &mut Context<'_>,
    ) -> Result<u32, AsmError> {
        #[cfg(whale_wave_loongarch64)]
        {
            wave::encode(instruction, context)
        }
        #[cfg(not(whale_wave_loongarch64))]
        {
            encoder::encode(instruction, context)
        }
    }
}

#[cfg(all(test, whale_wave_loongarch64))]
mod wave_tests {
    use super::*;

    struct Reference;

    impl FixedIsa for Reference {
        fn id(&self) -> &'static str {
            "loongarch64"
        }

        fn encode(
            &self,
            instruction: &Instruction,
            context: &mut Context<'_>,
        ) -> Result<u32, AsmError> {
            reference_encoder::encode(instruction, context)
        }
    }

    fn compare(source: &str, limits: AssemblyLimits) {
        let expected =
            assembler::fixed::reference::assemble(&Reference, "한/source.s", source, &limits);
        let actual = wave::assemble("한/source.s", source, &limits);
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => {
                assert_eq!(actual.sections.len(), expected.sections.len(), "{source}");
                for (actual, expected) in actual.sections.iter().zip(&expected.sections) {
                    assert_eq!(actual.name, expected.name, "{source}");
                    assert_eq!(actual.kind, expected.kind, "{source}");
                    assert_eq!(actual.align, expected.align, "{source}");
                    assert_eq!(actual.data, expected.data, "{source}");
                    assert_eq!(actual.zero_fill, expected.zero_fill, "{source}");
                    assert_eq!(
                        format!("{:?}", actual.relocs),
                        format!("{:?}", expected.relocs),
                        "{source}"
                    );
                }
                assert_eq!(actual.symbols.len(), expected.symbols.len(), "{source}");
                for (actual, expected) in actual.symbols.iter().zip(&expected.symbols) {
                    assert_eq!(actual.name, expected.name, "{source}");
                    assert_eq!(actual.section_index, expected.section_index, "{source}");
                    assert_eq!(actual.offset, expected.offset, "{source}");
                    assert_eq!(actual.is_global, expected.is_global, "{source}");
                }
            },
            (Err(actual), Err(expected)) => {
                assert_eq!(actual.to_string(), expected.to_string(), "{source}");
            },
            (actual, expected) => panic!(
                "{source}: result classes differ: actual={}, expected={}",
                actual.is_ok(),
                expected.is_ok()
            ),
        }
    }

    #[test]
    fn native_complete_inventory_and_validation_precedence_match_reference() {
        for line in include_str!("../tests/fixtures/encodings.tsv").lines() {
            if !line.is_empty() && !line.starts_with('#') {
                compare(line.split_once('\t').unwrap().1, AssemblyLimits::default());
            }
        }
        for source in [
            "", "ADD.D $R0,$R1,$R2", "move $FP, $S0", "move r01,r0",
            "ld.d BAD,$r0,99999", "addi.d BAD,$r0,99999", "slli.d BAD,$r0,99999",
            "lu12i.w BAD,524288", "jirl BAD,BAD,18446744073709551616", "li.d BAD,99999",
            "add.d $r0,$r0", "nop $r0", "beqz $r1,2", "b 134217728",
            "move $$sp,r0", "move r１,r0", ".global missing\nBAD\n",
            "here: here: nop", ".extern foo\nfoo: nop", "foo: nop\n.extern foo",
            ".text\n.byte 0\nnop", ".data\nnop", ".bss\n.word 0",
            ".balign 0", ".align 64", ".zero -1", ".quad missing", ".byte here\nhere: nop",
            ".section .TEXT\n.byte 1", ".data\n.byte 1,2,255,-128\n.hword 65535,-32768\n.long 4294967295\n.quad -1",
            ".global start\nstart: beq $r1,$r2,done\nnop\ndone: bl external+8\n.extern external\n.data\n.quad start+4",
            ".bss\n.zero 1099511627776\n.balign 4096\nstorage:\n.text\nret\n.global storage",
            "; 한글\r\n  add.d $bad,$r0,$r0 ; ignored\r\n", "nop\rret",
        ] {
            compare(source, AssemblyLimits::default());
        }
        for limits in [
            AssemblyLimits { max_output_bytes: 0, ..Default::default() },
            AssemblyLimits { max_output_bytes: 3, ..Default::default() },
            AssemblyLimits { max_relax_iterations: 0, ..Default::default() },
        ] {
            compare("nop", limits);
        }
    }

    #[test]
    fn native_generated_integer_and_branch_sources_match_explicit_reference() {
        let mut state = 0x0057_4841_4c45_4c41_u64;
        for _ in 0..2000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let rd = state % 32;
            let rj = (state >> 5) % 32;
            let rk = (state >> 10) % 32;
            let immediate = ((state >> 16) % 4096) as i64 - 2048;
            let offset = (((state >> 28) % 65536) as i64 - 32768) * 4;
            compare(&format!("add.d $r{rd},$r{rj},$r{rk}\naddi.d $r{rd},$r{rj},{immediate}\nbeq $r{rj},$r{rd},{offset}"), AssemblyLimits::default());
        }
    }
}
