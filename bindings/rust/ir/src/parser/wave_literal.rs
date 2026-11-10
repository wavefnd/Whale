// SPDX-License-Identifier: MPL-2.0
//! Exact scalar payload transport for Wave text decoding.

use super::ParseErrorKind;

use crate::{ConstValue, FloatBits, LimitError, Resource, Type};

#[repr(C)]
#[derive(Default)]
struct RawResult {
    status: u64,
    low: u64,
    high: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_parse_literal(
        source: *const u8,
        length: u64,
        category: u64,
        width: u64,
        max_work: u64,
        result: *mut RawResult,
    ) -> i32;
}

pub(super) fn parse(
    text: &str,
    ty: &Type,
    remaining_work: usize,
    work_limit: usize,
) -> Result<(ConstValue, usize), ParseErrorKind> {
    let (category, width) = match ty {
        Type::Bool => (1, 1),
        Type::I1 => (2, 1),
        Type::I8 => (2, 8),
        Type::I16 => (2, 16),
        Type::I32 => (2, 32),
        Type::I64 => (2, 64),
        Type::I128 => (2, 128),
        Type::U1 => (3, 1),
        Type::U8 => (3, 8),
        Type::U16 => (3, 16),
        Type::U32 => (3, 32),
        Type::U64 => (3, 64),
        Type::U128 => (3, 128),
        Type::F16 => (4, 16),
        Type::F32 => (4, 32),
        Type::F64 => (4, 64),
        _ => (0, 0),
    };
    let mut result = RawResult::default();
    let status = unsafe {
        whale_ir_parse_literal(
            text.as_ptr(),
            text.len() as u64,
            category,
            width,
            remaining_work as u64,
            &mut result,
        )
    };
    if status != 0 || result.status != 0 {
        if result.status == 10 {
            return Err(ParseErrorKind::ResourceLimit(LimitError {
                resource: Resource::AnalysisWork,
                limit: work_limit,
            }));
        }
        if !(1..=9).contains(&result.status) {
            return Err(ParseErrorKind::Backend("literal transport"));
        }
        return Err(ParseErrorKind::Syntax(match result.status {
            1 => "expected boolean literal".into(),
            2 => "expected signed decimal literal".into(),
            3 => "signed literal out of range".into(),
            4 => "expected unsigned decimal literal".into(),
            5 => "unsigned literal out of range".into(),
            6 => "float value must start with 0x".into(),
            7 => format!("f{width} requires exactly {} hexadecimal digits", width / 4),
            8 => "type has no scalar literal".into(),
            9 => "literal outside type range".into(),
            _ => "invalid Wave IR literal result".into(),
        }));
    }
    let invalid = || ParseErrorKind::Backend("literal transport");
    if result.work != text.len() as u64 || result.work > remaining_work as u64 {
        return Err(invalid());
    }
    let bits = (u128::from(result.high) << 64) | u128::from(result.low);
    let value = match category {
        1 if result.high == 0 && result.low <= 1 => ConstValue::Bool(result.low != 0),
        2 => ConstValue::I(bits as i128),
        3 => ConstValue::U(bits),
        4 if result.high == 0 => ConstValue::F(match width {
            16 if result.low <= u16::MAX.into() => FloatBits::F16(result.low as u16),
            32 if result.low <= u32::MAX.into() => FloatBits::F32(result.low as u32),
            64 => FloatBits::F64(result.low),
            _ => return Err(invalid()),
        }),
        _ => return Err(invalid()),
    };
    Ok((value, result.work as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parser::{lexer, syntax::Parser, ParseError};

    use crate::IrLimits;

    fn run(text: &str, ty: &Type, reference: bool) -> Result<ConstValue, ParseError> {
        let limits = IrLimits::default();
        let tokens =
            if reference { lexer::lex_reference(text, limits)? } else { lexer::lex(text, limits)? };
        let mut parser = Parser::new(tokens, limits);
        parser.reference_types = reference;
        parser.literal(ty)
    }

    fn compare(text: &str, ty: &Type) {
        match (run(text, ty, false), run(text, ty, true)) {
            (Ok(a), Ok(b)) => assert_eq!(a, b, "{text:?}: {ty}"),
            (Err(a), Err(b)) => {
                assert_eq!(a.location, b.location, "{text:?}: {ty}");
                assert_eq!(a.to_string(), b.to_string(), "{text:?}: {ty}");
            },
            (a, b) => panic!("Wave/reference literal divergence for {text:?}: {ty}: {a:?} / {b:?}"),
        }
    }

    #[test]
    fn wave_literals_match_reference_for_exact_bits_ranges_and_lexical_errors() {
        let types = [
            Type::Void,
            Type::Bool,
            Type::I1,
            Type::U1,
            Type::I8,
            Type::I16,
            Type::I32,
            Type::I64,
            Type::I128,
            Type::U8,
            Type::U16,
            Type::U32,
            Type::U64,
            Type::U128,
            Type::F16,
            Type::F32,
            Type::F64,
            Type::ptr_to(Type::U8),
        ];
        for ty in &types {
            for text in [
                "",
                "-",
                "-0",
                "+1",
                "true",
                "false",
                "\"1\"",
                "🐋",
                "0x0000",
                "0x7F800001",
                "0x7ff0000000000001",
                "0X0000",
                "0xgggg",
                "170141183460469231731687303715884105727",
                "170141183460469231731687303715884105728",
                "-170141183460469231731687303715884105728",
                "-170141183460469231731687303715884105729",
                "340282366920938463463374607431768211455",
                "340282366920938463463374607431768211456",
                "340282366920938463463374607431768211456x",
            ] {
                compare(text, ty);
            }
        }
        let mut state = 0x349_352_u128;
        for index in 0..10000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let ty = &types[index % types.len()];
            compare(&state.to_string(), ty);
            compare(&(state as i128).to_string(), ty);
            if index % 4 == 0 {
                compare(&format!("{state}x"), ty);
            }
        }
    }

    #[test]
    fn type_and_literal_parsing_share_the_work_ceiling() {
        let limits = IrLimits { max_analysis_work: 3, ..IrLimits::default() };
        let mut parser = Parser::new(lexer::lex("i32 1", limits).unwrap(), limits);
        let ty = parser.ty(0).unwrap();
        assert!(matches!(
            parser.literal(&ty).unwrap_err().kind,
            ParseErrorKind::ResourceLimit(LimitError {
                resource: Resource::AnalysisWork,
                limit: 3
            })
        ));
    }
}
