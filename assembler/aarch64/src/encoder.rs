// SPDX-License-Identifier: MPL-2.0
//! A64 instruction selection, pseudo expansion and operand validation.
use crate::{
    encoding::{arithmetic, branch, memory, sf},
    parser::{no_sp, reg, same, shift},
    tables,
};

use assembler::{
    fixed::{self, count, error, unsigned, Context, Instruction},
    AsmError,
};

pub(crate) fn encode(inst: &Instruction, cx: &mut Context<'_>) -> Result<u32, AsmError> {
    let m = inst.mnemonic.as_str();
    match m {
        "nop" => {
            count(inst, 0)?;
            Ok(0xd503201f)
        },
        "ret" | "br" | "blr" => {
            let register = if m == "ret" && inst.operands.is_empty() {
                reg("x30")?
            } else {
                reg(&count(inst, 1)?[0])?
            };
            if register.width != 64 || register.index == 31 {
                return Err(error("indirect control flow requires X0..X30"));
            }
            Ok(tables::indirect(m) | register.index << 5)
        },
        "svc" | "brk" => {
            let a = count(inst, 1)?;
            Ok(if m == "svc" { 0xd4000001 } else { 0xd4200000 } | unsigned(&a[0], 16)? << 5)
        },
        "mov" => {
            let a = count(inst, 2)?;
            let rd = reg(&a[0])?;
            if let Ok(value) = fixed::integer(&a[1]) {
                no_sp(rd)?;
                let (opcode, bits) = if (0..65536).contains(&value) {
                    (0x52800000, value as u32)
                } else if (-65536..0).contains(&value) {
                    (0x12800000, (!value as u32) & 0xffff)
                } else {
                    return Err(error("mov immediate requires a single 16-bit MOVZ/MOVN; use movz/movk for larger values"));
                };
                Ok(opcode | sf(rd) | bits << 5 | rd.index)
            } else {
                let rn = reg(&a[1])?;
                same(rd, rn)?;
                if rd.sp || rn.sp {
                    if rd.zero || rn.zero {
                        return Err(error("mov cannot mix SP and ZR"));
                    }
                    Ok(0x11000000 | sf(rd) | rn.index << 5 | rd.index)
                } else {
                    Ok(0x2a0003e0 | sf(rd) | rn.index << 16 | rd.index)
                }
            }
        },
        "movz" | "movn" | "movk" => {
            if !(2..=3).contains(&inst.operands.len()) {
                return Err(error("wide move requires register, imm16 and optional lsl"));
            }
            let a = &inst.operands;
            let rd = no_sp(reg(&a[0])?)?;
            let value = unsigned(&a[1], 16)?;
            let shift = if a.len() == 3 {
                let (kind, amount) = shift(&a[2], 64)?;
                if kind != 0 || amount % 16 != 0 || amount >= rd.width {
                    return Err(error("wide move shift must be 0/16 for W, or 0/16/32/48 for X"));
                }
                amount / 16
            } else {
                0
            };
            Ok(tables::wide_move(m) | sf(rd) | shift << 21 | value << 5 | rd.index)
        },
        "add" | "adds" | "sub" | "subs" => arithmetic::encode(inst, false),
        "cmp" | "cmn" => arithmetic::encode(inst, true),
        "and" | "ands" | "orr" | "eor" => {
            if !(3..=4).contains(&inst.operands.len()) {
                return Err(error(
                    "logical register operation requires three operands and optional shift",
                ));
            }
            let a = &inst.operands;
            let rd = no_sp(reg(&a[0])?)?;
            let rn = no_sp(reg(&a[1])?)?;
            let rm = no_sp(reg(&a[2])?)?;
            same(rd, rn)?;
            same(rd, rm)?;
            let (kind, amount) = if a.len() == 4 { shift(&a[3], rd.width)? } else { (0, 0) };
            Ok(tables::logical(m)
                | sf(rd)
                | kind << 22
                | rm.index << 16
                | amount << 10
                | rn.index << 5
                | rd.index)
        },
        "lsl" | "lsr" | "asr" => {
            let a = count(inst, 3)?;
            let rd = no_sp(reg(&a[0])?)?;
            let rn = no_sp(reg(&a[1])?)?;
            same(rd, rn)?;
            let amount = unsigned(&a[2], if rd.width == 64 { 6 } else { 5 })?;
            let (immr, imms) = if m == "lsl" {
                ((rd.width - amount) % rd.width, rd.width - 1 - amount)
            } else {
                (amount, rd.width - 1)
            };
            Ok(if m == "asr" { 0x13000000 } else { 0x53000000 }
                | sf(rd)
                | u32::from(rd.width == 64) << 22
                | immr << 16
                | imms << 10
                | rn.index << 5
                | rd.index)
        },
        "ldr" | "str" | "ldrb" | "strb" | "ldrh" | "strh" | "ldrsw" | "ldur" | "stur" => {
            memory::encode(inst)
        },
        "b" | "bl" => {
            let a = count(inst, 1)?;
            let relocation =
                if m == "b" { tables::JUMP26_RELOCATION } else { tables::CALL26_RELOCATION };
            branch::direct(m == "bl", cx.pc_relative(&a[0], relocation)?)
        },
        "cbz" | "cbnz" => {
            let a = count(inst, 2)?;
            let rt = no_sp(reg(&a[0])?)?;
            branch::compare(
                m == "cbnz",
                rt.width == 64,
                rt.index,
                cx.pc_relative(&a[1], tables::CONDBR19_RELOCATION)?,
            )
        },
        "adr" => {
            let a = count(inst, 2)?;
            let rd = no_sp(reg(&a[0])?)?;
            if rd.width != 64 {
                return Err(error("adr requires an X register"));
            }
            branch::address(rd.index, cx.pc_relative(&a[1], tables::ADR_RELOCATION)?)
        },
        _ if m.starts_with("b.") => {
            let condition =
                tables::condition(&m[2..]).ok_or_else(|| error("invalid A64 branch condition"))?;
            let a = count(inst, 1)?;
            branch::conditional(condition, cx.pc_relative(&a[0], tables::CONDBR19_RELOCATION)?)
        },
        _ => Err(error(format!("unsupported A64 instruction {m:?}"))),
    }
}
