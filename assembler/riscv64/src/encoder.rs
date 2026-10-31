// SPDX-License-Identifier: MPL-2.0
//! RV64I instruction transport; product instruction policy is Wave source.
use assembler::{
    fixed::{Context, Instruction},
    AsmError,
};

#[cfg(any(all(test, feature = "rust-reference"), not(whale_wave_riscv64)))]
#[path = "encoder_reference.rs"]
pub(crate) mod reference;

pub(crate) fn encode(
    instruction: &Instruction,
    context: &mut Context<'_>,
) -> Result<u32, AsmError> {
    #[cfg(whale_wave_riscv64)]
    return crate::wave::encode(instruction, context);
    #[cfg(not(whale_wave_riscv64))]
    reference::encode(instruction, context)
}
