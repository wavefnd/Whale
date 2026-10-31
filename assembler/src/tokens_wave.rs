// SPDX-License-Identifier: MPL-2.0
//! Allocation and typed transport for the native Wave source lexer.
use super::{Token, TokenKind};

use crate::{AsmError, SourceSpan};

const MAX_SOURCE: usize = 8 * 1024 * 1024;

const BACKEND: &str = "assembler lexer transport";

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RawToken {
    kind: u64,
    start: u64,
    end: u64,
    line: u64,
    column: u64,
    end_line: u64,
    end_column: u64,
    decoded_start: u64,
    decoded_len: u64,
    low: u64,
    high: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct RawResult {
    status: u64,
    reason: u64,
    start: u64,
    end: u64,
    line: u64,
    column: u64,
    detail: u64,
    tokens: u64,
    decoded: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_asm_lex(
        source: *const u8,
        length: u64,
        max_tokens: u64,
        max_decoded: u64,
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

fn backend() -> AsmError {
    AsmError::Backend(BACKEND)
}

fn initialized<T: Default + Clone>(count: usize) -> Result<Vec<T>, AsmError> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| AsmError::Backend("assembler lexer allocation"))?;
    values.resize(count, T::default());
    Ok(values)
}

fn copied<T: Clone>(source: &[T]) -> Result<Vec<T>, AsmError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(source.len())
        .map_err(|_| AsmError::Backend("assembler lexer allocation"))?;
    values.extend_from_slice(source);
    Ok(values)
}

fn index(value: u64) -> Result<usize, AsmError> {
    usize::try_from(value).map_err(|_| backend())
}

fn failure(source: &str, result: &RawResult) -> AsmError {
    if result.status == 5 {
        return backend();
    }
    let Ok(start) = index(result.start) else { return backend() };
    let Ok(end) = index(result.end) else { return backend() };
    let Ok(line) = index(result.line) else { return backend() };
    let Ok(column) = index(result.column) else { return backend() };
    if start > end || source.get(start..end).is_none() || line == 0 || column == 0 {
        return backend();
    }
    let message = match (result.status, result.reason) {
        (1, 1) => match u32::try_from(result.detail).ok().and_then(char::from_u32) {
            Some(ch) => format!("Unexpected char '{ch}'"),
            None => return backend(),
        },
        (1, 2) => "Unterminated string literal".into(),
        (1, 3) => "Truncated escape".into(),
        (1, 4) => "Truncated byte escape; expected two hex digits".into(),
        (1, 5) => "Invalid byte escape; expected two hex digits".into(),
        (1, 6) => "Unknown string escape".into(),
        (1, 7) => "Invalid hex number".into(),
        (1, 8) => "Invalid binary number".into(),
        (1, 9) => "Number exceeds unsigned 64-bit range".into(),
        (2, 0) => "assembly token limit exceeded".into(),
        (3, 0) => "assembly decoded-byte limit exceeded".into(),
        (4, 0) => "assembly lexer work limit exceeded".into(),
        (6, 0) => "assembly input exceeds 8388608 bytes".into(),
        _ => return backend(),
    };
    AsmError::LexerError(message).at("<input>", SourceSpan { start, end, line, column }, None)
}

fn call(
    source: &str,
    mode: u64,
    tokens: &mut [RawToken],
    decoded: &mut [u8],
) -> Result<RawResult, AsmError> {
    let maximum = source.len() as u64;
    let max_work = maximum.saturating_mul(2).saturating_add(1);
    let mut result = RawResult::default();
    // Both aliases address a private initialized buffer. Native publication only
    // copies scratch after complete validation; no public Token exists yet.
    let status = unsafe {
        whale_asm_lex(
            source.as_ptr(),
            maximum,
            maximum,
            maximum,
            max_work,
            mode,
            tokens.as_mut_ptr(),
            tokens.len() as u64,
            decoded.as_mut_ptr(),
            decoded.len() as u64,
            tokens.as_mut_ptr(),
            tokens.len() as u64,
            decoded.as_mut_ptr(),
            decoded.len() as u64,
            &mut result,
        )
    };
    if i32::try_from(result.status).ok() != Some(status)
        || result.tokens > maximum
        || result.decoded > maximum
        || result.work > max_work
    {
        return Err(backend());
    }
    if status != 0 {
        return Err(failure(source, &result));
    }
    if source.len() > MAX_SOURCE
        || (mode == 1
            && (result.tokens > tokens.len() as u64 || result.decoded > decoded.len() as u64))
    {
        return Err(backend());
    }
    Ok(result)
}

pub(super) fn tokenize(source: &str) -> Result<Vec<Token>, AsmError> {
    let probe = call(source, 0, &mut [], &mut [])?;
    let mut native = initialized::<RawToken>(index(probe.tokens)?)?;
    let mut decoded = initialized::<u8>(index(probe.decoded)?)?;
    let result = call(source, 1, &mut native, &mut decoded)?;
    if result.tokens != probe.tokens || result.decoded != probe.decoded || result.work != probe.work
    {
        return Err(backend());
    }
    let mut tokens = Vec::new();
    tokens
        .try_reserve_exact(native.len())
        .map_err(|_| AsmError::Backend("assembler lexer allocation"))?;
    let mut previous = 0;
    for raw in native {
        let start = index(raw.start)?;
        let end = index(raw.end)?;
        let text = source.get(start..end).ok_or_else(backend)?;
        if start < previous
            || start == end
            || raw.high != 0
            || raw.line == 0
            || raw.column == 0
            || raw.end_line < raw.line
            || raw.end_column == 0
        {
            return Err(backend());
        }
        previous = end;
        let kind = match raw.kind {
            0 => TokenKind::Identifier(
                String::from_utf8(copied(text.as_bytes())?).map_err(|_| backend())?,
            ),
            1 => TokenKind::Number(i128::from(raw.low)),
            2 => {
                let first = index(raw.decoded_start)?;
                let end = first.checked_add(index(raw.decoded_len)?).ok_or_else(backend)?;
                TokenKind::StringLiteral(copied(decoded.get(first..end).ok_or_else(backend)?)?)
            },
            3 => TokenKind::Comma,
            4 => TokenKind::Colon,
            5 => TokenKind::LBracket,
            6 => TokenKind::RBracket,
            7 => TokenKind::Plus,
            8 => TokenKind::Minus,
            9 => TokenKind::Multiply,
            10 => TokenKind::Divide,
            11 => TokenKind::Newline,
            _ => return Err(backend()),
        };
        tokens.push(Token {
            kind,
            position: start,
            end_position: end,
            line: index(raw.line)?,
            column: index(raw.column)?,
            end_line: index(raw.end_line)?,
            end_column: index(raw.end_column)?,
        });
    }
    Ok(tokens)
}
