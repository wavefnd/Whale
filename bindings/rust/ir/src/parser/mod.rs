// SPDX-License-Identifier: MPL-2.0
//! Bounded reader for versioned canonical typed text IR.
mod instruction;

mod lexer;

#[cfg(whale_wave_ir)]
mod wave_lexer;

#[cfg(whale_wave_ir)]
mod wave_header;

#[cfg(whale_wave_ir)]
mod wave_text;

#[cfg(whale_wave_ir)]
mod wave_type;

#[cfg(whale_wave_ir)]
mod wave_literal;

#[cfg(whale_wave_ir)]
mod wave_expression;

#[cfg(whale_wave_ir)]
mod wave_module;

#[cfg(whale_wave_ir)]
mod wave_instruction;

mod syntax;

use crate::{IrLimits, LimitError, Module, VerifyError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    pub byte_offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug)]
pub enum ParseErrorKind {
    Syntax(String),
    ResourceLimit(LimitError),
    Verification(Box<VerifyError>),
    Backend(&'static str),
}

#[derive(Debug)]
pub struct ParseError {
    pub location: SourceLocation,
    pub kind: ParseErrorKind,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: ", self.location.line, self.location.column)?;
        match &self.kind {
            ParseErrorKind::Syntax(s) => f.write_str(s),
            ParseErrorKind::ResourceLimit(e) => e.fmt(f),
            ParseErrorKind::Verification(e) => write!(f, "invalid IR: {e:?}"),
            ParseErrorKind::Backend(component) => write!(f, "IR Wave backend error: {component}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Read and verify typed text IR. IDs, ordering, names and exact scalar bits
/// are retained; unknown syntax and unsupported versions are errors.
pub fn parse_module(source: &str) -> Result<Module, ParseError> {
    parse_module_with_limits(source, IrLimits::default())
}

pub fn parse_module_with_limits(source: &str, limits: IrLimits) -> Result<Module, ParseError> {
    let start = SourceLocation { byte_offset: 0, line: 1, column: 1 };
    limits
        .validate()
        .map_err(|e| ParseError { location: start, kind: ParseErrorKind::ResourceLimit(e) })?;
    let tokens = lexer::lex(source, limits)?;
    let mut p = syntax::Parser::new(tokens, limits);
    let m = p.module()?;
    if !p.done() {
        return Err(p.error("unexpected trailing input"));
    }
    crate::verify_module_with_limits(&m, limits).map_err(|e| ParseError {
        location: p.verification_location(&e),
        kind: ParseErrorKind::Verification(Box::new(e)),
    })?;
    Ok(m)
}
