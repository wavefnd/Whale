pub const REGISTERS_64: &[(&str, u8)] = &[
    ("rax", 0),
    ("rcx", 1),
    ("rdx", 2),
    ("rbx", 3),
    ("rsp", 4),
    ("rbp", 5),
    ("rsi", 6),
    ("rdi", 7),
    ("r8", 8),
    ("r9", 9),
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
    ("r8d", 8),
    ("r9d", 9),
    ("r10d", 10),
    ("r11d", 11),
    ("r12d", 12),
    ("r13d", 13),
    ("r14d", 14),
    ("r15d", 15),
];

pub const REGISTERS_16: &[(&str, u8)] = &[
    ("ax", 0),
    ("cx", 1),
    ("dx", 2),
    ("bx", 3),
    ("sp", 4),
    ("bp", 5),
    ("si", 6),
    ("di", 7),
    ("r8w", 8),
    ("r9w", 9),
    ("r10w", 10),
    ("r11w", 11),
    ("r12w", 12),
    ("r13w", 13),
    ("r14w", 14),
    ("r15w", 15),
];

pub const REGISTERS_8: &[(&str, u8)] = &[
    ("spl", 4),
    ("bpl", 5),
    ("sil", 6),
    ("dil", 7),
    ("al", 0),
    ("cl", 1),
    ("dl", 2),
    ("bl", 3),
    ("ah", 4),
    ("ch", 5),
    ("dh", 6),
    ("bh", 7),
    ("r8b", 8),
    ("r9b", 9),
    ("r10b", 10),
    ("r11b", 11),
    ("r12b", 12),
    ("r13b", 13),
    ("r14b", 14),
    ("r15b", 15),
];

pub const MNEMONICS: &[&str] = &[
    "movss", "movsd", "addss", "addsd", "subss", "subsd", "mulss", "mulsd", "divss", "divsd",
    "mov", "add", "sub", "and", "or", "xor", "cmp", "imul", "push", "pop", "jmp", "je", "loop",
    "call", "ret", "nop", "syscall", "int3", "lea", "test", "not", "neg", "div", "idiv", "shl",
    "sal", "shr", "sar", "cwd", "cdq", "cqo", "jo", "jno", "jb", "jc", "jnae", "jae", "jnb", "jnc",
    "jz", "jne", "jnz", "jbe", "jna", "ja", "jnbe", "js", "jns", "jp", "jpe", "jnp", "jpo", "jl",
    "jnge", "jge", "jnl", "jle", "jng", "jg", "jnle", "movzx", "movsx", "movsxd", "adc", "sbb",
    "seto", "setno", "setb", "setc", "setnae", "setae", "setnb", "setnc", "sete", "setz", "setne",
    "setnz", "setbe", "setna", "seta", "setnbe", "sets", "setns", "setp", "setpe", "setnp",
    "setpo", "setl", "setnge", "setge", "setnl", "setle", "setng", "setg", "setnle", "cmovo",
    "cmovno", "cmovb", "cmovc", "cmovnae", "cmovae", "cmovnb", "cmovnc", "cmove", "cmovz",
    "cmovne", "cmovnz", "cmovbe", "cmovna", "cmova", "cmovnbe", "cmovs", "cmovns", "cmovp",
    "cmovpe", "cmovnp", "cmovpo", "cmovl", "cmovnge", "cmovge", "cmovnl", "cmovle", "cmovng",
    "cmovg", "cmovnle",
];

/// AMD64 integer condition codes and standard aliases.
pub const CONDITION_CODES: &[(&str, u8)] = &[
    ("jo", 0),
    ("jno", 1),
    ("jb", 2),
    ("jc", 2),
    ("jnae", 2),
    ("jae", 3),
    ("jnb", 3),
    ("jnc", 3),
    ("je", 4),
    ("jz", 4),
    ("jne", 5),
    ("jnz", 5),
    ("jbe", 6),
    ("jna", 6),
    ("ja", 7),
    ("jnbe", 7),
    ("js", 8),
    ("jns", 9),
    ("jp", 10),
    ("jpe", 10),
    ("jnp", 11),
    ("jpo", 11),
    ("jl", 12),
    ("jnge", 12),
    ("jge", 13),
    ("jnl", 13),
    ("jle", 14),
    ("jng", 14),
    ("jg", 15),
    ("jnle", 15),
];

#[cfg(any(not(whale_wave_amd64), test))]
pub(crate) fn condition_code_reference(mnemonic: &str) -> Option<u8> {
    CONDITION_CODES.iter().find(|(name, _)| *name == mnemonic).map(|(_, code)| *code)
}

/// Legacy scalar SSE register class; no packed/vector instruction support implied.
pub const REGISTERS_XMM: &[(&str, u8)] = &[
    ("xmm0", 0),
    ("xmm1", 1),
    ("xmm2", 2),
    ("xmm3", 3),
    ("xmm4", 4),
    ("xmm5", 5),
    ("xmm6", 6),
    ("xmm7", 7),
    ("xmm8", 8),
    ("xmm9", 9),
    ("xmm10", 10),
    ("xmm11", 11),
    ("xmm12", 12),
    ("xmm13", 13),
    ("xmm14", 14),
    ("xmm15", 15),
];

#[cfg(any(not(whale_wave_amd64), test))]
pub(crate) fn xmm_register_code_reference(name: &str) -> Option<u8> {
    REGISTERS_XMM.iter().find(|(register, _)| *register == name).map(|(_, code)| *code)
}

/// AMD64 condition code, preserving the exact case-sensitive public contract.
pub fn condition_code(mnemonic: &str) -> Option<u8> {
    #[cfg(whale_wave_amd64)]
    {
        native::condition(mnemonic)
    }
    #[cfg(not(whale_wave_amd64))]
    {
        condition_code_reference(mnemonic)
    }
}

/// Legacy scalar SSE register code.
pub fn xmm_register_code(name: &str) -> Option<u8> {
    #[cfg(whale_wave_amd64)]
    {
        native::xmm(name)
    }
    #[cfg(not(whale_wave_amd64))]
    {
        xmm_register_code_reference(name)
    }
}

#[cfg(whale_wave_amd64)]
mod native {
    use crate::ast_model::Name;

    #[repr(C)]
    #[derive(Default)]
    struct Register {
        kind: u64,
        code: u64,
        width: u64,
        high8: u64,
        rex8: u64,
    }

    unsafe extern "C" {
        fn whale_amd64_register(name: *const Name, fold: u64, output: *mut Register) -> i32;

        fn whale_amd64_condition(name: *const Name, output: *mut u64) -> i32;
    }

    pub(super) fn xmm(text: &str) -> Option<u8> {
        let name = Name { bytes: text.as_ptr(), count: text.len() as u64 };
        let mut output = Register::default();
        match unsafe { whale_amd64_register(&name, 0, &mut output) } {
            0 if output.kind == 1
                && output.code < 16
                && output.width == 128
                && output.high8 == 0
                && output.rex8 == 0 =>
            {
                Some(output.code as u8)
            },
            0 if output.kind == 0
                && output.code < 16
                && matches!(output.width, 8 | 16 | 32 | 64)
                && output.high8 <= 1
                && output.rex8 <= 1
                && (output.high8 == 0
                    || (output.width == 8
                        && (4..8).contains(&output.code)
                        && output.rex8 == 0))
                && (output.rex8 == 0
                    || (output.width == 8
                        && (4..8).contains(&output.code)
                        && output.high8 == 0)) =>
            {
                None
            },
            1 => None,
            _ => panic!("AMD64 register classification transport"),
        }
    }

    pub(super) fn condition(text: &str) -> Option<u8> {
        let name = Name { bytes: text.as_ptr(), count: text.len() as u64 };
        let mut output = 0;
        match unsafe { whale_amd64_condition(&name, &mut output) } {
            0 if output < 16 => Some(output as u8),
            1 => None,
            _ => panic!("AMD64 condition classification transport"),
        }
    }
}

#[cfg(all(test, whale_wave_amd64))]
mod native_tests {
    use super::*;

    #[test]
    fn public_native_classifiers_match_independent_reference_tables() {
        for (name, _) in REGISTERS_64
            .iter()
            .chain(REGISTERS_32)
            .chain(REGISTERS_16)
            .chain(REGISTERS_8)
            .chain(REGISTERS_XMM)
        {
            assert_eq!(xmm_register_code(name), xmm_register_code_reference(name));
            assert_eq!(xmm_register_code(&name.to_ascii_uppercase()), None);
        }
        for (name, expected) in CONDITION_CODES {
            assert_eq!(condition_code(name), Some(*expected));
            assert_eq!(condition_code(name), condition_code_reference(name));
            assert_eq!(condition_code(&name.to_ascii_uppercase()), None);
        }
        for name in ["", "xmm16", "xmm01", "r16", "j", "jn", "cmove", "seto", "라벨"] {
            assert_eq!(condition_code(name), condition_code_reference(name));
            assert_eq!(xmm_register_code(name), xmm_register_code_reference(name));
        }
    }
}
