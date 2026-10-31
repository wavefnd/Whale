// SPDX-License-Identifier: MPL-2.0
use super::{ParseError, ParseErrorKind, SourceLocation};

use crate::{IrLimits, LimitError, Resource};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Kind<'a> {
    Word(&'a str),
    String(String),
    Mark(&'a str),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Token<'a> {
    pub kind: Kind<'a>,
    pub location: SourceLocation,
}

#[cfg(all(test, whale_wave_ir))]
mod wave_tests {
    use super::*;

    #[test]
    fn wave_lexer_matches_reference_on_unicode_escapes_and_generated_tokens() {
        let seeds = [
            "module { // comment\n  target \"🐋\\u{1f600}\\n\"\n}",
            "  α β\u{2003}γ\u{0085}δ -> => \"\" ",
            "\"escaped \\\" \\\\ \\0 \\t \\r\"",
            "a->b a=>b : < > [ ] ( ) { } , =",
        ];
        for seed in seeds {
            assert_eq!(
                lex(seed, IrLimits::default()).unwrap(),
                lex_reference(seed, IrLimits::default()).unwrap()
            );
        }
        for malformed in [
            "\"unterminated",
            "\"\\",
            "\"\\q\"",
            "\"\\u123\"",
            "\"\\u{}\"",
            "\"\\u{1234567}\"",
            "\"\\u{d800}\"",
            "\"\\u{110000}\"",
            "\"\n\"",
        ] {
            let wave = lex(malformed, IrLimits::default()).unwrap_err();
            let reference = lex_reference(malformed, IrLimits::default()).unwrap_err();
            assert_eq!(wave.location, reference.location, "{malformed:?}");
            assert_eq!(wave.to_string(), reference.to_string(), "{malformed:?}");
        }
        let pieces =
            ["a", "b", "=>", "->", "{", "}", "\"a\\u{1F40B}\"", "// note\n", " ", "\n", "\u{2009}"];
        let mut state = 0x4839_0ab3_d541_u64;
        for _ in 0..4000 {
            let mut text = String::new();
            for _ in 0..40 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                text.push_str(pieces[(state as usize) % pieces.len()]);
            }
            let wave = lex(&text, IrLimits::default());
            let reference = lex_reference(&text, IrLimits::default());
            match (wave, reference) {
                (Ok(a), Ok(b)) => assert_eq!(a, b, "{text:?}"),
                (Err(a), Err(b)) => {
                    assert_eq!(a.location, b.location, "{text:?}");
                    assert_eq!(a.to_string(), b.to_string(), "{text:?}");
                },
                (a, b) => panic!("Wave/reference lexer divergence on {text:?}: {a:?} vs {b:?}"),
            }
        }
    }
}

fn error(location: SourceLocation, message: &str) -> ParseError {
    ParseError { location, kind: ParseErrorKind::Syntax(message.into()) }
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

#[cfg(whale_wave_ir)]
pub(super) fn lex(source: &str, limits: IrLimits) -> Result<Vec<Token<'_>>, ParseError> {
    super::wave_lexer::lex(source, limits)
}

#[cfg(not(whale_wave_ir))]
pub(super) fn lex(source: &str, limits: IrLimits) -> Result<Vec<Token<'_>>, ParseError> {
    lex_reference(source, limits)
}

#[cfg_attr(whale_wave_ir, allow(dead_code))]
pub(super) fn lex_reference(source: &str, limits: IrLimits) -> Result<Vec<Token<'_>>, ParseError> {
    let mut pos = SourceLocation { byte_offset: 0, line: 1, column: 1 };
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
                        },
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
        tokens.push(Token { kind, location: start });
    }
    tokens.push(Token { kind: Kind::Mark("<eof>"), location: pos });
    Ok(tokens)
}
