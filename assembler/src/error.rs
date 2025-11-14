use std::fmt;

#[derive(Debug, Clone)]
pub enum AsmError {
    LexerError(String),
    ParserError(String),
    EncodeError(String),
    SymbolError(String),
    UnexpectedToken(String),
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsmError::LexerError(s) => write!(f, "Lexer error: {}", s),
            AsmError::ParserError(s) => write!(f, "Parser error: {}", s),
            AsmError::EncodeError(s) => write!(f, "Encode error: {}", s),
            AsmError::SymbolError(s) => write!(f, "Symbol error: {}", s),
            AsmError::UnexpectedToken(s) => write!(f, "Unexpected token: {}", s),
        }
    }
}

impl std::error::Error for AsmError {}