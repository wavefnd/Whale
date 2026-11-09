use crate::assembler::AssemblerOutput;

use crate::ast::AST;

use crate::error::AsmError;

pub trait ISA {
    fn parse(&self, tokens: &[crate::tokens::Token]) -> Result<AST, AsmError>;

    fn encode(&self, ast: &AST) -> Result<AssemblerOutput, AsmError>;

    fn encode_with_limits(
        &self,
        ast: &AST,
        limits: &crate::AssemblyLimits,
    ) -> Result<AssemblerOutput, AsmError> {
        let output = self.encode(ast)?;
        let size = output
            .sections
            .iter()
            .try_fold(0usize, |size, section| size.checked_add(section.data.len()));
        if size.is_none_or(|size| size > limits.max_output_bytes) {
            return Err(AsmError::EncodeError("Assembly output limit exceeded".into()));
        }
        Ok(output)
    }
}
