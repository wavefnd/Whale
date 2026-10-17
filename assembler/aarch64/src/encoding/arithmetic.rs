// SPDX-License-Identifier: MPL-2.0
//! ADD/SUB and comparison aliases, including SP/ZR and shift restrictions.
use super::sf;

use crate::parser::{no_sp, reg, same, shift};

use assembler::{
    fixed::{self, error, unsigned, Instruction},
    AsmError,
};

pub(crate) fn encode(inst: &Instruction, compare: bool) -> Result<u32, AsmError> {
    let m = inst.mnemonic.as_str();
    let sub = matches!(m, "sub" | "subs" | "cmp");
    let flags = compare || m.ends_with('s');
    let mut a = inst.operands.clone();
    if compare {
        if a.is_empty() {
            return Err(error("comparison requires operands"));
        }
        let n = reg(&a[0])?;
        a.insert(0, if n.width == 64 { "xzr" } else { "wzr" }.into());
    }
    if !(3..=4).contains(&a.len()) {
        return Err(error("arithmetic requires three operands and optional shift"));
    }
    let rd = reg(&a[0])?;
    let rn = reg(&a[1])?;
    same(rd, rn)?;
    let operation = u32::from(sub) << 30 | u32::from(flags) << 29;
    if fixed::integer(&a[2]).is_ok() {
        if rn.zero || (flags && rd.sp) || (!flags && rd.zero) {
            return Err(error(
                "immediate arithmetic uses SP for register 31, except the flag-setting destination",
            ));
        }
        let imm = unsigned(&a[2], 12)?;
        let sh = if a.len() == 4 {
            let (kind, amount) = shift(&a[3], 64)?;
            if kind != 0 || !matches!(amount, 0 | 12) {
                return Err(error("immediate add/sub accepts only lsl #0 or #12"));
            }
            u32::from(amount == 12)
        } else {
            0
        };
        Ok(0x11000000 | sf(rd) | operation | sh << 22 | imm << 10 | rn.index << 5 | rd.index)
    } else {
        no_sp(rd)?;
        no_sp(rn)?;
        let rm = no_sp(reg(&a[2])?)?;
        same(rd, rm)?;
        let (kind, amount) = if a.len() == 4 { shift(&a[3], rd.width)? } else { (0, 0) };
        if kind == 3 {
            return Err(error("add/sub does not support ror"));
        }
        Ok(0x0b000000
            | sf(rd)
            | operation
            | kind << 22
            | rm.index << 16
            | amount << 10
            | rn.index << 5
            | rd.index)
    }
}
