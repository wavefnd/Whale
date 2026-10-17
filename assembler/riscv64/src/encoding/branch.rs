// SPDX-License-Identifier: MPL-2.0
//! Checked J/B displacements and noncontiguous immediate field packing.
use assembler::{fixed, AsmError};

pub(crate) fn jump(rd: u32, displacement: i64) -> Result<u32, AsmError> {
    fixed::displacement(displacement, 20, 2)?;
    let imm = displacement as u32;
    Ok(0x6f
        | rd << 7
        | ((imm >> 20) & 1) << 31
        | ((imm >> 1) & 0x3ff) << 21
        | ((imm >> 11) & 1) << 20
        | ((imm >> 12) & 0xff) << 12)
}

pub(crate) fn branch(f3: u32, rs1: u32, rs2: u32, displacement: i64) -> Result<u32, AsmError> {
    fixed::displacement(displacement, 12, 2)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jal_checks_signed_21_bit_byte_displacement() {
        for offset in [-(1 << 20), -2, 0, (1 << 20) - 2] {
            assert!(jump(1, offset).is_ok(), "{offset}");
        }
        for offset in [-(1 << 20) - 2, 1 << 20, -1, 1] {
            assert!(jump(1, offset).is_err(), "{offset}");
        }
    }

    #[test]
    fn conditional_branch_has_its_own_narrower_range() {
        for offset in [-4096, -2, 0, 4094] {
            assert!(branch(0, 1, 2, offset).is_ok(), "{offset}");
        }
        for offset in [-4098, 4096, -1, 1] {
            assert!(branch(0, 1, 2, offset).is_err(), "{offset}");
        }
    }
}
