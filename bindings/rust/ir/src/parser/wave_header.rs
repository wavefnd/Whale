// SPDX-License-Identifier: MPL-2.0
//! Borrowed bridge for the Wave-owned versioned IR header parser.

use super::{lexer::Kind, lexer::Token, wave_text::RawToken, ParseError, ParseErrorKind};

use crate::{DataLayout, Endian, IR_FORMAT_VERSION, SEMANTICS_VERSION};

#[repr(C)]
#[derive(Default)]
struct RawResult {
    status: u64,
    at: u64,
    expected: u64,
    version: u64,
    ptr_bits: u64,
    endian: u64,
    target_index: u64,
    next: u64,
}

unsafe extern "C" {
    fn whale_ir_header(
        tokens: *const RawToken,
        count: u64,
        max_format: u64,
        semantics: u64,
        result: *mut RawResult,
    ) -> i32;
}

const EXPECTED: [&str; 18] = [
    "module",
    "{",
    "format_version",
    "",
    "semantics_version",
    "",
    "target",
    "",
    "datalayout",
    "{",
    "ptr",
    "=",
    "",
    ",",
    "endian",
    "=",
    "",
    "}",
];

pub(super) fn parse(tokens: &[Token<'_>]) -> Result<(String, DataLayout, u32, usize), ParseError> {
    let raw: Vec<_> = tokens.iter().take(18).map(RawToken::from).collect();
    let mut result = RawResult::default();
    let status = unsafe {
        whale_ir_header(
            raw.as_ptr(),
            raw.len() as u64,
            IR_FORMAT_VERSION.into(),
            SEMANTICS_VERSION.into(),
            &mut result,
        )
    };
    if status != 0 || result.status != 0 {
        let index = usize::try_from(result.at).unwrap_or(usize::MAX).min(tokens.len() - 1);
        if !matches!(result.status, 1..=7 | 9)
            || (result.status == 1
                && EXPECTED.get(result.expected as usize).is_none_or(|word| word.is_empty()))
        {
            return Err(ParseError {
                location: tokens[index].location,
                kind: ParseErrorKind::Backend("header transport"),
            });
        }
        let found = match &tokens[index].kind {
            Kind::Word(s) | Kind::Mark(s) => *s,
            Kind::String(_) => "<string>",
        };
        let message = match result.status {
            1 => {
                let expected = EXPECTED.get(result.expected as usize).copied().unwrap_or("?");
                format!("expected '{expected}', found '{found}'")
            },
            2 => "expected unsigned decimal integer".into(),
            3 => "integer out of range".into(),
            4 => "unsupported IR format version".into(),
            5 => "unsupported semantics version".into(),
            6 => "expected quoted string".into(),
            7 => "unknown byte order".into(),
            9 => "expected word".into(),
            _ => "invalid Wave IR header result".into(),
        };
        return Err(ParseError {
            location: tokens[index].location,
            kind: ParseErrorKind::Syntax(message),
        });
    }
    let target_index = usize::try_from(result.target_index).map_err(|_| ParseError {
        location: tokens[0].location,
        kind: ParseErrorKind::Backend("header transport"),
    })?;
    let next = usize::try_from(result.next).map_err(|_| ParseError {
        location: tokens[0].location,
        kind: ParseErrorKind::Backend("header transport"),
    })?;
    let Some(Token { kind: Kind::String(target), .. }) = tokens.get(target_index) else {
        return Err(ParseError {
            location: tokens[0].location,
            kind: ParseErrorKind::Backend("header transport"),
        });
    };
    if next != 18
        || target_index != 7
        || !(3..=u64::from(IR_FORMAT_VERSION)).contains(&result.version)
        || result.ptr_bits > u32::MAX.into()
        || result.endian > 1
    {
        return Err(ParseError {
            location: tokens[0].location,
            kind: ParseErrorKind::Backend("header transport"),
        });
    }
    Ok((
        target.clone(),
        DataLayout {
            ptr_bits: result.ptr_bits as u32,
            endian: if result.endian == 0 { Endian::Little } else { Endian::Big },
        },
        result.version as u32,
        next,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::IrLimits;

    fn reference(tokens: Vec<Token<'_>>) -> Result<(String, DataLayout, u32, usize), ParseError> {
        let mut p = super::super::syntax::Parser::new(tokens, IrLimits::default());
        p.expect("module")?;
        p.expect("{")?;
        p.expect("format_version")?;
        let version: u32 = p.number()?;
        if !(3..=IR_FORMAT_VERSION).contains(&version) {
            return Err(p.previous_error("unsupported IR format version"));
        }
        p.expect("semantics_version")?;
        let semantics: u32 = p.number()?;
        if semantics != SEMANTICS_VERSION {
            return Err(p.previous_error("unsupported semantics version"));
        }
        p.expect("target")?;
        let target = p.string()?;
        p.expect("datalayout")?;
        p.expect("{")?;
        p.expect("ptr")?;
        p.expect("=")?;
        let ptr_bits = p.number()?;
        p.comma()?;
        p.expect("endian")?;
        p.expect("=")?;
        let endian = match p.word()? {
            "little" => Endian::Little,
            "big" => Endian::Big,
            _ => return Err(p.previous_error("unknown byte order")),
        };
        p.expect("}")?;
        Ok((target, DataLayout { ptr_bits, endian }, version, 18))
    }

    #[test]
    fn wave_header_matches_reference_for_versions_errors_and_truncation() {
        let canonical = [
            "module",
            "{",
            "format_version",
            "8",
            "semantics_version",
            "1",
            "target",
            "\"🐋\"",
            "datalayout",
            "{",
            "ptr",
            "=",
            "64",
            ",",
            "endian",
            "=",
            "little",
            "}",
        ];
        let mut cases = Vec::new();
        cases.push(canonical.to_vec());
        for end in 0..canonical.len() {
            cases.push(canonical[..end].to_vec());
        }
        for index in 0..canonical.len() {
            for replacement in ["unexpected", "\"quoted\"", "0", "-1", "4294967296", "4294967296x"]
            {
                let mut words = canonical.to_vec();
                words[index] = replacement;
                cases.push(words);
            }
        }
        for (index, replacement) in
            [(3, "2"), (3, "3"), (3, "9"), (5, "2"), (12, "32"), (16, "big")]
        {
            let mut words = canonical.to_vec();
            words[index] = replacement;
            cases.push(words);
        }
        for words in cases {
            let source = words.join(" ");
            let wave_tokens = super::super::lexer::lex(&source, IrLimits::default()).unwrap();
            let reference_tokens =
                super::super::lexer::lex_reference(&source, IrLimits::default()).unwrap();
            match (parse(&wave_tokens), reference(reference_tokens)) {
                (Ok(actual), Ok(expected)) => assert_eq!(actual, expected, "{source:?}"),
                (Err(actual), Err(expected)) => {
                    assert_eq!(actual.location, expected.location, "{source:?}");
                    assert_eq!(actual.to_string(), expected.to_string(), "{source:?}");
                },
                (actual, expected) => {
                    panic!("Wave/reference header divergence for {source:?}: {actual:?} / {expected:?}")
                },
            }
        }
    }
}
