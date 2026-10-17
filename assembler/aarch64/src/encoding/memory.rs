// SPDX-License-Identifier: MPL-2.0
//! Scaled and signed-unscaled load/store encoding, with transfer width checks.
use crate::parser::{no_sp, reg};

use assembler::{
    fixed::{count, error, Instruction},
    AsmError,
};

pub(crate) fn encode(inst: &Instruction) -> Result<u32, AsmError> {
    let a = count(inst, 2)?;
    let rt = no_sp(reg(&a[0])?)?;
    let address = crate::parser::memory(&a[1])?;
    let base = address.base;
    let offset = address.offset;
    let m = inst.mnemonic.as_str();
    let load = m.starts_with("ld");
    let unscaled = matches!(m, "ldur" | "stur");
    let size = if m.ends_with('b') {
        0
    } else if m.ends_with('h') {
        1
    } else if rt.width == 64 {
        3
    } else {
        2
    };
    if size < 2 && rt.width != 32 {
        return Err(error("byte/halfword transfer requires a W register"));
    }
    if m == "ldrsw" && rt.width != 64 {
        return Err(error("ldrsw requires an X register"));
    }
    let (opcode, immediate) = if unscaled {
        (0x38000000 | size << 30 | u32::from(load) << 22, signed_offset(offset)? << 12)
    } else {
        let value = offset;
        let size = if m == "ldrsw" { 2 } else { size };
        let scale = 1i128 << size;
        if value < 0 || value % scale != 0 || value / scale > 4095 {
            return Err(error(
                "unsigned load/store offset must be scaled, nonnegative and fit 12 bits",
            ));
        }
        (
            if m == "ldrsw" { 0xb9800000 } else { 0x39000000 | size << 30 | u32::from(load) << 22 },
            (value as u32 / scale as u32) << 10,
        )
    };
    Ok(opcode | immediate | base.index << 5 | rt.index)
}

fn signed_offset(value: i128) -> Result<u32, AsmError> {
    if !(-256..256).contains(&value) {
        return Err(error("unscaled load/store offset must fit signed 9 bits"));
    }
    Ok(value as u32 & 0x1ff)
}
