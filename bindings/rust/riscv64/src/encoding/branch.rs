// SPDX-License-Identifier: MPL-2.0
//! Direct native branch validation and field packing regression boundary.
use assembler::AsmError;

pub(crate) fn jump(rd: u32, displacement: i64) -> Result<u32, AsmError> {
    #[cfg(whale_wave_riscv64)]
    return crate::wave::branch(true, 0, rd, 0, displacement);
    #[cfg(not(whale_wave_riscv64))]
    super::reference::branch::jump(rd, displacement)
}

pub(crate) fn branch(f3: u32, rs1: u32, rs2: u32, displacement: i64) -> Result<u32, AsmError> {
    #[cfg(whale_wave_riscv64)]
    return crate::wave::branch(false, f3, rs1, rs2, displacement);
    #[cfg(not(whale_wave_riscv64))]
    super::reference::branch::branch(f3, rs1, rs2, displacement)
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
