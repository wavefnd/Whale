// SPDX-License-Identifier: MPL-2.0
//! Explicit compatibility reference for the Wave assembler lexer.
use super::{Token, TokenKind};

use crate::{AsmError, SourceSpan};

#[derive(Clone, Copy)]
struct Mark {
    position: usize,
    line: usize,
    column: usize,
}

struct Cursor<'a> {
    source: &'a str,
    position: usize,
    line: usize,
    column: usize,
    previous_cr: bool,
}

impl<'a> Cursor<'a> {
    fn mark(&self) -> Mark {
        Mark { position: self.position, line: self.line, column: self.column }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.position..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.position += ch.len_utf8();
        match ch {
            '\r' => {
                self.line += 1;
                self.column = 1;
                self.previous_cr = true;
            },
            '\n' => {
                if !self.previous_cr {
                    self.line += 1;
                }
                self.column = 1;
                self.previous_cr = false;
            },
            _ => {
                self.column += 1;
                self.previous_cr = false;
            },
        }
        Some(ch)
    }

    fn token(&self, kind: TokenKind, start: Mark) -> Token {
        Token {
            kind,
            position: start.position,
            end_position: self.position,
            line: start.line,
            column: start.column,
            end_line: self.line,
            end_column: self.column,
        }
    }

    fn error(&self, msg: &str, start: Mark) -> AsmError {
        let end = if self.position > start.position {
            self.position
        } else {
            self.position + self.peek().map_or(0, char::len_utf8)
        };
        AsmError::LexerError(msg.into()).at(
            "<input>",
            SourceSpan { start: start.position, end, line: start.line, column: start.column },
            None,
        )
    }
}

pub(super) fn tokenize(source: &str) -> Result<Vec<Token>, AsmError> {
    if source.len() > 8 * 1024 * 1024 {
        return Err(AsmError::LexerError("assembly input exceeds 8388608 bytes".into()).at(
            "<input>",
            SourceSpan { start: 0, end: 0, line: 1, column: 1 },
            None,
        ));
    }
    let mut c = Cursor { source, position: 0, line: 1, column: 1, previous_cr: false };
    let mut tokens = Vec::new();
    while let Some(ch) = c.peek() {
        let start = c.mark();
        let kind = match ch {
            ';' => {
                while c.peek().is_some_and(|ch| ch != '\n' && ch != '\r') {
                    c.bump();
                }
                continue;
            },
            ' ' | '\t' => {
                c.bump();
                continue;
            },
            '\n' | '\r' => {
                c.bump();
                if ch == '\r' && c.peek() == Some('\n') {
                    c.bump();
                }
                TokenKind::Newline
            },
            ',' => {
                c.bump();
                TokenKind::Comma
            },
            ':' => {
                c.bump();
                TokenKind::Colon
            },
            '[' => {
                c.bump();
                TokenKind::LBracket
            },
            ']' => {
                c.bump();
                TokenKind::RBracket
            },
            '+' => {
                c.bump();
                TokenKind::Plus
            },
            '-' => {
                c.bump();
                TokenKind::Minus
            },
            '*' => {
                c.bump();
                TokenKind::Multiply
            },
            '"' => {
                c.bump();
                let mut bytes = Vec::new();
                loop {
                    let at = c.mark();
                    let ch =
                        c.bump().ok_or_else(|| c.error("Unterminated string literal", start))?;
                    match ch {
                        '"' => break,
                        '\\' => {
                            let escape = c.bump().ok_or_else(|| c.error("Truncated escape", at))?;
                            let byte = match escape {
                                '"' => b'"',
                                '\\' => b'\\',
                                'n' => b'\n',
                                'r' => b'\r',
                                't' => b'\t',
                                '0' => 0,
                                'x' => {
                                    let mut value = 0;
                                    for _ in 0..2 {
                                        let digit = c.bump().ok_or_else(|| {
                                            c.error(
                                                "Truncated byte escape; expected two hex digits",
                                                at,
                                            )
                                        })?;
                                        let digit = digit
                                            .to_digit(16)
                                            .filter(|_| digit.is_ascii_hexdigit())
                                            .ok_or_else(|| {
                                                c.error(
                                                    "Invalid byte escape; expected two hex digits",
                                                    at,
                                                )
                                            })?;
                                        value = value * 16 + digit as u8;
                                    }
                                    value
                                },
                                _ => return Err(c.error("Unknown string escape", at)),
                            };
                            bytes.push(byte);
                        },
                        _ => {
                            let mut utf8 = [0; 4];
                            bytes.extend_from_slice(ch.encode_utf8(&mut utf8).as_bytes());
                        },
                    }
                }
                TokenKind::StringLiteral(bytes)
            },
            ch if ch.is_ascii_digit() => {
                let rest = &source[c.position..];
                let radix = if rest.starts_with("0x") || rest.starts_with("0X") {
                    16
                } else if rest.starts_with("0b") || rest.starts_with("0B") {
                    2
                } else {
                    10
                };
                if radix != 10 {
                    c.bump();
                    c.bump();
                }
                let digits = c.position;
                while c.peek().is_some_and(|ch| match radix {
                    16 => ch.is_ascii_hexdigit(),
                    2 => matches!(ch, '0' | '1'),
                    _ => ch.is_ascii_digit(),
                }) {
                    c.bump();
                }
                if c.position == digits {
                    return Err(c.error(
                        if radix == 16 { "Invalid hex number" } else { "Invalid binary number" },
                        start,
                    ));
                }
                let number = i128::from_str_radix(&source[digits..c.position], radix)
                    .ok()
                    .filter(|n| *n <= u64::MAX as i128)
                    .ok_or_else(|| c.error("Number exceeds unsigned 64-bit range", start))?;
                TokenKind::Number(number)
            },
            ch if crate::tokens::unicode::alphanumeric(ch) || ch == '_' || ch == '.' => {
                while c.peek().is_some_and(|ch| {
                    crate::tokens::unicode::alphanumeric(ch) || ch == '_' || ch == '.'
                }) {
                    c.bump();
                }
                TokenKind::Identifier(source[start.position..c.position].into())
            },
            _ => return Err(c.error(&format!("Unexpected char '{ch}'"), start)),
        };
        tokens.push(c.token(kind, start));
    }
    Ok(tokens)
}
