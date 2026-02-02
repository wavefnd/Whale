pub const REGISTERS_64: &[(&str, u8)] = &[
    ("rax", 0),
    ("rcx", 1),
    ("rdx", 2),
    ("rbx", 3),
    ("rsp", 4),
    ("rbp", 5),
    ("rsi", 6),
    ("rdi", 7),

    ("r8",  8),
    ("r9",  9),
    ("r10", 10),
    ("r11", 11),
    ("r12", 12),
    ("r13", 13),
    ("r14", 14),
    ("r15", 15),
];

pub const REGISTERS_32: &[(&str, u8)] = &[
    ("eax", 0),
    ("ecx", 1),
    ("edx", 2),
    ("ebx", 3),
    ("esp", 4),
    ("ebp", 5),
    ("esi", 6),
    ("edi", 7),
];

pub const MNEMONICS: &[&str] = &[
    "mov",
    "add",
    "sub",
    "jmp",
    "call",
    "ret",
    "nop",
];
