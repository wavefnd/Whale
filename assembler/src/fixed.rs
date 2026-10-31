// SPDX-License-Identifier: MPL-2.0
//! Two-pass source/section handling for fixed-width little-endian ISAs.
//! Instruction syntax and bit encodings belong to the implementation crate.
use crate::{
    AsmError, AsmSection, AsmSectionKind, AsmSymbol, AssemblerOutput, AssemblyLimits, RelocKind,
    Relocation, SourceSpan, SymbolKind, SymbolVisibility,
};

use std::collections::BTreeMap;

pub struct Instruction {
    pub mnemonic: String,
    pub operands: Vec<String>,
}

pub trait FixedIsa {
    fn id(&self) -> &'static str;

    fn hash_comments(&self) -> bool {
        true
    }

    #[doc(hidden)]
    fn native_assemble(
        &self,
        _name: &str,
        _source: &str,
        _limits: &AssemblyLimits,
    ) -> Option<Result<AssemblerOutput, AsmError>> {
        None
    }

    fn encode(&self, instruction: &Instruction, context: &mut Context<'_>)
        -> Result<u32, AsmError>;
}

struct Symbol {
    section: Option<usize>,
    offset: u64,
    span: SourceSpan,
}

pub struct Context<'a> {
    namespace: &'static str,
    section: usize,
    offset: u64,
    symbols: &'a BTreeMap<String, Symbol>,
    relocations: &'a mut Vec<Relocation>,
}

#[doc(hidden)]
#[cfg(feature = "rust-reference")]
#[path = "fixed_reference.rs"]
pub mod reference;

#[cfg(whale_wave_assembler)]
#[doc(hidden)]
#[path = "fixed_wave.rs"]
pub mod native;

impl Context<'_> {
    pub fn pc_relative(&mut self, operand: &str, code: u32) -> Result<i64, AsmError> {
        #[cfg(whale_wave_assembler)]
        return native::pc_relative(self, operand, code);
        #[cfg(not(whale_wave_assembler))]
        reference::pc_relative(self, operand, code)
    }

    #[doc(hidden)]
    #[cfg(feature = "rust-reference")]
    pub fn pc_relative_reference(&mut self, operand: &str, code: u32) -> Result<i64, AsmError> {
        reference::pc_relative(self, operand, code)
    }
}

pub fn error(message: impl Into<String>) -> AsmError {
    AsmError::EncodeError(message.into())
}

pub fn count(instruction: &Instruction, expected: usize) -> Result<&[String], AsmError> {
    #[cfg(whale_wave_assembler)]
    {
        native::count(instruction, expected)?;
        Ok(&instruction.operands)
    }
    #[cfg(not(whale_wave_assembler))]
    reference::count(instruction, expected)
}

pub fn integer(text: &str) -> Result<i128, AsmError> {
    #[cfg(whale_wave_assembler)]
    return native::integer(text);
    #[cfg(not(whale_wave_assembler))]
    reference::integer(text)
}

pub fn signed(text: &str, bits: u32) -> Result<u32, AsmError> {
    #[cfg(whale_wave_assembler)]
    return native::field(text, bits, false);
    #[cfg(not(whale_wave_assembler))]
    reference::signed(text, bits)
}

pub fn unsigned(text: &str, bits: u32) -> Result<u32, AsmError> {
    #[cfg(whale_wave_assembler)]
    return native::field(text, bits, true);
    #[cfg(not(whale_wave_assembler))]
    reference::unsigned(text, bits)
}

pub fn displacement(value: i64, bits: u32, alignment: i64) -> Result<u32, AsmError> {
    #[cfg(whale_wave_assembler)]
    return native::displacement(value, bits, alignment);
    #[cfg(not(whale_wave_assembler))]
    reference::displacement(value, bits, alignment)
}

pub fn operands(text: &str) -> Result<Vec<String>, AsmError> {
    #[cfg(whale_wave_assembler)]
    return native::operands(text);
    #[cfg(not(whale_wave_assembler))]
    reference::operands(text)
}

pub fn assemble(
    isa: &impl FixedIsa,
    name: &str,
    source: &str,
    limits: &AssemblyLimits,
) -> Result<AssemblerOutput, AsmError> {
    if let Some(output) = isa.native_assemble(name, source, limits) {
        return output;
    }
    #[cfg(not(whale_wave_assembler))]
    return reference::assemble(isa, name, source, limits);
    #[cfg(whale_wave_assembler)]
    Err(AsmError::Backend("selected ISA has no Wave source driver"))
}

#[cfg(all(test, whale_wave_assembler))]
mod driver_selection_tests {
    use super::*;

    struct Missing;

    impl FixedIsa for Missing {
        fn id(&self) -> &'static str {
            "missing"
        }

        fn encode(&self, _: &Instruction, _: &mut Context<'_>) -> Result<u32, AsmError> {
            panic!("a missing native driver cannot invoke Rust encoding")
        }
    }

    #[test]
    fn missing_wave_driver_never_uses_the_reference_parser() {
        assert!(matches!(
            assemble(&Missing, "input.s", "nop", &AssemblyLimits::default()),
            Err(AsmError::Backend("selected ISA has no Wave source driver"))
        ));
    }
}
