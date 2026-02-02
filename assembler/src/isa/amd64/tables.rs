pub const REGISTERS_64: &[(&str, u8)] = &[
    ("rax", 0), ("rcx", 1), ("rdx", 2), ("rbx", 3),
    ("rsp", 4), ("rbp", 5), ("rsi", 6), ("rdi", 7),
    ("r8", 8),  ("r9", 9),  ("r10", 10), ("r11", 11),
    ("r12", 12), ("r13", 13), ("r14", 14), ("r15", 15),
];

pub const REGISTERS_32: &[(&str, u8)] = &[
    ("eax", 0), ("ecx", 1), ("edx", 2), ("ebx", 3),
    ("esp", 4), ("ebp", 5), ("esi", 6), ("edi", 7),
    ("r8d", 8), ("r9d", 9), ("r10d", 10), ("r11d", 11),
    ("r12d", 12), ("r13d", 13), ("r14d", 14), ("r15d", 15),
];

pub const REGISTERS_16: &[(&str, u8)] = &[
    ("ax", 0), ("cx", 1), ("dx", 2), ("bx", 3),
    ("sp", 4), ("bp", 5), ("si", 6), ("di", 7),
    ("r8w", 8), ("r9w", 9), ("r10w", 10), ("r11w", 11),
    ("r12w", 12), ("r13w", 13), ("r14w", 14), ("r15w", 15),
];

pub const REGISTERS_8: &[(&str, u8)] = &[
    ("al", 0), ("cl", 1), ("dl", 2), ("bl", 3),
    ("ah", 4), ("ch", 5), ("dh", 6), ("bh", 7),
    ("r8b", 8), ("r9b", 9), ("r10b", 10), ("r11b", 11),
    ("r12b", 12), ("r13b", 13), ("r14b", 14), ("r15b", 15),
];

pub const MNEMONICS: &[&str] = &[
    "mov",
    "add",
    "sub",
    "and",
    "or",
    "xor",
    "cmp",
    "push",
    "pop",
    "jmp",
    "call",
    "ret",
    "nop",
    "syscall",
    "int3",
];
