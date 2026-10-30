// SPDX-License-Identifier: MPL-2.0
//! Conversion between the internal Wave postorder type model and Rust callers.

use super::{lexer::Kind, lexer::Token, wave_text::RawToken, ParseError, ParseErrorKind};

use crate::{CallingConvention, FunctionSignature, IrLimits, LimitError, Resource, Type};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Node {
    kind: u64,
    value: u64,
    count: u64,
}

#[repr(C)]
#[derive(Default)]
struct RawResult {
    status: u64,
    reason: u64,
    at: u64,
    expected: u64,
    next: u64,
    nodes: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_parse_type(
        tokens: *const RawToken,
        token_count: u64,
        start: u64,
        depth: u64,
        max_depth: u64,
        max_nodes: u64,
        max_work: u64,
        mode: u64,
        scratch: *mut Node,
        capacity: u64,
        output: *mut Node,
        output_capacity: u64,
        result: *mut RawResult,
    ) -> i32;
}

pub(super) struct Options {
    pub start: usize,
    pub depth: usize,
    pub limits: IrLimits,
    pub remaining_nodes: usize,
    pub remaining_work: usize,
}

pub(super) struct Parsed {
    pub ty: Type,
    pub next: usize,
    pub nodes: usize,
    pub work: usize,
}

fn error(tokens: &[Token<'_>], result: &RawResult, options: &Options) -> ParseError {
    let index = usize::try_from(result.at).unwrap_or(usize::MAX).min(tokens.len() - 1);
    let resource = |resource, limit| ParseErrorKind::ResourceLimit(LimitError { resource, limit });
    let kind = match result.status {
        2 => resource(Resource::TypeDepth, options.limits.max_type_depth),
        3 => resource(Resource::Nodes, options.limits.max_nodes),
        4 => resource(Resource::AnalysisWork, options.limits.max_analysis_work),
        1 if (1..=6).contains(&result.reason)
            && (result.reason != 1 || (24..=31).contains(&result.expected)) =>
        {
            ParseErrorKind::Syntax(match result.reason {
                1 => {
                    let expected = match result.expected {
                        24 => "<",
                        25 => ">",
                        26 => "{",
                        27 => "}",
                        28 => "(",
                        29 => ")",
                        30 => "->",
                        31 => ",",
                        _ => "?",
                    };
                    let found = match &tokens[index].kind {
                        Kind::Word(s) | Kind::Mark(s) => *s,
                        Kind::String(_) => "<string>",
                    };
                    format!("expected '{expected}', found '{found}'")
                },
                2 => "expected unsigned decimal integer".into(),
                3 => "integer out of range".into(),
                4 => "expected word".into(),
                5 => "unknown type".into(),
                6 => "unknown calling convention".into(),
                _ => "invalid Wave IR type result".into(),
            })
        },
        _ => ParseErrorKind::Backend("type parser transport"),
    };
    ParseError { location: tokens[index].location, kind }
}

fn children(stack: &mut Vec<Type>, count: u64) -> Option<Vec<Type>> {
    let count = usize::try_from(count).ok()?;
    let first = stack.len().checked_sub(count)?;
    let mut result = Vec::new();
    result.try_reserve_exact(count).ok()?;
    result.extend(stack.drain(first..));
    Some(result)
}

pub(super) fn decode(nodes: &[Node]) -> Option<Type> {
    let mut stack = Vec::new();
    stack.try_reserve_exact(nodes.len()).ok()?;
    for node in nodes {
        let ty = match node.kind {
            0..=16 if node.count == 0 && node.value == 0 => match node.kind {
                0 => Type::Void,
                1 => Type::Bool,
                2 => Type::I1,
                3 => Type::U1,
                4 => Type::I8,
                5 => Type::I16,
                6 => Type::I32,
                7 => Type::I64,
                8 => Type::I128,
                9 => Type::U8,
                10 => Type::U16,
                11 => Type::U32,
                12 => Type::U64,
                13 => Type::U128,
                14 => Type::F16,
                15 => Type::F32,
                16 => Type::F64,
                _ => unreachable!(),
            },
            17 if node.count == 1 && node.value == 0 => Type::ptr_to(stack.pop()?),
            18 if node.value <= 1 => {
                let ret = stack.pop()?;
                let params = children(&mut stack, node.count)?;
                Type::FnPtr(Box::new(FunctionSignature {
                    params,
                    ret,
                    variadic: false,
                    convention: if node.value == 0 {
                        CallingConvention::Whale
                    } else {
                        CallingConvention::SysV64
                    },
                }))
            },
            19 if node.count == 1 => Type::Array(Box::new(stack.pop()?), node.value),
            20 if node.value == 0 => Type::Struct(children(&mut stack, node.count)?),
            21 if node.value == 0 => Type::Tuple(children(&mut stack, node.count)?),
            _ => return None,
        };
        stack.push(ty);
    }
    if stack.len() != 1 {
        return None;
    }
    stack.pop()
}

pub(super) fn parse(
    tokens: &[Token<'_>],
    raw: &[RawToken],
    options: Options,
) -> Result<Parsed, ParseError> {
    let mut result = RawResult::default();
    let status = unsafe {
        whale_ir_parse_type(
            raw.as_ptr(),
            raw.len() as u64,
            options.start as u64,
            options.depth as u64,
            options.limits.max_type_depth as u64,
            options.remaining_nodes as u64,
            options.remaining_work as u64,
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            0,
            &mut result,
        )
    };
    if status != 0 || result.status != 0 {
        return Err(error(tokens, &result, &options));
    }
    let count = usize::try_from(result.nodes).map_err(|_| error(tokens, &result, &options))?;
    let next = usize::try_from(result.next).map_err(|_| error(tokens, &result, &options))?;
    let work = usize::try_from(result.work).map_err(|_| error(tokens, &result, &options))?;
    if count == 0
        || count > options.remaining_nodes
        || count > raw.len()
        || next <= options.start
        || next >= raw.len()
        || work > options.remaining_work
    {
        return Err(error(tokens, &result, &options));
    }
    let mut scratch = Vec::new();
    let mut output = Vec::new();
    for buffer in [&mut scratch, &mut output] {
        buffer.try_reserve_exact(count).map_err(|_| error(tokens, &result, &options))?;
        buffer.resize(count, Node::default());
    }
    let status = unsafe {
        whale_ir_parse_type(
            raw.as_ptr(),
            raw.len() as u64,
            options.start as u64,
            options.depth as u64,
            options.limits.max_type_depth as u64,
            options.remaining_nodes as u64,
            options.remaining_work as u64,
            1,
            scratch.as_mut_ptr(),
            count as u64,
            output.as_mut_ptr(),
            count as u64,
            &mut result,
        )
    };
    if status != 0
        || result.status != 0
        || result.nodes != count as u64
        || result.next != next as u64
        || result.work != work as u64
    {
        return Err(error(tokens, &result, &options));
    }
    let ty = decode(&output).ok_or_else(|| error(tokens, &result, &options))?;
    Ok(Parsed { ty, next, nodes: count, work })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parser::{lexer, syntax::Parser};

    fn parse_case(
        text: &str,
        limits: IrLimits,
        reference: bool,
    ) -> Result<(Type, String), ParseError> {
        let tokens =
            if reference { lexer::lex_reference(text, limits)? } else { lexer::lex(text, limits)? };
        let mut parser = Parser::new(tokens, limits);
        parser.reference_types = reference;
        let ty = parser.ty(0)?;
        Ok((ty, parser.peek().to_owned()))
    }

    fn compare(text: &str, limits: IrLimits) {
        match (parse_case(text, limits, false), parse_case(text, limits, true)) {
            (Ok(actual), Ok(expected)) => assert_eq!(actual, expected, "{text:?}"),
            (Err(actual), Err(expected)) => {
                assert_eq!(actual.location, expected.location, "{text:?}");
                assert_eq!(actual.to_string(), expected.to_string(), "{text:?}");
            },
            (actual, expected) => {
                panic!("Wave/reference type divergence for {text:?}: {actual:?} / {expected:?}")
            },
        }
    }

    fn random(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    fn generated(state: &mut u64, depth: usize) -> String {
        const LEAVES: [&str; 17] = [
            "void", "bool", "i1", "u1", "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32",
            "u64", "u128", "f16", "f32", "f64",
        ];
        let choice = random(state) % if depth == 0 { 17 } else { 22 };
        match choice {
            0..=16 => LEAVES[choice as usize].into(),
            17 => format!("ptr<{}>", generated(state, depth - 1)),
            18 => {
                let convention = if random(state) & 1 == 0 { "whale" } else { "sysv64" };
                let params = (0..random(state) % 4)
                    .map(|_| generated(state, depth - 1))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fnptr<{convention} ({params}) -> {}>", generated(state, depth - 1))
            },
            19 => {
                let count = random(state);
                format!("array<{}, {count}>", generated(state, depth - 1))
            },
            20 | 21 => {
                let fields = (0..random(state) % 4)
                    .map(|_| generated(state, depth - 1))
                    .collect::<Vec<_>>()
                    .join(", ");
                if choice == 20 {
                    format!("struct{{{fields}}}")
                } else {
                    format!("tuple<{fields}>")
                }
            },
            _ => unreachable!(),
        }
    }

    #[test]
    fn wave_types_match_reference_for_complete_grammar_and_failures() {
        let seeds = [
            "fnptr<whale () -> void>",
            "fnptr<sysv64 (ptr<u8>, tuple<i128, f16>) -> struct{u1, array<i32, 18446744073709551615>}>",
            "tuple<>", "struct{}", "ptr<ptr<ptr<u8>>>",
            "array<i8, 18446744073709551616>", "array<i8, 18446744073709551616x>",
            "array<i8, -1>", "fnptr<unknown () -> i32>", "fnptr<\"whale\" () -> i32>",
            "struct{i32,}", "tuple<,i32>", "\"i32\"", "", "🐋", "array<i8 1>",
        ];
        for seed in seeds {
            compare(seed, IrLimits::default());
            for (end, _) in seed.char_indices() {
                compare(&seed[..end], IrLimits::default());
            }
            for max_type_depth in 0..4 {
                compare(seed, IrLimits { max_type_depth, ..IrLimits::default() });
            }
            for max_nodes in 0..8 {
                compare(seed, IrLimits { max_nodes, ..IrLimits::default() });
            }
        }
        let mut state = 0x348_349_350_u64;
        for _ in 0..10000 {
            let text = generated(&mut state, 5);
            compare(&text, IrLimits::default());
            if text.len() > 1 {
                let end = (random(&mut state) as usize) % text.len();
                compare(&text[..end], IrLimits::default());
            }
        }
    }

    #[test]
    fn wave_type_limits_are_cumulative_across_types() {
        let limits = IrLimits { max_nodes: 2, ..IrLimits::default() };
        let mut parser = Parser::new(lexer::lex("i32 ptr<u8>", limits).unwrap(), limits);
        assert_eq!(parser.ty(0).unwrap(), Type::I32);
        let error = parser.ty(0).unwrap_err();
        assert!(matches!(
            error.kind,
            ParseErrorKind::ResourceLimit(LimitError { resource: Resource::Nodes, limit: 2 })
        ));
        assert_eq!(error.location.byte_offset, 8);
        let limits = IrLimits { max_analysis_work: 3, ..IrLimits::default() };
        let mut parser = Parser::new(lexer::lex("i32 i32", limits).unwrap(), limits);
        assert_eq!(parser.ty(0).unwrap(), Type::I32);
        assert!(matches!(
            parser.ty(0).unwrap_err().kind,
            ParseErrorKind::ResourceLimit(LimitError {
                resource: Resource::AnalysisWork,
                limit: 3
            })
        ));
    }
}
