use crate::ast::AST;
use crate::error::AsmError;
use crate::assembler::AssemblerOutput;

pub trait ISA {
    fn parse(&self, tokens: &[crate::tokens::Token]) -> Result<AST, AsmError>;
    fn encode(&self, ast: &AST) -> Result<AssemblerOutput, AsmError>;
}