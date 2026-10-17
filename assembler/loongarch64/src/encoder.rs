// SPDX-License-Identifier: MPL-2.0
//! LoongArch64 instruction selection, immediate validation and relocation resolution.
use crate::{
    encoding::{branch, formats},
    parser::reg,
    tables,
};

use assembler::{
    fixed::{self, count, error, signed, unsigned, Context, Instruction},
    AsmError,
};

pub(crate) fn encode(inst: &Instruction, cx: &mut Context<'_>) -> Result<u32, AsmError> {
    let m = inst.mnemonic.as_str();
    if let Some(opcode) = tables::three_register(m) {
        let a = count(inst, 3)?;
        return Ok(formats::three(opcode, reg(&a[0])?, reg(&a[1])?, reg(&a[2])?));
    }
    if let Some((opcode, unsigned_immediate)) = tables::immediate(m) {
        let a = count(inst, 3)?;
        let imm = if unsigned_immediate { unsigned(&a[2], 12)? } else { signed(&a[2], 12)? };
        return Ok(formats::immediate(opcode, reg(&a[0])?, reg(&a[1])?, imm));
    }
    if let Some((opcode, bits)) = tables::shift(m) {
        let a = count(inst, 3)?;
        return Ok(formats::immediate(opcode, reg(&a[0])?, reg(&a[1])?, unsigned(&a[2], bits)?));
    }
    if let Some(opcode) = tables::memory(m) {
        let a = count(inst, 3)?;
        return Ok(formats::immediate(opcode, reg(&a[0])?, reg(&a[1])?, signed(&a[2], 12)?));
    }
    if let Some(opcode) = tables::conditional(m) {
        let a = count(inst, 3)?;
        return branch::conditional(
            opcode,
            reg(&a[0])?,
            reg(&a[1])?,
            cx.pc_relative(&a[2], tables::B16_RELOCATION)?,
        );
    }
    match m {
        "nop" | "ret" => {
            count(inst, 0)?;
            Ok(if m == "nop" { 0x03400000 } else { 0x4c000020 })
        },
        "move" => {
            let a = count(inst, 2)?;
            Ok(0x00150000 | reg(&a[0])? | reg(&a[1])? << 5)
        },
        "li.d" | "li.w" => {
            let a = count(inst, 2)?;
            Ok(if m == "li.d" { 0x02c00000 } else { 0x02800000 }
                | reg(&a[0])?
                | signed(&a[1], 12)? << 10)
        },
        "lu12i.w" | "lu32i.d" => {
            let a = count(inst, 2)?;
            let n = fixed::integer(&a[1])?;
            if !(-(1 << 19)..(1 << 19)).contains(&n) {
                return Err(error("upper immediate must fit signed 20 bits"));
            }
            Ok(formats::upper(
                if m == "lu12i.w" { 0x14000000 } else { 0x16000000 },
                reg(&a[0])?,
                n as u32,
            ))
        },
        "b" | "bl" => {
            let a = count(inst, 1)?;
            branch::direct(m == "bl", cx.pc_relative(&a[0], tables::B26_RELOCATION)?)
        },
        "beqz" | "bnez" => {
            let a = count(inst, 2)?;
            branch::zero(m == "bnez", reg(&a[0])?, cx.pc_relative(&a[1], tables::B21_RELOCATION)?)
        },
        "jirl" => {
            let a = count(inst, 3)?;
            let value = i64::try_from(fixed::integer(&a[2])?)
                .map_err(|_| error("jirl displacement exceeds i64"))?;
            branch::indirect(reg(&a[0])?, reg(&a[1])?, value)
        },
        "syscall" | "break" => {
            let a = count(inst, 1)?;
            Ok(if m == "syscall" { 0x002b0000 } else { 0x002a0000 } | unsigned(&a[0], 15)?)
        },
        _ => Err(error(format!("unsupported LoongArch64 instruction {m:?}"))),
    }
}
