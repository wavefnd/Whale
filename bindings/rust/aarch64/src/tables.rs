// SPDX-License-Identifier: MPL-2.0
//! A64 register aliases, opcode selections, branch conditions and ELF relocation IDs.
pub(crate) fn register_alias(name: &str) -> Option<(u32, u32, bool, bool)> {
    Some(match name {
        "sp" => (31, 64, true, false),
        "wsp" => (31, 32, true, false),
        "xzr" => (31, 64, false, true),
        "wzr" => (31, 32, false, true),
        "fp" => (29, 64, false, false),
        "lr" => (30, 64, false, false),
        _ => return None,
    })
}

pub(crate) fn condition(name: &str) -> Option<u32> {
    Some(match name {
        "eq" => 0,
        "ne" => 1,
        "cs" | "hs" => 2,
        "cc" | "lo" => 3,
        "mi" => 4,
        "pl" => 5,
        "vs" => 6,
        "vc" => 7,
        "hi" => 8,
        "ls" => 9,
        "ge" => 10,
        "lt" => 11,
        "gt" => 12,
        "le" => 13,
        "al" => 14,
        "nv" => 15,
        _ => return None,
    })
}

pub(crate) fn indirect(name: &str) -> u32 {
    match name {
        "ret" => 0xd65f0000,
        "br" => 0xd61f0000,
        _ => 0xd63f0000,
    }
}

pub(crate) fn wide_move(name: &str) -> u32 {
    match name {
        "movz" => 0x52800000,
        "movn" => 0x12800000,
        _ => 0x72800000,
    }
}

pub(crate) fn logical(name: &str) -> u32 {
    match name {
        "and" => 0x0a000000,
        "orr" => 0x2a000000,
        "eor" => 0x4a000000,
        _ => 0x6a000000,
    }
}

pub(crate) const ADR_RELOCATION: u32 = 274;

pub(crate) const CONDBR19_RELOCATION: u32 = 280;

pub(crate) const JUMP26_RELOCATION: u32 = 282;

pub(crate) const CALL26_RELOCATION: u32 = 283;
