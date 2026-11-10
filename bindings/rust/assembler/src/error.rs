use diagnostic::EscapedName;

use std::fmt;

/// Half-open UTF-8 byte range; line/column are one-based Unicode-scalar coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub enum AsmError {
    /// A native module/transport failure, distinct from invalid assembly input.
    Backend(&'static str),
    LexerError(String),
    ParserError(String),
    EncodeError(String),
    SymbolError(String),
    UnexpectedToken(String),
    Diagnostic {
        source_name: String,
        span: SourceSpan,
        related: Option<SourceSpan>,
        error: Box<AsmError>,
    },
}

impl AsmError {
    pub fn at(
        self,
        source_name: impl Into<String>,
        span: SourceSpan,
        related: Option<SourceSpan>,
    ) -> Self {
        if matches!(self, Self::Diagnostic { .. }) {
            return self;
        }
        Self::Diagnostic { source_name: source_name.into(), span, related, error: Box::new(self) }
    }

    pub fn with_source_name(self, source_name: impl Into<String>) -> Self {
        match self {
            Self::Diagnostic { span, related, error, .. } => {
                Self::Diagnostic { source_name: source_name.into(), span, related, error }
            },
            other => other,
        }
    }
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsmError::Backend(reason) => write!(f, "Assembler backend failure: {reason}"),
            AsmError::LexerError(s) => write!(f, "Lexer error: {}", s),
            AsmError::ParserError(s) => write!(f, "Parser error: {}", s),
            AsmError::EncodeError(s) => write!(f, "Encode error: {}", s),
            AsmError::SymbolError(s) => write!(f, "Symbol error: {}", s),
            AsmError::UnexpectedToken(s) => write!(f, "Unexpected token: {}", s),
            AsmError::Diagnostic { source_name, span, related, error } => {
                write!(
                    f,
                    "{error} at {}: line {}, column {} (bytes {}..{})",
                    EscapedName(source_name),
                    span.line,
                    span.column,
                    span.start,
                    span.end
                )?;
                if let Some(previous) = related {
                    write!(
                        f,
                        "; previous declaration at {}: line {}, column {} (bytes {}..{})",
                        EscapedName(source_name),
                        previous.line,
                        previous.column,
                        previous.start,
                        previous.end
                    )?;
                }
                Ok(())
            },
        }
    }
}

impl std::error::Error for AsmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Diagnostic { error, .. } => Some(error.as_ref()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names_are_escaped_in_primary_and_related_locations() {
        let span = SourceSpan { start: 0, end: 1, line: 1, column: 2 };
        for control in ['\n', '\r', '\t', '\x1b'] {
            let name = format!("경로/{control}file.s");
            let error = AsmError::ParserError("bad".into()).at(&name, span, Some(span));
            let rendered = error.to_string();
            assert!(!rendered.contains(control), "{rendered:?}");
            assert_eq!(rendered.matches(&control.escape_debug().to_string()).count(), 2);
            assert!(rendered.contains("line 1, column 2 (bytes 0..1)"));
            if let AsmError::Diagnostic { source_name, .. } = error {
                assert_eq!(source_name, name);
            } else {
                panic!("expected located assembler error");
            }
        }
        assert!(AsmError::ParserError("bad".into())
            .at("경로/파일.s", span, None)
            .to_string()
            .contains("경로/파일.s"));
    }
}
