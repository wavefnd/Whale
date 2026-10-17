// SPDX-License-Identifier: MPL-2.0
//! A64 instruction-family encoding and checked displacement fields.
pub(crate) mod arithmetic;

pub(crate) mod branch;

pub(crate) mod memory;

pub(crate) fn sf(register: crate::parser::Reg) -> u32 {
    u32::from(register.width == 64) << 31
}
