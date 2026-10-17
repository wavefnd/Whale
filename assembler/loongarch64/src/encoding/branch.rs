// SPDX-License-Identifier: MPL-2.0
//! LoongArch B16/B21/B26 scaled offsets, split fields and JIRL encoding.
use assembler::{fixed::displacement, AsmError};

pub(crate) fn direct(link: bool, offset: i64) -> Result<u32, AsmError> {
    let imm = displacement(offset, 26, 4)?;
    Ok(if link { 0x54000000 } else { 0x50000000 } | (imm & 0xffff) << 10 | imm >> 16)
}

pub(crate) fn conditional(opcode: u32, rj: u32, rd: u32, offset: i64) -> Result<u32, AsmError> {
    Ok(opcode | rd | rj << 5 | displacement(offset, 16, 4)? << 10)
}

pub(crate) fn zero(nonzero: bool, rj: u32, offset: i64) -> Result<u32, AsmError> {
    let imm = displacement(offset, 21, 4)?;
    Ok(if nonzero { 0x44000000 } else { 0x40000000 } | rj << 5 | (imm & 0xffff) << 10 | imm >> 16)
}

pub(crate) fn indirect(rd: u32, rj: u32, offset: i64) -> Result<u32, AsmError> {
    conditional(0x4c000000, rj, rd, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b26_and_b21_enforce_distinct_scaled_ranges() {
        for offset in [-(1 << 27), -4, 0, (1 << 27) - 4] {
            assert!(direct(false, offset).is_ok(), "{offset}");
        }
        for offset in [-(1 << 27) - 4, 1 << 27, -2, 2] {
            assert!(direct(true, offset).is_err(), "{offset}");
        }
        assert!(zero(false, 4, -(1 << 22)).is_ok());
        assert!(zero(false, 4, (1 << 22) - 4).is_ok());
        assert!(zero(false, 4, 1 << 22).is_err());
        assert!(zero(false, 4, 2).is_err());
    }

    #[test]
    fn jirl_and_b16_reject_unaligned_or_overflowing_offsets() {
        for offset in [-(1 << 17), -4, 0, (1 << 17) - 4] {
            assert!(indirect(1, 2, offset).is_ok(), "{offset}");
            assert!(conditional(0x58000000, 1, 2, offset).is_ok(), "{offset}");
        }
        for offset in [-(1 << 17) - 4, 1 << 17, -2, 2] {
            assert!(indirect(1, 2, offset).is_err(), "{offset}");
            assert!(conditional(0x58000000, 1, 2, offset).is_err(), "{offset}");
        }
    }
}
