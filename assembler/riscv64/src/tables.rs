// SPDX-License-Identifier: MPL-2.0
//! RV64I ABI register names, opcode/funct fields and ELF relocation IDs.
pub(crate) const REGISTER_ALIASES: [&str; 32] = [
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
    "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
    "t5", "t6",
];

pub(crate) fn branch(name: &str) -> Option<u32> {
    Some(match name {
        "beq" => 0,
        "bne" => 1,
        "blt" => 4,
        "bge" => 5,
        "bltu" => 6,
        "bgeu" => 7,
        _ => return None,
    })
}

pub(crate) fn immediate(name: &str) -> Option<(u32, u32)> {
    Some(match name {
        "addi" => (0x13, 0),
        "slti" => (0x13, 2),
        "sltiu" => (0x13, 3),
        "xori" => (0x13, 4),
        "ori" => (0x13, 6),
        "andi" => (0x13, 7),
        "addiw" => (0x1b, 0),
        _ => return None,
    })
}

pub(crate) fn register(name: &str) -> Option<(u32, u32, u32)> {
    let opcode = if name.ends_with('w') { 0x3b } else { 0x33 };
    let (f3, f7) = match name {
        "add" | "addw" => (0, 0),
        "sub" | "subw" => (0, 0x20),
        "sll" | "sllw" => (1, 0),
        "slt" => (2, 0),
        "sltu" => (3, 0),
        "xor" => (4, 0),
        "srl" | "srlw" => (5, 0),
        "sra" | "sraw" => (5, 0x20),
        "or" => (6, 0),
        "and" => (7, 0),
        _ => return None,
    };
    Some((opcode, f3, f7))
}

pub(crate) fn load(name: &str) -> Option<u32> {
    Some(match name {
        "lb" => 0,
        "lh" => 1,
        "lw" => 2,
        "ld" => 3,
        "lbu" => 4,
        "lhu" => 5,
        "lwu" => 6,
        _ => return None,
    })
}

pub(crate) fn store(name: &str) -> Option<u32> {
    Some(match name {
        "sb" => 0,
        "sh" => 1,
        "sw" => 2,
        "sd" => 3,
        _ => return None,
    })
}

pub(crate) const BRANCH_RELOCATION: u32 = 16;

pub(crate) const JAL_RELOCATION: u32 = 17;
