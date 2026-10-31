use crate::error::{AsmError, SourceSpan};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    Number(i128),
    StringLiteral(Vec<u8>),
    Comma,
    Colon,
    LBracket,
    RBracket,
    Plus,
    Minus,
    Multiply,
    Divide,
    Newline,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    /// UTF-8 byte offset into the original source.
    pub position: usize,
    pub end_position: usize,
    pub line: usize,
    /// One-based Unicode-scalar column; tabs count as one column.
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl Token {
    pub fn span(&self) -> SourceSpan {
        SourceSpan {
            start: self.position,
            end: self.end_position,
            line: self.line,
            column: self.column,
        }
    }
}

#[cfg(any(test, not(whale_wave_assembler)))]
#[path = "unicode_reference.rs"]
mod unicode;

#[cfg(any(test, not(whale_wave_assembler)))]
#[path = "tokens_reference.rs"]
mod reference;

#[cfg(whale_wave_assembler)]
#[path = "tokens_wave.rs"]
mod wave;

pub fn tokenize(source: &str) -> Result<Vec<Token>, AsmError> {
    #[cfg(whale_wave_assembler)]
    {
        wave::tokenize(source)
    }
    #[cfg(not(whale_wave_assembler))]
    {
        reference::tokenize(source)
    }
}

#[cfg(all(test, whale_wave_assembler))]
mod tests {
    use super::*;

    fn compare(source: &str) {
        let expected = reference::tokenize(source);
        let actual = tokenize(source);
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => {
                assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{source:?}");
            },
            (Err(actual), Err(expected)) => {
                assert_eq!(actual.to_string(), expected.to_string(), "{source:?}");
            },
            (actual, expected) => panic!("{source:?}: actual={actual:?}, expected={expected:?}"),
        }
    }

    #[test]
    fn native_utf8_tokens_and_exact_malformed_diagnostics_match_reference() {
        for source in [
            "",
            "nop\r\nret",
            "étiquette: db \"한\\xFF\"",
            "0x",
            "0b",
            "18446744073709551615",
            "18446744073709551616",
            "0xfffffffffffffffff",
            "\"\\",
            "\"\\x",
            "\"\\xA",
            "\"\\x한\"",
            "\"\\q\"",
            "; 😀\r@",
            "/",
            "\"a\r\nb\"\r\nnop",
            "\u{1e6c0}: nop",
        ] {
            compare(source);
        }
        let alphabet: Vec<_> =
            "abcXYZ_.,:[]+-*; \t\r\n0123456789xAbB\"\\né한٣²😀\0\u{2003}".chars().collect();
        let mut state = 0x0057_4841_4c45_4153_u64;
        for _ in 0..10_000 {
            let mut source = String::new();
            for _ in 0..(state % 100) {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                source.push(alphabet[state as usize % alphabet.len()]);
            }
            compare(&source);
            state = state.wrapping_add(1);
        }
    }

    #[test]
    fn long_streams_do_not_accumulate_native_stack_or_change_coordinates() {
        compare(&"nop\r\n".repeat(30_000));
        compare(&format!("db \"{}\"", "한\\xFF".repeat(10_000)));
        let source = ";".repeat(8 * 1024 * 1024 + 1);
        compare(&source);
    }
}
