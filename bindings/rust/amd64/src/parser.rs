// SPDX-License-Identifier: MPL-2.0
//! AMD64 grammar dispatch. Wave owns the production parser.
#[cfg(any(not(whale_wave_amd64), test))]
#[path = "parser_reference.rs"]
pub(crate) mod reference;

#[cfg(whale_wave_amd64)]
#[path = "parser_wave.rs"]
mod wave;

pub fn parse(
    tokens: &[assembler::tokens::Token],
) -> Result<assembler::ast::AST, assembler::AsmError> {
    #[cfg(whale_wave_amd64)]
    {
        wave::parse(tokens)
    }
    #[cfg(not(whale_wave_amd64))]
    {
        reference::parse(tokens)
    }
}
