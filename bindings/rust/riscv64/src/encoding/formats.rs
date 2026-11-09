// SPDX-License-Identifier: MPL-2.0
//! RV64I R/I/S/U instruction field packing; operands are validated by the encoder.
pub(crate) fn r(opcode: u32, f3: u32, f7: u32, rd: u32, rs1: u32, rs2: u32) -> u32 {
    opcode | rd << 7 | f3 << 12 | rs1 << 15 | rs2 << 20 | f7 << 25
}

pub(crate) fn i(opcode: u32, f3: u32, rd: u32, rs1: u32, imm: u32) -> u32 {
    opcode | rd << 7 | f3 << 12 | rs1 << 15 | imm << 20
}

pub(crate) fn store(f3: u32, rs1: u32, rs2: u32, imm: u32) -> u32 {
    0x23 | (imm & 31) << 7 | f3 << 12 | rs1 << 15 | rs2 << 20 | (imm >> 5) << 25
}

pub(crate) fn upper(opcode: u32, rd: u32, imm: u32) -> u32 {
    opcode | rd << 7 | (imm & 0xfffff) << 12
}
