// SPDX-License-Identifier: MPL-2.0
//! RiscV64 assembler entry point; instruction handling lives in the ISA modules.
use assembler::{
    fixed::{Context, FixedIsa, Instruction},
    AsmError, AssemblerOutput, AssemblyLimits,
};

mod encoder;

#[cfg(any(test, not(whale_wave_riscv64)))]
mod encoding;

#[cfg(any(test, not(whale_wave_riscv64)))]
mod parser;

#[cfg(any(test, not(whale_wave_riscv64)))]
mod tables;

#[cfg(whale_wave_riscv64)]
mod wave;

pub struct RiscV64;

impl assembler::extension::Assembler for RiscV64 {
    fn id(&self) -> &str {
        "riscv64"
    }

    fn object_target(&self) -> Option<target_spec::ObjectTarget> {
        Some(target_spec::ObjectTarget::elf64(target_spec::Machine::RiscV64))
    }

    fn assemble(
        &self,
        name: &str,
        source: &str,
        limits: &AssemblyLimits,
    ) -> Result<AssemblerOutput, AsmError> {
        #[cfg(whale_wave_riscv64)]
        return wave::assemble(name, source, limits);
        #[cfg(not(whale_wave_riscv64))]
        assembler::fixed::assemble(self, name, source, limits)
    }
}

impl FixedIsa for RiscV64 {
    fn id(&self) -> &'static str {
        "riscv64"
    }

    #[cfg(whale_wave_riscv64)]
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
        encoder::encode(instruction, context)
    }
}

#[cfg(all(test, whale_wave_riscv64))]
mod wave_tests {
    use super::*;

    struct Reference;

    impl FixedIsa for Reference {
        fn id(&self) -> &'static str {
            "riscv64"
        }

        fn encode(
            &self,
            instruction: &Instruction,
            context: &mut Context<'_>,
        ) -> Result<u32, AsmError> {
            encoder::reference::encode(instruction, context)
        }
    }

    fn snapshot(output: AssemblerOutput) -> String {
        let mut value = String::new();
        for section in output.sections {
            value.push_str(&format!(
                "{:?}|{}|{}|{}|{:?}|",
                section.kind, section.name, section.align, section.zero_fill, section.data
            ));
            for relocation in section.relocs {
                value.push_str(&format!(
                    "{}|{}|{:?}|{}|",
                    relocation.offset, relocation.symbol, relocation.kind, relocation.addend
                ));
            }
        }
        for symbol in output.symbols {
            value.push_str(&format!(
                "{}|{:?}|{}|{}|{:?}|{}|{:?}|",
                symbol.name,
                symbol.section_index,
                symbol.offset,
                symbol.is_global,
                symbol.kind,
                symbol.size,
                symbol.visibility
            ));
        }
        value
    }

    #[test]
    fn whole_wave_source_pipeline_matches_independent_retained_implementation() {
        use assembler::extension::Assembler;
        for source in [
            ".text\n.global entry\nentry: addi a0,zero,42\nret\n",
            ".text\n.extern ext\n.global entry\nentry: beq a0,a1,tail\njal ra,ext+4\ntail: ret\n.data\n.word entry+4,ext\n.quad entry\n.bss\n.space 1099511627776\n",
            ".text\nstart: nop\n.balign 16\nbeqz a0,start\nfence rw,0\njalr a0,12(sp)\n",
            ".data\n.byte 255,-128\n.hword 65535,-32768\n.word 4294967295\n.quad 18446744073709551615\n",
            ".section .custom\n.zero 13\n.balign 8\n.global data\ndata: .quad 1\n",
        ] {
            let actual = RiscV64.assemble("test.s", source, &AssemblyLimits::default()).unwrap();
            let expected = assembler::fixed::reference::assemble(&Reference, "test.s", source,
                &AssemblyLimits::default()).unwrap();
            let expected = snapshot(expected);
            assert_eq!(snapshot(actual), expected, "{source}");
            // Public FixedIsa::encode remains native when invoked by the
            // retained generic driver, exercising the Rust Context snapshot.
            let compatibility = assembler::fixed::reference::assemble(&RiscV64, "test.s", source,
                &AssemblyLimits::default()).unwrap();
            assert_eq!(snapshot(compatibility), expected, "compatibility: {source}");

        }
    }

    #[test]
    fn native_errors_preserve_reference_diagnostics_and_source_ranges() {
        use assembler::extension::Assembler;
        for source in [
            "addi x01,a0,1\n",
            "addi a0,a1,2048\n",
            "lui bad,-1\n",
            "slli bad,bad,64\n",
            "lw bad,2048(sp)\n",
            "fence RW,0\n",
            "jal ra,3\n",
            "beq a0,a1,4096\n",
            ".global absent\n",
            "label: nop\nlabel: ret\n",
            ".bss\n.word 1\n",
            ".data\n.byte 256\n",
            "mul a0,a1,a2\n",
            ".unknown 1\n",
        ] {
            let actual =
                RiscV64.assemble("test.s", source, &AssemblyLimits::default()).err().unwrap();
            let expected = assembler::fixed::reference::assemble(
                &Reference,
                "test.s",
                source,
                &AssemblyLimits::default(),
            )
            .err()
            .unwrap();
            assert_eq!(actual.to_string(), expected.to_string(), "{source}");
        }
    }
}
