// SPDX-License-Identifier: MPL-2.0
//! Typed transport and diagnostics for the complete native Wave LoongArch64 module.
use assembler::{
    fixed::{native, Context, Instruction},
    AsmError, AssemblerOutput, AssemblyLimits,
};

unsafe extern "C" {
    fn whale_loongarch64_assemble(
        packet: *const native::FixedInput,
        result: *mut native::FixedResult,
    ) -> i32;

    fn whale_loongarch64_encode(
        instruction: *const native::FixedInstruction,
        context: *mut native::FixedContext,
        result: *mut native::FixedResult,
    ) -> i32;
}

fn render(code: u64, text: &str, _detail: u64) -> AsmError {
    match code {
        3000 => AsmError::EncodeError(format!("invalid LoongArch64 register {text:?}")),
        3001 => AsmError::EncodeError("upper immediate must fit signed 20 bits".into()),
        3002 => AsmError::EncodeError("jirl displacement exceeds i64".into()),
        3003 => AsmError::EncodeError(format!("unsupported LoongArch64 instruction {text:?}")),
        _ => AsmError::Backend("LoongArch64 native transport"),
    }
}

pub(super) fn assemble(
    name: &str,
    source: &str,
    limits: &AssemblyLimits,
) -> Result<AssemblerOutput, AsmError> {
    native::assemble(name, source, limits, "loongarch64", true, whale_loongarch64_assemble, render)
}

pub(super) fn encode(
    instruction: &Instruction,
    context: &mut Context<'_>,
) -> Result<u32, AsmError> {
    native::encode(instruction, context, whale_loongarch64_encode, render)
}
