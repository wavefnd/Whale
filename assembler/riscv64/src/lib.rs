// SPDX-License-Identifier: MPL-2.0
//! RiscV64 assembler entry point; instruction handling lives in the ISA modules.
use assembler::{
    fixed::{self, Context, FixedIsa, Instruction},
    AsmError, AssemblerOutput, AssemblyLimits,
};

mod encoder;

mod encoding;

mod parser;

mod tables;

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
        fixed::assemble(self, name, source, limits)
    }
}

impl FixedIsa for RiscV64 {
    fn id(&self) -> &'static str {
        "riscv64"
    }

    fn encode(
        &self,
        instruction: &Instruction,
        context: &mut Context<'_>,
    ) -> Result<u32, AsmError> {
        encoder::encode(instruction, context)
    }
}
