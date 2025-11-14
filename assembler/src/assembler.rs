use crate::tokens::tokenize;
use crate::traits::ISA;
use crate::error::{AsmError};
use crate::ast::AST;

pub struct AssemblerOutput {
    pub bytes: Vec<u8>,
    pub symbols: Vec<(String, usize)>,
    pub relocations: Vec<Relocation>,
}

#[derive(Debug, Clone)]
pub struct Relocation {
    pub offset: usize,
    pub symbol: String,
    pub kind: RelocKind,
}

#[derive(Debug, Clone)]
pub enum RelocKind {
    Absolute,
    Relative,
}

pub fn assemble(source: &str, isa: &impl ISA) -> Result<AssemblerOutput, AssemError> {
    let tokens = toknize(source)
        .map_err(|e| AsmError::LexerError(e.to_string()))?;

    let ast: AST = isa.parse(&tokens)?;

    let encoded = isa.encode(&ast)?;

    Ok(encoded)
}