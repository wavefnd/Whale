// SPDX-License-Identifier: MPL-2.0
//! AArch64 assembler entry point; instruction handling lives in the ISA modules.
use assembler::{
    fixed::{Context, FixedIsa, Instruction},
    AsmError, AssemblerOutput, AssemblyLimits,
};

#[cfg(any(not(whale_wave_aarch64), all(test, feature = "rust-reference")))]
mod encoder;

#[cfg(any(not(whale_wave_aarch64), all(test, feature = "rust-reference")))]
mod encoding;

#[cfg(any(not(whale_wave_aarch64), all(test, feature = "rust-reference")))]
mod parser;

#[cfg(any(not(whale_wave_aarch64), all(test, feature = "rust-reference")))]
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

/// The implementation actually selected by this owning module.
pub fn module_identity() -> &'static str {
    #[cfg(whale_wave_aarch64)]
    {
        unsafe extern "C" {
            fn whale_aarch64_module_identity() -> u64;
        }
        // This no-argument stamp is exported by this module's guarded archive.
        if unsafe { whale_aarch64_module_identity() } == 6289348675458121985 {
            "wave"
        } else {
            "invalid-wave"
        }
    }
    #[cfg(not(whale_wave_aarch64))]
    {
        "rust-reference"
    }
}

#[cfg(test)]
mod implementation_identity_tests {
    #[test]
    fn selected_implementation_has_its_own_compiled_identity() {
        assert_eq!(
            super::module_identity(),
            if cfg!(whale_wave_aarch64) { "wave" } else { "rust-reference" }
        );
    }
}
