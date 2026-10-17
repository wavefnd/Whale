// SPDX-License-Identifier: MPL-2.0
//! LoongArch64 three-register and immediate instruction field packing.
pub(crate) fn three(opcode: u32, rd: u32, rj: u32, rk: u32) -> u32 {
    opcode | rd | rj << 5 | rk << 10
}

pub(crate) fn immediate(opcode: u32, rd: u32, rj: u32, imm: u32) -> u32 {
    opcode | rd | rj << 5 | imm << 10
}

pub(crate) fn upper(opcode: u32, rd: u32, imm: u32) -> u32 {
    opcode | rd | (imm & 0xfffff) << 5
}
