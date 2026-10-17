// SPDX-License-Identifier: MPL-2.0
//! A64 branch ranges, scaled immediates and split ADR immediate fields.
use assembler::{fixed::displacement, AsmError};

pub(crate) fn direct(link: bool, offset: i64) -> Result<u32, AsmError> {
    Ok(if link { 0x94000000 } else { 0x14000000 } | displacement(offset, 26, 4)?)
}

pub(crate) fn conditional(condition: u32, offset: i64) -> Result<u32, AsmError> {
    Ok(0x54000000 | displacement(offset, 19, 4)? << 5 | condition)
}

pub(crate) fn compare(
    nonzero: bool,
    wide: bool,
    register: u32,
    offset: i64,
) -> Result<u32, AsmError> {
    Ok(0x34000000
        | u32::from(wide) << 31
        | u32::from(nonzero) << 24
        | displacement(offset, 19, 4)? << 5
        | register)
}

pub(crate) fn address(register: u32, offset: i64) -> Result<u32, AsmError> {
    let imm = displacement(offset, 21, 1)?;
    Ok(0x10000000 | (imm & 3) << 29 | (imm >> 2) << 5 | register)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_branch_enforces_signed_scaled_range() {
        for offset in [-(1 << 27), -4, 0, (1 << 27) - 4] {
            assert!(direct(false, offset).is_ok(), "{offset}");
        }
        for offset in [-(1 << 27) - 4, 1 << 27, -2, 2] {
            assert!(direct(true, offset).is_err(), "{offset}");
        }
    }

    #[test]
    fn conditional_and_adr_have_separate_alignment_rules() {
        assert!(conditional(0, -(1 << 20)).is_ok());
        assert!(conditional(0, (1 << 20) - 4).is_ok());
        assert!(conditional(0, 1 << 20).is_err());
        assert!(compare(false, true, 0, 2).is_err());
        assert!(address(0, -1).is_ok());
        assert!(address(0, (1 << 20) - 1).is_ok());
        assert!(address(0, 1 << 20).is_err());
    }
}
