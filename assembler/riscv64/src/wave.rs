// SPDX-License-Identifier: MPL-2.0
//! RV64I typed transport. Classification, validation and encoding run in Wave.
use assembler::fixed::{native, Context, Instruction};

use assembler::{AsmError, AssemblerOutput, AssemblyLimits};

#[cfg(all(test, feature = "rust-reference"))]
use native::FixedText;

use native::{FixedContext, FixedInput, FixedInstruction, FixedResult};

unsafe extern "C" {
    fn whale_riscv64_entry(input: *const FixedInput, result: *mut FixedResult) -> i32;

    fn whale_riscv64_instruction(
        instruction: *const FixedInstruction,
        context: *mut FixedContext,
        result: *mut FixedResult,
    ) -> i32;

    #[cfg(all(test, feature = "rust-reference"))]
    fn whale_riscv64_operand(mode: u64, text: *const FixedText, result: *mut FixedResult) -> i32;

    #[cfg(all(test, feature = "rust-reference"))]
    fn whale_riscv64_branch(
        jump: u64,
        f3: u64,
        rs1: u64,
        rs2: u64,
        displacement: i64,
        result: *mut FixedResult,
    ) -> i32;
}

pub(crate) fn render(code: u64, text: &str, _: u64) -> AsmError {
    assembler::fixed::error(match code {
        2000 => format!("invalid RV64 register {text:?}"),
        2001 => "memory operand requires offset(base)".into(),
        2002 => "invalid memory operand".into(),
        2003 => "fence sets use i,o,r,w or literal 0".into(),
        2004 => "duplicate fence flag".into(),
        2006 => format!("unsupported RV64I instruction {text:?}"),
        _ => return AsmError::Backend("RV64I diagnostic transport"),
    })
}

pub(crate) fn assemble(
    name: &str,
    source: &str,
    limits: &AssemblyLimits,
) -> Result<AssemblerOutput, AsmError> {
    native::assemble(name, source, limits, "riscv64", true, whale_riscv64_entry, render)
}

pub(crate) fn encode(
    instruction: &Instruction,
    context: &mut Context<'_>,
) -> Result<u32, AsmError> {
    native::encode(instruction, context, whale_riscv64_instruction, render)
}

#[cfg(all(test, feature = "rust-reference"))]
fn operand(mode: u64, text: &str) -> Result<(u64, u64), AsmError> {
    let text_record = FixedText::new(text);
    let mut result = FixedResult::new();
    let status = unsafe { whale_riscv64_operand(mode, &text_record, &mut result) };
    native::check(status, &result, &[text], render)?;
    Ok((result.word, result.detail))
}

#[cfg(all(test, feature = "rust-reference"))]
pub(crate) fn reg(text: &str) -> Result<u32, AsmError> {
    let (word, detail) = operand(0, text)?;
    if word >= 32 || detail != 0 {
        return Err(AsmError::Backend("RV64I register transport"));
    }
    Ok(word as u32)
}

#[cfg(all(test, feature = "rust-reference"))]
pub(crate) fn memory(text: &str) -> Result<(u32, u32), AsmError> {
    let (word, detail) = operand(1, text)?;
    if word >= 32 || detail >= 4096 {
        return Err(AsmError::Backend("RV64I memory transport"));
    }
    Ok((word as u32, detail as u32))
}

#[cfg(all(test, feature = "rust-reference"))]
pub(crate) fn fence_mask(text: &str) -> Result<u32, AsmError> {
    let (word, detail) = operand(2, text)?;
    if word >= 16 || detail != 0 {
        return Err(AsmError::Backend("RV64I fence transport"));
    }
    Ok(word as u32)
}

#[cfg(all(test, feature = "rust-reference"))]
pub(crate) fn branch(
    jump: bool,
    f3: u32,
    rs1: u32,
    rs2: u32,
    displacement: i64,
) -> Result<u32, AsmError> {
    let mut result = FixedResult::new();
    let status = unsafe {
        whale_riscv64_branch(
            u64::from(jump),
            f3.into(),
            rs1.into(),
            rs2.into(),
            displacement,
            &mut result,
        )
    };
    native::check(status, &result, &[], render)?;
    u32::try_from(result.word).map_err(|_| AsmError::Backend("RV64I branch transport"))
}
