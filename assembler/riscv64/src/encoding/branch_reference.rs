// SPDX-License-Identifier: MPL-2.0
//! Checked J/B displacements and noncontiguous immediate field packing.
use assembler::{fixed, AsmError};

pub(crate) fn jump(rd: u32, displacement: i64) -> Result<u32, AsmError> {
    fixed::reference::displacement(displacement, 20, 2)?;
    let imm = displacement as u32;
    Ok(0x6f
        | rd << 7
        | ((imm >> 20) & 1) << 31
        | ((imm >> 1) & 0x3ff) << 21
        | ((imm >> 11) & 1) << 20
        | ((imm >> 12) & 0xff) << 12)
}

pub(crate) fn branch(f3: u32, rs1: u32, rs2: u32, displacement: i64) -> Result<u32, AsmError> {
    fixed::reference::displacement(displacement, 12, 2)?;
    let imm = displacement as u32;
    Ok(0x63
        | ((imm >> 11) & 1) << 7
        | ((imm >> 1) & 15) << 8
        | f3 << 12
        | rs1 << 15
        | rs2 << 20
        | ((imm >> 5) & 63) << 25
        | ((imm >> 12) & 1) << 31)
}
