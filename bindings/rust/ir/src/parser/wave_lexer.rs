// SPDX-License-Identifier: MPL-2.0
//! Internal borrowed bridge to the Wave IR lexer. Rust owns allocation and
//! parser objects; Wave owns token boundaries, escapes and source positions.

use super::{
    lexer::{Kind, Token},
    ParseError, ParseErrorKind, SourceLocation,
};

use crate::{IrLimits, LimitError, Resource};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RawToken {
    kind: u64,
    start: u64,
    end: u64,
    line: u64,
    column: u64,
    decoded_start: u64,
    decoded_len: u64,
}

#[repr(C)]
#[derive(Default)]
struct RawResult {
    status: u64,
    reason: u64,
    offset: u64,
    line: u64,
    column: u64,
    token_count: u64,
    decoded_count: u64,
}

unsafe extern "C" {
    fn whale_ir_lex(
        source: *const u8,
        length: u64,
        max_tokens: u64,
        max_work: u64,
        mode: u64,
        scratch_tokens: *mut RawToken,
        token_capacity: u64,
        scratch_decoded: *mut u8,
        decoded_capacity: u64,
        output_tokens: *mut RawToken,
        output_token_capacity: u64,
        output_decoded: *mut u8,
        output_decoded_capacity: u64,
        result: *mut RawResult,
    ) -> i32;
}

fn error(result: &RawResult, limits: IrLimits) -> ParseError {
    let location = SourceLocation {
        byte_offset: result.offset as usize,
        line: result.line as usize,
        column: result.column as usize,
    };
    let kind = match result.status {
        2 => ParseErrorKind::ResourceLimit(LimitError {
            resource: Resource::Tokens,
            limit: limits.max_tokens,
        }),
        3 => ParseErrorKind::ResourceLimit(LimitError {
            resource: Resource::InputBytes,
            limit: limits.max_input_bytes,
        }),
        4 => ParseErrorKind::ResourceLimit(LimitError {
            resource: Resource::AnalysisWork,
            limit: limits.max_analysis_work,
        }),
        1 if (1..=8).contains(&result.reason) => ParseErrorKind::Syntax(
            match result.reason {
                2 => "unterminated string",
                3 => "unterminated escape",
                4 => "expected '{' in Unicode escape",
                5 => "invalid Unicode escape",
                6 => "unknown string escape",
                7 => "string control characters must be escaped",
                8 => "invalid Unicode scalar",
                _ => "invalid UTF-8 in IR source",
            }
            .into(),
        ),
        _ => ParseErrorKind::Backend("lexer transport"),
    };
    ParseError { location, kind }
}

pub(super) fn lex(source: &str, limits: IrLimits) -> Result<Vec<Token<'_>>, ParseError> {
    let start = SourceLocation { byte_offset: 0, line: 1, column: 1 };
    if source.len() > limits.max_input_bytes {
        return Err(ParseError {
            location: start,
            kind: ParseErrorKind::ResourceLimit(LimitError {
                resource: Resource::InputBytes,
                limit: limits.max_input_bytes,
            }),
        });
    }
    let length = source.len() as u64;
    let work = length.saturating_mul(8).saturating_add(32);
    let mut result = RawResult::default();
    let status = unsafe {
        whale_ir_lex(
            source.as_ptr(),
            length,
            limits.max_tokens as u64,
            work,
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            0,
            &mut result,
        )
    };
    if status != 0 || result.status != 0 {
        return Err(error(&result, limits));
    }
    let count = usize::try_from(result.token_count).map_err(|_| error(&result, limits))?;
    let decoded_count =
        usize::try_from(result.decoded_count).map_err(|_| error(&result, limits))?;
    if count == 0
        || count > limits.max_tokens.saturating_add(1)
        || count > source.len().saturating_add(1)
        || decoded_count > source.len()
    {
        return Err(error(&result, limits));
    }
    let mut scratch = Vec::new();
    let mut output = Vec::new();
    let mut scratch_decoded = Vec::new();
    let mut decoded = Vec::new();
    for tokens in [&mut scratch, &mut output] {
        tokens.try_reserve_exact(count).map_err(|_| error(&result, limits))?;
        tokens.resize(count, RawToken::default());
    }
    for bytes in [&mut scratch_decoded, &mut decoded] {
        bytes.try_reserve_exact(decoded_count).map_err(|_| error(&result, limits))?;
        bytes.resize(decoded_count, 0);
    }
    result = RawResult::default();
    let status = unsafe {
        whale_ir_lex(
            source.as_ptr(),
            length,
            limits.max_tokens as u64,
            work,
            1,
            scratch.as_mut_ptr(),
            count as u64,
            scratch_decoded.as_mut_ptr(),
            decoded_count as u64,
            output.as_mut_ptr(),
            count as u64,
            decoded.as_mut_ptr(),
            decoded_count as u64,
            &mut result,
        )
    };
    if status != 0
        || result.status != 0
        || result.token_count != count as u64
        || result.decoded_count != decoded_count as u64
    {
        return Err(error(&result, limits));
    }
    let mut tokens = Vec::new();
    tokens.try_reserve_exact(count).map_err(|_| error(&result, limits))?;
    for raw in output {
        let offset = usize::try_from(raw.start).map_err(|_| error(&result, limits))?;
        let end = usize::try_from(raw.end).map_err(|_| error(&result, limits))?;
        let decoded_start =
            usize::try_from(raw.decoded_start).map_err(|_| error(&result, limits))?;
        let decoded_len = usize::try_from(raw.decoded_len).map_err(|_| error(&result, limits))?;
        let word = source.get(offset..end).ok_or_else(|| error(&result, limits))?;
        let kind = match raw.kind {
            0 => Kind::Word(word),
            1 => {
                let bytes = decoded
                    .get(decoded_start..decoded_start.saturating_add(decoded_len))
                    .ok_or_else(|| error(&result, limits))?;
                Kind::String(String::from_utf8(bytes.to_vec()).map_err(|_| error(&result, limits))?)
            },
            2 => Kind::Mark(word),
            3 if offset == source.len() && end == offset => Kind::Mark("<eof>"),
            _ => return Err(error(&result, limits)),
        };
        tokens.push(Token {
            kind,
            location: SourceLocation {
                byte_offset: offset,
                line: raw.line as usize,
                column: raw.column as usize,
            },
        });
    }
    Ok(tokens)
}
