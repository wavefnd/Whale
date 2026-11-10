// SPDX-License-Identifier: MPL-2.0
//! LoongArch64 ABI aliases, opcode/form classification and ELF relocation IDs.
pub(crate) const REGISTER_ALIASES: [&str; 32] = [
    "zero", "ra", "tp", "sp", "a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "t0", "t1", "t2",
    "t3", "t4", "t5", "t6", "t7", "t8", "u0", "fp", "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "s8",
];

pub(crate) fn three_register(name: &str) -> Option<u32> {
    Some(match name {
        "add.w" => 0x00100000,
        "add.d" => 0x00108000,
        "sub.w" => 0x00110000,
        "sub.d" => 0x00118000,
        "slt" => 0x00120000,
        "sltu" => 0x00128000,
        "nor" => 0x00140000,
        "and" => 0x00148000,
        "or" => 0x00150000,
        "xor" => 0x00158000,
        "sll.w" => 0x00170000,
        "srl.w" => 0x00178000,
        "sra.w" => 0x00180000,
        "sll.d" => 0x00188000,
        "srl.d" => 0x00190000,
        "sra.d" => 0x00198000,
        _ => return None,
    })
}

pub(crate) fn immediate(name: &str) -> Option<(u32, bool)> {
    Some(match name {
        "slti" => (0x02000000, false),
        "sltui" => (0x02400000, false),
        "addi.w" => (0x02800000, false),
        "addi.d" => (0x02c00000, false),
        "lu52i.d" => (0x03000000, false),
        "andi" => (0x03400000, true),
        "ori" => (0x03800000, true),
        "xori" => (0x03c00000, true),
        _ => return None,
    })
}

pub(crate) fn shift(name: &str) -> Option<(u32, u32)> {
    Some(match name {
        "slli.w" => (0x00408000, 5),
        "srli.w" => (0x00448000, 5),
        "srai.w" => (0x00488000, 5),
        "slli.d" => (0x00410000, 6),
        "srli.d" => (0x00450000, 6),
        "srai.d" => (0x00490000, 6),
        _ => return None,
    })
}

pub(crate) fn memory(name: &str) -> Option<u32> {
    Some(match name {
        "ld.b" => 0x28000000,
        "ld.h" => 0x28400000,
        "ld.w" => 0x28800000,
        "ld.d" => 0x28c00000,
        "st.b" => 0x29000000,
        "st.h" => 0x29400000,
        "st.w" => 0x29800000,
        "st.d" => 0x29c00000,
        "ld.bu" => 0x2a000000,
        "ld.hu" => 0x2a400000,
        "ld.wu" => 0x2a800000,
        _ => return None,
    })
}

pub(crate) fn conditional(name: &str) -> Option<u32> {
    Some(match name {
        "beq" => 0x58000000,
        "bne" => 0x5c000000,
        "blt" => 0x60000000,
        "bge" => 0x64000000,
        "bltu" => 0x68000000,
        "bgeu" => 0x6c000000,
        _ => return None,
    })
}

pub(crate) const B16_RELOCATION: u32 = 64;

pub(crate) const B21_RELOCATION: u32 = 65;

pub(crate) const B26_RELOCATION: u32 = 66;
