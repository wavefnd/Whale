use crate::error::{AsmError, SourceSpan};

use diagnostic::{Context, Diagnostic, SourceLocation, ToDiagnostic};

fn source(name: &str, span: SourceSpan) -> Context {
    Context::Source(SourceLocation {
        source: name.into(),
        line: span.line as u64,
        column: span.column as u64,
        start_byte: Some(span.start as u64),
        end_byte: Some(span.end as u64),
        ..Default::default()
    })
}

impl ToDiagnostic for AsmError {
    fn diagnostic(&self) -> Diagnostic {
        let (phase, code) = match self {
            Self::Backend(_) => ("backend", "assembler.backend"),
            Self::LexerError(_) => ("lex", "assembler.lex"),
            Self::ParserError(_) => ("parse", "assembler.parse"),
            Self::EncodeError(_) => ("encode", "assembler.encode"),
            Self::SymbolError(_) => ("symbols", "assembler.symbol"),
            Self::UnexpectedToken(_) => ("parse", "assembler.unexpected-token"),
            Self::Diagnostic { source_name, span, related, error } => {
                let mut d = error.diagnostic().with_context(source(source_name, *span));
                if let Some(previous) = related {
                    d.contexts.push(source(source_name, *previous));
                }
                return d;
            },
        };
        Diagnostic::new("assembler", phase, code, self.to_string())
    }
}
