// SPDX-License-Identifier: MPL-2.0
//! RV64I instruction selection, single-instruction aliases and relocation resolution.
use crate::{
    encoding::{
        branch::{branch, jump},
        formats::{self, i, r},
    },
    parser::{fence_mask, memory, reg},
    tables,
};

use assembler::{
    fixed::{count, error, signed, unsigned, Context, Instruction},
    AsmError,
};

pub(crate) fn encode(inst: &Instruction, cx: &mut Context<'_>) -> Result<u32, AsmError> {
    let m = inst.mnemonic.as_str();
    if let Some((opcode, f3, f7)) = tables::register(m) {
        let a = count(inst, 3)?;
        return Ok(r(opcode, f3, f7, reg(&a[0])?, reg(&a[1])?, reg(&a[2])?));
    }
    if let Some((opcode, f3)) = tables::immediate(m) {
        let a = count(inst, 3)?;
        return Ok(i(opcode, f3, reg(&a[0])?, reg(&a[1])?, signed(&a[2], 12)?));
    }
    if let Some(f3) = tables::branch(m) {
        let a = count(inst, 3)?;
        return branch(
            f3,
            reg(&a[0])?,
            reg(&a[1])?,
            cx.pc_relative(&a[2], tables::BRANCH_RELOCATION)?,
        );
    }
    if let Some(f3) = tables::load(m) {
        let a = count(inst, 2)?;
        let (base, imm) = memory(&a[1])?;
        return Ok(i(3, f3, reg(&a[0])?, base, imm));
    }
    if let Some(f3) = tables::store(m) {
        let a = count(inst, 2)?;
        let (base, imm) = memory(&a[1])?;
        return Ok(formats::store(f3, base, reg(&a[0])?, imm));
    }
    match m {
        "nop" | "ret" | "ecall" | "ebreak" => {
            count(inst, 0)?;
            Ok(match m {
                "nop" => 0x13,
                "ret" => 0x8067,
                "ecall" => 0x73,
                _ => 0x100073,
            })
        },
        "mv" | "not" | "neg" | "negw" => {
            let a = count(inst, 2)?;
            let rd = reg(&a[0])?;
            let rs = reg(&a[1])?;
            Ok(match m {
                "mv" => i(0x13, 0, rd, rs, 0),
                "not" => i(0x13, 4, rd, rs, 0xfff),
                "neg" => r(0x33, 0, 0x20, rd, 0, rs),
                _ => r(0x3b, 0, 0x20, rd, 0, rs),
            })
        },
        "li" => {
            let a = count(inst, 2)?;
            Ok(i(0x13, 0, reg(&a[0])?, 0, signed(&a[1], 12)?))
        },
        "jr" => {
            let a = count(inst, 1)?;
            Ok(i(0x67, 0, 0, reg(&a[0])?, 0))
        },
        "j" => {
            let a = count(inst, 1)?;
            jump(0, cx.pc_relative(&a[0], tables::JAL_RELOCATION)?)
        },
        "jal" => {
            if inst.operands.len() == 1 {
                return jump(1, cx.pc_relative(&inst.operands[0], tables::JAL_RELOCATION)?);
            }
            let a = count(inst, 2)?;
            jump(reg(&a[0])?, cx.pc_relative(&a[1], tables::JAL_RELOCATION)?)
        },
        "jalr" => {
            if inst.operands.len() == 1 {
                return Ok(i(0x67, 0, 1, reg(&inst.operands[0])?, 0));
            }
            if inst.operands.len() == 2 {
                let a = count(inst, 2)?;
                let (base, imm) = memory(&a[1])?;
                return Ok(i(0x67, 0, reg(&a[0])?, base, imm));
            }
            let a = count(inst, 3)?;
            Ok(i(0x67, 0, reg(&a[0])?, reg(&a[1])?, signed(&a[2], 12)?))
        },
        "beqz" | "bnez" => {
            let a = count(inst, 2)?;
            branch(
                u32::from(m == "bnez"),
                reg(&a[0])?,
                0,
                cx.pc_relative(&a[1], tables::BRANCH_RELOCATION)?,
            )
        },
        "lui" | "auipc" => {
            let a = count(inst, 2)?;
            let value = unsigned(&a[1], 20)?;
            Ok(formats::upper(if m == "lui" { 0x37 } else { 0x17 }, reg(&a[0])?, value))
        },
        "slli" | "srli" | "srai" | "slliw" | "srliw" | "sraiw" => {
            let a = count(inst, 3)?;
            let word = m.ends_with('w');
            let shift = unsigned(&a[2], if word { 5 } else { 6 })?;
            Ok(i(
                if word { 0x1b } else { 0x13 },
                if m.starts_with("sll") { 1 } else { 5 },
                reg(&a[0])?,
                reg(&a[1])?,
                shift | if m.starts_with("sra") { 0x400 } else { 0 },
            ))
        },
        "fence" => {
            if inst.operands.is_empty() {
                return Ok(0x0ff0000f);
            }
            let a = count(inst, 2)?;
            Ok(0x0f | fence_mask(&a[0])? << 24 | fence_mask(&a[1])? << 20)
        },
        _ => Err(error(format!("unsupported RV64I instruction {m:?}"))),
    }
}
