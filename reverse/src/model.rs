// SPDX-License-Identifier: MPL-2.0
//! Typed decoding output. Encoded widths and semantic operand widths are distinct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opcode {
    Mov,
    Add,
    Or,
    Adc,
    Sbb,
    And,
    Sub,
    Xor,
    Cmp,
    Imul,
    Push,
    Pop,
    Jmp,
    Jcc,
    Loop,
    Call,
    Ret,
    Nop,
    Syscall,
    Int3,
    Lea,
    Test,
    Not,
    Neg,
    Div,
    Idiv,
    Shl,
    Shr,
    Sar,
    Cwd,
    Cdq,
    Cqo,
    Movzx,
    Movsx,
    Movsxd,
    Setcc,
    Cmovcc,
    Movss,
    Movsd,
    Addss,
    Addsd,
    Subss,
    Subsd,
    Mulss,
    Mulsd,
    Divss,
    Divsd,
}

impl Opcode {
    pub fn mnemonic(self) -> &'static str {
        match self {
            Self::Mov => "mov",
            Self::Add => "add",
            Self::Or => "or",
            Self::Adc => "adc",
            Self::Sbb => "sbb",
            Self::And => "and",
            Self::Sub => "sub",
            Self::Xor => "xor",
            Self::Cmp => "cmp",
            Self::Imul => "imul",
            Self::Push => "push",
            Self::Pop => "pop",
            Self::Jmp => "jmp",
            Self::Jcc => "jcc",
            Self::Loop => "loop",
            Self::Call => "call",
            Self::Ret => "ret",
            Self::Nop => "nop",
            Self::Syscall => "syscall",
            Self::Int3 => "int3",
            Self::Lea => "lea",
            Self::Test => "test",
            Self::Not => "not",
            Self::Neg => "neg",
            Self::Div => "div",
            Self::Idiv => "idiv",
            Self::Shl => "shl",
            Self::Shr => "shr",
            Self::Sar => "sar",
            Self::Cwd => "cwd",
            Self::Cdq => "cdq",
            Self::Cqo => "cqo",
            Self::Movzx => "movzx",
            Self::Movsx => "movsx",
            Self::Movsxd => "movsxd",
            Self::Setcc => "setcc",
            Self::Cmovcc => "cmovcc",
            Self::Movss => "movss",
            Self::Movsd => "movsd",
            Self::Addss => "addss",
            Self::Addsd => "addsd",
            Self::Subss => "subss",
            Self::Subsd => "subsd",
            Self::Mulss => "mulss",
            Self::Mulsd => "mulsd",
            Self::Divss => "divss",
            Self::Divsd => "divsd",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Condition {
    O,
    No,
    B,
    Ae,
    E,
    Ne,
    Be,
    A,
    S,
    Ns,
    P,
    Np,
    L,
    Ge,
    Le,
    G,
}

impl Condition {
    pub fn suffix(self) -> &'static str {
        ["o", "no", "b", "ae", "e", "ne", "be", "a", "s", "ns", "p", "np", "l", "ge", "le", "g"]
            [self as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterClass {
    General,
    Xmm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Register {
    pub class: RegisterClass,
    pub index: u8,
    pub width: u16,
    /// AH/CH/DH/BH use indices 0..3 and are impossible with an effective REX.
    pub high8: bool,
}

impl Register {
    pub fn name(self) -> String {
        if self.class == RegisterClass::Xmm {
            return format!("xmm{}", self.index);
        }
        if self.high8 {
            return ["ah", "ch", "dh", "bh"]
                .get(self.index as usize)
                .unwrap_or(&"invalid-register")
                .to_string();
        }
        let names = match self.width {
            8 => ["al", "cl", "dl", "bl", "spl", "bpl", "sil", "dil"],
            16 => ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"],
            32 => ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"],
            _ => ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"],
        };
        if self.index < 8 {
            names[self.index as usize].into()
        } else {
            format!(
                "r{}{}",
                self.index,
                match self.width {
                    8 => "b",
                    16 => "w",
                    32 => "d",
                    _ => "",
                }
            )
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryOperand {
    pub width: u16,
    pub base: Option<Register>,
    pub index: Option<Register>,
    pub scale: u8,
    pub displacement: i64,
    pub rip_relative: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Immediate {
    /// Raw encoded bits; sign extension is explicit rather than inferred from value.
    pub value: u64,
    pub width: u16,
    pub encoded_width: u8,
    pub sign_extended: bool,
}

impl Immediate {
    pub fn signed_value(self) -> i64 {
        let bits = u32::from(self.encoded_width);
        if bits == 0 || bits >= 64 {
            self.value as i64
        } else {
            ((self.value << (64 - bits)) as i64) >> (64 - bits)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Relative {
    pub displacement: i64,
    pub encoded_width: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    Register(Register),
    Memory(MemoryOperand),
    Immediate(Immediate),
    Relative(Relative),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodingByte {
    pub offset: u8,
    pub value: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodingField {
    pub offset: u8,
    /// Bytes in the encoding, never inferred from the destination width.
    pub width: u8,
    pub bits: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoding {
    pub prefixes: Vec<u8>,
    pub rex: Option<u8>,
    pub opcode_offset: u8,
    pub opcode_len: u8,
    pub modrm: Option<EncodingByte>,
    pub sib: Option<EncodingByte>,
    pub displacement: Option<EncodingField>,
    pub immediate: Option<EncodingField>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    FallThrough,
    Call { relative: Option<i64> },
    Jump { relative: Option<i64> },
    Conditional { relative: i64 },
    Loop { relative: i64 },
    Return,
    Trap,
    Syscall,
}

impl Flow {
    pub fn relative(self) -> Option<i64> {
        match self {
            Self::Call { relative } | Self::Jump { relative } => relative,
            Self::Conditional { relative } | Self::Loop { relative } => Some(relative),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instruction {
    pub offset: usize,
    pub length: u8,
    pub bytes: [u8; 15],
    pub opcode: Opcode,
    pub condition: Option<Condition>,
    pub operands: Vec<Operand>,
    pub encoding: Encoding,
    pub flow: Flow,
}

impl Instruction {
    pub fn original_bytes(&self) -> &[u8] {
        &self.bytes[..self.length as usize]
    }

    pub fn mnemonic(&self) -> String {
        let suffix = self.condition.map(Condition::suffix).unwrap_or("");
        match self.opcode {
            Opcode::Jcc => format!("j{suffix}"),
            Opcode::Setcc => format!("set{suffix}"),
            Opcode::Cmovcc => format!("cmov{suffix}"),
            opcode => opcode.mnemonic().into(),
        }
    }

    /// Address arithmetic is checked; relocation overlays belong to the host.
    pub fn relative_target(&self, base: u64) -> Option<u64> {
        let displacement = self.flow.relative()?;
        let target = i128::from(base)
            + self.offset as i128
            + i128::from(self.length)
            + i128::from(displacement);
        u64::try_from(target).ok()
    }
}
