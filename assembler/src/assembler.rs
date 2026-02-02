use crate::tokens::tokenize;
use crate::traits::ISA;
use crate::error::{AsmError};
use crate::ast::AST;

pub struct AssemblerOutput {
    pub sections: Vec<AsmSection>,
    pub symbols: Vec<AsmSymbol>,
}

pub struct AsmSection {
    pub name: String,
    pub data: Vec<u8>,
    pub relocs: Vec<Relocation>,
}

pub struct AsmSymbol {
    pub name: String,
    pub section_index: Option<usize>,
    pub offset: usize,
    pub is_global: bool,
}

#[derive(Debug, Clone)]
pub struct Relocation {
    pub offset: usize,
    pub symbol: String,
    pub kind: RelocKind,
    pub addend: i64,
}

#[derive(Debug, Clone)]
pub enum RelocKind {
    Absolute64,
    Relative32,
}

pub fn assemble(source: &str, isa: &impl ISA) -> Result<AssemblerOutput, AsmError> {
    let tokens = tokenize(source)
        .map_err(|e| AsmError::LexerError(e.to_string()))?;

    let ast: AST = isa.parse(&tokens)?;

    let encoded = isa.encode(&ast)?;

    Ok(encoded)
}