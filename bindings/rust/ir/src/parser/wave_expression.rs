// SPDX-License-Identifier: MPL-2.0
//! Internal postorder transport for Wave-owned constant-expression parsing.

use super::{
    lexer::{Kind, Token},
    wave_text::RawToken,
    wave_type, ParseError, ParseErrorKind,
};

use crate::{
    ConstBinaryOp, ConstCompareOp, ConstExpr, ConstExprKind, ConstRef, ConstValue, FloatBits,
    GlobalId, IrLimits, LimitError, Resource, Type, ValueId,
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Node {
    kind: u64,
    op: u64,
    type_first: u64,
    type_count: u64,
    low: u64,
    high: u64,
}

#[repr(C)]
#[derive(Default)]
struct RawResult {
    status: u64,
    reason: u64,
    at: u64,
    expected: u64,
    detail: u64,
    next: u64,
    expressions: u64,
    types: u64,
    spent: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_parse_expression(
        tokens: *const RawToken,
        token_count: u64,
        start: u64,
        depth: u64,
        format: u64,
        max_depth: u64,
        max_type_depth: u64,
        max_nodes: u64,
        max_work: u64,
        mode: u64,
        type_scratch: *mut wave_type::Node,
        type_capacity: u64,
        expression_scratch: *mut Node,
        expression_capacity: u64,
        output_types: *mut wave_type::Node,
        output_type_capacity: u64,
        output_expressions: *mut Node,
        output_expression_capacity: u64,
        result: *mut RawResult,
    ) -> i32;
}

pub(super) struct Options {
    pub start: usize,
    pub depth: usize,
    pub format: u32,
    pub limits: IrLimits,
    pub remaining_nodes: usize,
    pub remaining_work: usize,
}

pub(super) struct Parsed {
    pub expression: ConstExpr,
    pub next: usize,
    pub nodes: usize,
    pub work: usize,
}

fn error(tokens: &[Token<'_>], r: &RawResult, options: &Options) -> ParseError {
    let index = usize::try_from(r.at).unwrap_or(usize::MAX).min(tokens.len() - 1);
    let resource = |resource, limit| ParseErrorKind::ResourceLimit(LimitError { resource, limit });
    let syntax = |message: &str| ParseErrorKind::Syntax(message.into());
    let found = match &tokens[index].kind {
        Kind::Word(s) | Kind::Mark(s) => *s,
        Kind::String(_) => "<string>",
    };
    let expected_token =
        |expected: &str| ParseErrorKind::Syntax(format!("expected '{expected}', found '{found}'"));
    let kind = match r.status {
        1 if (1..=5).contains(&r.reason) && (r.reason != 1 || (20..=22).contains(&r.expected)) => {
            match r.reason {
                1 => expected_token(match r.expected {
                    20 => "(",
                    21 => ")",
                    22 => ",",
                    _ => "?",
                }),
                2 => syntax("extended constant operators require typed IR format 5"),
                3 => syntax("constant neg requires typed IR format 8"),
                4 => syntax("invalid numeric ID"),
                5 => syntax("ID out of range"),
                _ => ParseErrorKind::Backend("expression parser transport"),
            }
        },
        2 => resource(Resource::ConstantDepth, options.limits.max_const_depth),
        3 => resource(Resource::Nodes, options.limits.max_nodes),
        4 => resource(Resource::AnalysisWork, options.limits.max_analysis_work),
        7 => match r.reason {
            1 if (1..=6).contains(&r.expected)
                && (r.expected != 1 || (24..=31).contains(&r.detail)) =>
            {
                match r.expected {
                    1 => expected_token(match r.detail {
                        24 => "<",
                        25 => ">",
                        26 => "{",
                        27 => "}",
                        28 => "(",
                        29 => ")",
                        30 => "->",
                        31 => ",",
                        _ => "?",
                    }),
                    2 => syntax("expected unsigned decimal integer"),
                    3 => syntax("integer out of range"),
                    4 => syntax("expected word"),
                    5 => syntax("unknown type"),
                    6 => syntax("unknown calling convention"),
                    _ => ParseErrorKind::Backend("expression parser transport"),
                }
            },
            2 => resource(Resource::TypeDepth, options.limits.max_type_depth),
            3 => resource(Resource::Nodes, options.limits.max_nodes),
            4 => resource(Resource::AnalysisWork, options.limits.max_analysis_work),
            _ => ParseErrorKind::Backend("expression parser transport"),
        },
        8 => match r.reason {
            1 => syntax("expected boolean literal"),
            2 => syntax("expected signed decimal literal"),
            3 => syntax("signed literal out of range"),
            4 => syntax("expected unsigned decimal literal"),
            5 => syntax("unsigned literal out of range"),
            6 => syntax("float value must start with 0x"),
            7 if matches!(r.expected, 16 | 32 | 64) => syntax(&format!(
                "f{} requires exactly {} hexadecimal digits",
                r.expected,
                r.expected / 4
            )),
            8 => syntax("type has no scalar literal"),
            9 => syntax("literal outside type range"),
            10 => resource(Resource::AnalysisWork, options.limits.max_analysis_work),
            _ => ParseErrorKind::Backend("expression parser transport"),
        },
        _ => ParseErrorKind::Backend("expression parser transport"),
    };
    ParseError { location: tokens[index].location, kind }
}

fn decode(types: &[wave_type::Node], nodes: &[Node]) -> Option<ConstExpr> {
    let mut stack: Vec<ConstExpr> = Vec::new();
    stack.try_reserve_exact(nodes.len()).ok()?;
    for n in nodes {
        let first = usize::try_from(n.type_first).ok()?;
        let count = usize::try_from(n.type_count).ok()?;
        let ty = wave_type::decode(types.get(first..first.checked_add(count)?)?)?;
        let kind = match n.kind {
            0 => {
                let bits = (u128::from(n.high) << 64) | u128::from(n.low);
                ConstExprKind::Literal(match &ty {
                    Type::Bool if n.low <= 1 && n.high == 0 => ConstValue::Bool(n.low != 0),
                    Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 => {
                        ConstValue::I(bits as i128)
                    },
                    Type::U1 | Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 => {
                        ConstValue::U(bits)
                    },
                    Type::F16 if n.low <= u16::MAX.into() && n.high == 0 => {
                        ConstValue::F(FloatBits::F16(n.low as u16))
                    },
                    Type::F32 if n.low <= u32::MAX.into() && n.high == 0 => {
                        ConstValue::F(FloatBits::F32(n.low as u32))
                    },
                    Type::F64 if n.high == 0 => ConstValue::F(FloatBits::F64(n.low)),
                    _ => return None,
                })
            },
            1 | 2 => {
                let id = u32::try_from(n.op).ok()?;
                ConstExprKind::Reference(if n.kind == 1 {
                    ConstRef::Global(GlobalId(id))
                } else {
                    ConstRef::Local(ValueId(id))
                })
            },
            3 => ConstExprKind::Not(Box::new(stack.pop()?)),
            4 => ConstExprKind::Neg(Box::new(stack.pop()?)),
            5 => {
                let right = Box::new(stack.pop()?);
                let left = Box::new(stack.pop()?);
                let op = match n.op {
                    0 => ConstBinaryOp::Add,
                    1 => ConstBinaryOp::Sub,
                    2 => ConstBinaryOp::Mul,
                    3 => ConstBinaryOp::BitAnd,
                    4 => ConstBinaryOp::BitOr,
                    5 => ConstBinaryOp::BitXor,
                    6 => ConstBinaryOp::Div,
                    7 => ConstBinaryOp::Rem,
                    8 => ConstBinaryOp::ShiftLeft,
                    9 => ConstBinaryOp::ShiftRight,
                    10 => ConstBinaryOp::LogicalAnd,
                    11 => ConstBinaryOp::LogicalOr,
                    _ => return None,
                };
                ConstExprKind::Binary { op, left, right }
            },
            6 => {
                let right = Box::new(stack.pop()?);
                let left = Box::new(stack.pop()?);
                let op = match n.op {
                    0 => ConstCompareOp::Eq,
                    1 => ConstCompareOp::Ne,
                    2 => ConstCompareOp::Lt,
                    3 => ConstCompareOp::Le,
                    4 => ConstCompareOp::Gt,
                    5 => ConstCompareOp::Ge,
                    _ => return None,
                };
                ConstExprKind::Compare { op, left, right }
            },
            _ => return None,
        };
        stack.push(ConstExpr { ty, kind });
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
    let invoke = |mode,
                  type_scratch: &mut [wave_type::Node],
                  expression_scratch: &mut [Node],
                  output_types: &mut [wave_type::Node],
                  output_expressions: &mut [Node],
                  result: &mut RawResult| unsafe {
        whale_ir_parse_expression(
            raw.as_ptr(),
            raw.len() as u64,
            options.start as u64,
            options.depth as u64,
            options.format.into(),
            options.limits.max_const_depth as u64,
            options.limits.max_type_depth as u64,
            options.remaining_nodes as u64,
            options.remaining_work as u64,
            mode,
            type_scratch.as_mut_ptr(),
            type_scratch.len() as u64,
            expression_scratch.as_mut_ptr(),
            expression_scratch.len() as u64,
            output_types.as_mut_ptr(),
            output_types.len() as u64,
            output_expressions.as_mut_ptr(),
            output_expressions.len() as u64,
            result,
        )
    };
    let status = invoke(0, &mut [], &mut [], &mut [], &mut [], &mut result);
    if status != 0 || result.status != 0 {
        return Err(error(tokens, &result, &options));
    }
    let expr_count =
        usize::try_from(result.expressions).map_err(|_| error(tokens, &result, &options))?;
    let type_count = usize::try_from(result.types).map_err(|_| error(tokens, &result, &options))?;
    let next = usize::try_from(result.next).map_err(|_| error(tokens, &result, &options))?;
    let nodes = usize::try_from(result.spent).map_err(|_| error(tokens, &result, &options))?;
    let work = usize::try_from(result.work).map_err(|_| error(tokens, &result, &options))?;
    if expr_count == 0
        || type_count == 0
        || expr_count > raw.len()
        || type_count > raw.len()
        || next <= options.start
        || next >= raw.len()
        || nodes > options.remaining_nodes
        || work > options.remaining_work
    {
        return Err(error(tokens, &result, &options));
    }
    let allocate_types = || -> Result<Vec<wave_type::Node>, ParseError> {
        let mut v = Vec::new();
        v.try_reserve_exact(type_count).map_err(|_| error(tokens, &result, &options))?;
        v.resize(type_count, wave_type::Node::default());
        Ok(v)
    };
    let allocate_expressions = || -> Result<Vec<Node>, ParseError> {
        let mut v = Vec::new();
        v.try_reserve_exact(expr_count).map_err(|_| error(tokens, &result, &options))?;
        v.resize(expr_count, Node::default());
        Ok(v)
    };
    let mut type_scratch = allocate_types()?;
    let mut types = allocate_types()?;
    let mut expr_scratch = allocate_expressions()?;
    let mut expressions = allocate_expressions()?;
    let status =
        invoke(1, &mut type_scratch, &mut expr_scratch, &mut types, &mut expressions, &mut result);
    if status != 0
        || result.status != 0
        || result.expressions != expr_count as u64
        || result.types != type_count as u64
        || result.next != next as u64
        || result.spent != nodes as u64
        || result.work != work as u64
    {
        return Err(error(tokens, &result, &options));
    }
    let expression =
        decode(&types, &expressions).ok_or_else(|| error(tokens, &result, &options))?;
    Ok(Parsed { expression, next, nodes, work })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parser::{lexer, syntax::Parser};

    fn run(
        source: &str,
        version: u32,
        limits: IrLimits,
        reference: bool,
    ) -> Result<(ConstExpr, String), ParseError> {
        let tokens = if reference {
            lexer::lex_reference(source, limits)?
        } else {
            lexer::lex(source, limits)?
        };
        let mut parser = Parser::new(tokens, limits);
        parser.reference_types = reference;
        parser.format_version = version;
        let expression = parser.expression(0)?;
        Ok((expression, parser.peek().into()))
    }

    fn compare(source: &str, version: u32, limits: IrLimits) {
        match (run(source, version, limits, false), run(source, version, limits, true)) {
            (Ok(a), Ok(b)) => assert_eq!(a, b, "v{version} {source:?}"),
            (Err(a), Err(b)) => {
                assert_eq!(a.location, b.location, "v{version} {source:?}");
                assert_eq!(a.to_string(), b.to_string(), "v{version} {source:?}");
            },
            (a, b) => {
                panic!("Wave/reference expression divergence v{version} {source:?}: {a:?} / {b:?}")
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
        let choice = random(state) % if depth == 0 { 8 } else { 28 };
        match choice {
            0 => format!("i128 {}", random(state) as i64),
            1 => format!("u128 {}", random(state)),
            2 => format!("i32 @g{}", random(state) as u32),
            3 => format!("tuple<u32, ptr<i8>> %v{}", random(state) as u32),
            4 => format!("f64 0x{:016x}", random(state)),
            5 => format!("f32 0x{:08x}", random(state) as u32),
            6 => format!("f16 0x{:04x}", random(state) as u16),
            7 => if random(state) & 1 == 0 { "bool true" } else { "bool false" }.into(),
            8 | 9 => format!(
                "{}({})",
                if choice == 8 { "bit_not" } else { "neg" },
                generated(state, depth - 1)
            ),
            _ => {
                const OPS: [&str; 18] = [
                    "add",
                    "sub",
                    "mul",
                    "bit_and",
                    "bit_or",
                    "bit_xor",
                    "div",
                    "rem",
                    "shift_left",
                    "shift_right",
                    "logical_and",
                    "logical_or",
                    "eq",
                    "ne",
                    "lt",
                    "le",
                    "gt",
                    "ge",
                ];
                format!(
                    "{}({}, {})",
                    OPS[(choice - 10) as usize],
                    generated(state, depth - 1),
                    generated(state, depth - 1)
                )
            },
        }
    }

    #[test]
    fn wave_expression_grammar_matches_reference_and_preserves_diagnostics() {
        for seed in [
            "add(i32 1, i32 2)",
            "neg(bit_not(i128 -170141183460469231731687303715884105728))",
            "logical_and(eq(u8 1, u8 1), bool false)",
            "ptr<u8> @g12",
            "fnptr<whale () -> void> %v2",
            "i32 @g4294967296",
            "i32 @g4294967296x",
            "i32 @g",
            "i32 %v",
            "i32 %v-1",
            "array<i8, 18446744073709551616> @g0",
            "i128 170141183460469231731687303715884105728",
            "u128 340282366920938463463374607431768211456x",
            "f32 0xGGGGGGGG",
            "bool \"true\"",
            "i32 \"1\"",
            "struct{} 0",
            "bit_not(i32 1",
            "add(i32 1 i32 2)",
            "neg(i32)",
            "",
            "🐋",
        ] {
            for version in 3..=8 {
                compare(seed, version, IrLimits::default());
            }
            for (end, _) in seed.char_indices() {
                compare(&seed[..end], 8, IrLimits::default());
            }
            for max_nodes in 0..12 {
                compare(seed, 8, IrLimits { max_nodes, ..IrLimits::default() });
            }
            for max_const_depth in 0..4 {
                compare(seed, 8, IrLimits { max_const_depth, ..IrLimits::default() });
            }
        }
        let mut state = 0x349_351_u64;
        for _ in 0..10000 {
            let source = generated(&mut state, 5);
            compare(&source, 8, IrLimits::default());
            compare(&source, 3 + (random(&mut state) % 6) as u32, IrLimits::default());
            let end = random(&mut state) as usize % source.len();
            compare(&source[..end], 8, IrLimits::default());
        }
    }
}
