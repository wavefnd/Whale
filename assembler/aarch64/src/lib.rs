// SPDX-License-Identifier: MPL-2.0
//! AArch64 assembler entry point; instruction handling lives in the ISA modules.
use assembler::{
    fixed::{Context, FixedIsa, Instruction},
    AsmError, AssemblerOutput, AssemblyLimits,
};

#[cfg(any(not(whale_wave_aarch64), test))]
mod encoder;

#[cfg(any(not(whale_wave_aarch64), test))]
mod encoding;

#[cfg(any(not(whale_wave_aarch64), test))]
mod parser;

#[cfg(any(not(whale_wave_aarch64), test))]
mod tables;

#[cfg(whale_wave_aarch64)]
mod wave;

pub struct AArch64;

impl assembler::extension::Assembler for AArch64 {
    fn id(&self) -> &str {
        "aarch64"
    }

    fn object_target(&self) -> Option<target_spec::ObjectTarget> {
        Some(target_spec::ObjectTarget::elf64(target_spec::Machine::AArch64))
    }

    fn assemble(
        &self,
        name: &str,
        source: &str,
        limits: &AssemblyLimits,
    ) -> Result<AssemblerOutput, AsmError> {
        #[cfg(whale_wave_aarch64)]
        {
            wave::assemble(name, source, limits)
        }
        #[cfg(not(whale_wave_aarch64))]
        {
            assembler::fixed::reference::assemble(self, name, source, limits)
        }
    }
}

impl FixedIsa for AArch64 {
    fn id(&self) -> &'static str {
        "aarch64"
    }

    fn hash_comments(&self) -> bool {
        false
    }

    #[cfg(whale_wave_aarch64)]
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
        #[cfg(whale_wave_aarch64)]
        {
            wave::encode(instruction, context)
        }
        #[cfg(not(whale_wave_aarch64))]
        {
            encoder::encode(instruction, context)
        }
    }
}
