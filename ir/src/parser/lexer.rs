// SPDX-License-Identifier: MPL-2.0
use super::{ParseError, ParseErrorKind, SourceLocation};
use crate::{IrLimits, LimitError, Resource};
#[derive(Debug)]
pub(super) enum Kind<'a> {
    Word(&'a str),
    String(String),
    Mark(&'a str),
}
#[derive(Debug)]
pub(super) struct Token<'a> {
    pub kind: Kind<'a>,
    pub location: SourceLocation,
}
fn error(location: SourceLocation, message: &str) -> ParseError {
    ParseError {
        location,
        kind: ParseErrorKind::Syntax(message.into()),
    }
}
fn advance(location: &mut SourceLocation, c: char) {
    location.byte_offset += c.len_utf8();
    if c == '\n' {
        location.line += 1;
        location.column = 1;
    } else {
        location.column += 1;
    }
}
pub(super) fn lex(source: &str, limits: IrLimits) -> Result<Vec<Token<'_>>, ParseError> {
    let mut pos = SourceLocation {
        byte_offset: 0,
        line: 1,
        column: 1,
    };
    if source.len() > limits.max_input_bytes {
        return Err(ParseError {
            location: pos,
            kind: ParseErrorKind::ResourceLimit(LimitError {
                resource: Resource::InputBytes,
                limit: limits.max_input_bytes,
            }),
        });
    }
    let mut tokens = Vec::new();
    while pos.byte_offset < source.len() {
        let rest = &source[pos.byte_offset..];
        let c = rest.chars().next().unwrap();
        if c.is_whitespace() {
            advance(&mut pos, c);
            continue;
        }
        if rest.starts_with("//") {
            for c in rest.chars().take_while(|c| *c != '\n') {
                advance(&mut pos, c);
            }
            continue;
        }
        let start = pos;
        if tokens.len() >= limits.max_tokens {
            return Err(ParseError {
                location: pos,
                kind: ParseErrorKind::ResourceLimit(LimitError {
                    resource: Resource::Tokens,
                    limit: limits.max_tokens,
                }),
            });
        }
        let kind = if c == '"' {
            advance(&mut pos, c);
            let mut value = String::new();
            loop {
                let c = source[pos.byte_offset..]
                    .chars()
                    .next()
                    .ok_or_else(|| error(start, "unterminated string"))?;
                advance(&mut pos, c);
                if c == '"' {
                    break;
                }
                if c == '\\' {
                    let e = source[pos.byte_offset..]
                        .chars()
                        .next()
                        .ok_or_else(|| error(pos, "unterminated escape"))?;
                    advance(&mut pos, e);
                    value.push(match e {
                        '"' => '"',
                        '\\' => '\\',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        '0' => '\0',
                        'u' => {
                            if !source[pos.byte_offset..].starts_with('{') {
                                return Err(error(pos, "expected '{' in Unicode escape"));
                            }
                            advance(&mut pos, '{');
                            let digits_start = pos.byte_offset;
                            while let Some(ch) = source[pos.byte_offset..].chars().next() {
                                if !ch.is_ascii_hexdigit() {
                                    break;
                                }
                                advance(&mut pos, ch);
                            }
                            let digits = &source[digits_start..pos.byte_offset];
                            if digits.is_empty()
                                || digits.len() > 6
                                || !source[pos.byte_offset..].starts_with('}')
                            {
                                return Err(error(pos, "invalid Unicode escape"));
                            }
                            advance(&mut pos, '}');
                            u32::from_str_radix(digits, 16)
                                .ok()
                                .and_then(char::from_u32)
                                .ok_or_else(|| error(pos, "invalid Unicode scalar"))?
                        }
                        _ => return Err(error(pos, "unknown string escape")),
                    });
                } else if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                    return Err(error(start, "string control characters must be escaped"));
                } else {
                    value.push(c);
                }
            }
            Kind::String(value)
        } else if rest.starts_with("->") || rest.starts_with("=>") {
            advance(&mut pos, c);
            advance(&mut pos, '>');
            Kind::Mark(&source[start.byte_offset..pos.byte_offset])
        } else if "{}()[]<>,:= ".contains(c) {
            advance(&mut pos, c);
            Kind::Mark(&source[start.byte_offset..pos.byte_offset])
        } else {
            for ch in rest.chars() {
                if ch.is_whitespace() || "{}()[]<>,:=\"".contains(ch) {
                    break;
                }
                advance(&mut pos, ch);
            }
            Kind::Word(&source[start.byte_offset..pos.byte_offset])
        };
        tokens.push(Token {
            kind,
            location: start,
        });
    }
    tokens.push(Token {
        kind: Kind::Mark("<eof>"),
        location: pos,
    });
    Ok(tokens)
}
