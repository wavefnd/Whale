#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocKind {
    /// A checked ELF relocation belonging to one explicit machine, never AMD64 fallback.
    TargetSpecific {
        machine: target_spec::Machine,
        code: u32,
    },
    Absolute64,
    Absolute32,
    /// Sign-extended 32-bit absolute value (R_X86_64_32S), distinct from unsigned 32.
    SignedAbsolute32,
    Relative32,
    Relative8,
    GOTPCREL,
    PLT32,
}

impl RelocKind {
    pub(crate) fn encoding(
        self,
        target: target_spec::ObjectTarget,
    ) -> Result<(u32, usize), String> {
        use target_spec::Machine::*;
        let kind = match (target.machine, self) {
            (X86_64, Self::Absolute64) => (1, 8),
            (X86_64, Self::Absolute32) => (10, 4),
            (X86_64, Self::SignedAbsolute32) => (11, 4),
            (X86_64, Self::Relative32) => (2, 4),
            (X86_64, Self::Relative8) => (15, 1),
            (X86_64, Self::GOTPCREL) => (9, 4),
            (X86_64, Self::PLT32) => (4, 4),
            (AArch64, Self::Absolute64) => (257, 8),
            (AArch64, Self::Absolute32) => (258, 4),
            (RiscV64 | LoongArch64, Self::Absolute64) => (2, 8),
            (RiscV64 | LoongArch64, Self::Absolute32) => (1, 4),
            (
                AArch64,
                Self::TargetSpecific { machine: AArch64, code: code @ (274 | 280 | 282 | 283) },
            )
            | (RiscV64, Self::TargetSpecific { machine: RiscV64, code: code @ (16 | 17) })
            | (LoongArch64, Self::TargetSpecific { machine: LoongArch64, code: code @ 64..=66 }) => {
                (code, 4)
            },
            _ => return Err(format!("unsupported relocation {self:?} for {:?}", target.machine)),
        };
        Ok(kind)
    }
}

#[derive(Debug, Clone)]
pub struct ObjectRelocation {
    pub section_index: usize,
    pub offset: usize,
    pub symbol: String,
    pub addend: i64,
    pub kind: RelocKind,
}
