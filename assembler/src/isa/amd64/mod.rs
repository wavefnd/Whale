use crate::traits::ISA;
use crate::ast::AST;
use crate::error::AsmError;
use crate::assembler::{AssemblerOutput};

pub mod encoder;
pub mod parser;
pub mod tables;
pub mod encoding;

pub struct AMD64;

impl ISA for AMD64 {
    fn parse(&self, tokens: &[crate::tokens::Token]) -> Result<AST, AsmError> {
        parser::parse(tokens)
    }

    fn encode(&self, ast: &AST) -> Result<AssemblerOutput, AsmError> {
        encoder::encode(ast)
    }
}