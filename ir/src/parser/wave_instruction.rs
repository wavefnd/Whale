// SPDX-License-Identifier: MPL-2.0
//! Rust materialization of events from the Wave instruction grammar. Production
//! choice, primitive parsing, loops, version gates and result rules stay in Wave.
use super::{
    instruction::ParsedInstruction,
    lexer::{Kind, Token},
    syntax::Parser,
    wave_text::RawToken,
    ParseError, ParseErrorKind,
};

use crate::*;

#[repr(C)]
#[derive(Default)]
struct State {
    phase: u64,
    at: u64,
    format: u64,
    mode: u64,
    op: u64,
    sub: u64,
    op_at: u64,
    has_dst: u64,
    dst: u64,
    annotation_kind: u64,
    type_kind: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    name: u64,
    aux: u64,
    nodes: u64,
    work: u64,
}

#[repr(C)]
#[derive(Default)]
struct Event {
    status: u64,
    reason: u64,
    at: u64,
    expected: u64,
    event: u64,
    role: u64,
    op: u64,
    sub: u64,
    has_dst: u64,
    dst: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    name: u64,
    aux: u64,
    next: u64,
    nodes: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_instruction_step(
        tokens: *const RawToken,
        count: u64,
        state: *mut State,
        max_nodes: u64,
        max_work: u64,
        result: *mut Event,
    ) -> i32;
}

const WORDS: &[&str] = &[
    ":",
    "=",
    ",",
    "[",
    "]",
    "(",
    ")",
    "<",
    ">",
    "bool",
    "ptr",
    "u8",
    "u64",
    "align",
    "label",
    "reason",
    "to",
    "=>",
    "indirect",
    "whale",
    "sysv64",
    "global_addr",
    "lifetime_end",
    "ptrdiff",
    "const_decl",
    "const",
    "undef",
    "mov",
    "neg",
    "not",
    "cmp",
    "icmp",
    "fcmp",
    "select",
    "phi",
    "extract",
    "alloca",
    "volatile_load",
    "volatile_store",
    "load",
    "uninit",
    "store",
    "gep",
    "memcpy",
    "memset",
    "null_function",
    "function_addr",
    "call",
    "trap_if",
    "br",
    "cbr",
    "switch",
    "ret",
    "trap",
    "add",
    "sub",
    "mul",
    "udiv",
    "sdiv",
    "urem",
    "srem",
    "fadd",
    "fsub",
    "fmul",
    "fdiv",
    "frem",
    "and",
    "or",
    "xor",
    "shl",
    "lshr",
    "ashr",
    "checked_int",
    "zext",
    "sext",
    "trunc",
    "fext",
    "ftrunc",
    "itof_s",
    "itof_u",
    "ftoi_s",
    "ftoi_u",
    "bitcast",
    "ptrtoint",
    "inttoptr",
    "uadd_chk",
    "usub_chk",
    "umul_chk",
    "sadd_chk",
    "ssub_chk",
    "smul_chk",
    "eq",
    "ne",
    "slt",
    "sle",
    "sgt",
    "sge",
    "ult",
    "ule",
    "ugt",
    "uge",
    "feq",
    "fne",
    "flt",
    "fle",
    "fgt",
    "fge",
    "oeq",
    "one",
    "olt",
    "ole",
    "ogt",
    "oge",
    "ord",
    "uno",
    "ueq",
    "une",
];

fn type_kind(ty: &Type) -> u64 {
    match ty {
        Type::Void => 0,
        Type::Bool => 1,
        Type::I1 => 2,
        Type::U1 => 3,
        Type::I8 => 4,
        Type::I16 => 5,
        Type::I32 => 6,
        Type::I64 => 7,
        Type::I128 => 8,
        Type::U8 => 9,
        Type::U16 => 10,
        Type::U32 => 11,
        Type::U64 => 12,
        Type::U128 => 13,
        Type::F16 => 14,
        Type::F32 => 15,
        Type::F64 => 16,
        Type::Ptr(_) => 17,
        Type::FnPtr(_) => 18,
        Type::Array(..) => 19,
        Type::Struct(_) => 20,
        Type::Tuple(_) => 21,
    }
}

fn error(tokens: &[Token<'_>], event: &Event, limits: IrLimits) -> ParseError {
    let index = (event.at as usize).min(tokens.len() - 1);
    let found = match &tokens[index].kind {
        Kind::Word(s) | Kind::Mark(s) => *s,
        Kind::String(_) => "<string>",
    };
    if event.at as usize >= tokens.len()
        || (event.status == 1 && event.reason == 1 && event.expected as usize >= WORDS.len())
        || (event.status == 1 && event.reason == 6 && event.expected >= 4)
    {
        return ParseError {
            location: tokens[index].location,
            kind: ParseErrorKind::Backend("instruction parser"),
        };
    }
    let kind = match event.status {
        3 => ParseErrorKind::ResourceLimit(LimitError {
            resource: Resource::Nodes,
            limit: limits.max_nodes,
        }),
        4 => ParseErrorKind::ResourceLimit(LimitError {
            resource: Resource::AnalysisWork,
            limit: limits.max_analysis_work,
        }),
        1 if (1..=25).contains(&event.reason) => ParseErrorKind::Syntax(match event.reason {
            1 => format!(
                "expected '{}', found '{found}'",
                WORDS.get(event.expected as usize).unwrap_or(&"?")
            ),
            2 => "expected word".into(),
            3 => "expected quoted string".into(),
            4 => "expected unsigned decimal integer".into(),
            5 => "integer out of range".into(),
            6 => format!(
                "expected {} ID",
                ["%v", "%b", "@g", "@f"].get(event.expected as usize).unwrap_or(&"?")
            ),
            7 => "invalid numeric ID".into(),
            8 => "ID out of range".into(),
            9 => "instruction requires a result".into(),
            10 => "global_addr requires a data-pointer result".into(),
            11 => "global/lifetime/pointer operations require typed IR format 6".into(),
            12 => "neg requires typed IR format 8".into(),
            13 => "pointer ordering requires typed IR format 6".into(),
            14 => "unknown comparison predicate".into(),
            15 => "unknown integer predicate".into(),
            16 => "unknown float predicate".into(),
            17 => "checked_int requires typed IR format 8".into(),
            18 => "uninit requires typed IR format 4".into(),
            19 => "volatile accesses require typed IR format 8".into(),
            20 => "load requires a result".into(),
            21 => "function address requires fnptr type".into(),
            22 => "unknown calling convention".into(),
            23 => "unknown instruction".into(),
            24 => "incorrect result presence".into(),
            25 => "unknown terminator".into(),
            _ => "invalid Wave IR instruction result".into(),
        }),
        _ => ParseErrorKind::Backend("instruction parser"),
    };
    ParseError { location: tokens[index].location, kind }
}

fn bin(sub: u64) -> Option<BinOp> {
    Some(match sub {
        0 => BinOp::Add,
        1 => BinOp::Sub,
        2 => BinOp::Mul,
        3 => BinOp::UDiv,
        4 => BinOp::SDiv,
        5 => BinOp::URem,
        6 => BinOp::SRem,
        7 => BinOp::FAdd,
        8 => BinOp::FSub,
        9 => BinOp::FMul,
        10 => BinOp::FDiv,
        11 => BinOp::FRem,
        12 => BinOp::And,
        13 => BinOp::Or,
        14 => BinOp::Xor,
        15 => BinOp::Shl,
        16 => BinOp::LShr,
        17 => BinOp::AShr,
        _ => return None,
    })
}

fn cast(sub: u64) -> Option<CastOp> {
    Some(match sub {
        0 => CastOp::CheckedInt,
        1 => CastOp::ZExt,
        2 => CastOp::SExt,
        3 => CastOp::Trunc,
        4 => CastOp::FExt,
        5 => CastOp::FTrunc,
        6 => CastOp::IToF_S,
        7 => CastOp::IToF_U,
        8 => CastOp::FToI_S,
        9 => CastOp::FToI_U,
        10 => CastOp::Bitcast,
        11 => CastOp::PtrToInt,
        12 => CastOp::IntToPtr,
        _ => return None,
    })
}

fn cmp(sub: u64) -> Option<CmpOp> {
    Some(match sub {
        0 => CmpOp::Eq,
        1 => CmpOp::Ne,
        2 => CmpOp::SLt,
        3 => CmpOp::SLe,
        4 => CmpOp::SGt,
        5 => CmpOp::SGe,
        6 => CmpOp::ULt,
        7 => CmpOp::ULe,
        8 => CmpOp::UGt,
        9 => CmpOp::UGe,
        10 => CmpOp::FEq,
        11 => CmpOp::FNe,
        12 => CmpOp::FLt,
        13 => CmpOp::FLe,
        14 => CmpOp::FGt,
        15 => CmpOp::FGe,
        _ => return None,
    })
}

fn icmp(sub: u64) -> Option<ICmpPred> {
    Some(match sub {
        0 => ICmpPred::Eq,
        1 => ICmpPred::Ne,
        2 => ICmpPred::Ult,
        3 => ICmpPred::Ule,
        4 => ICmpPred::Ugt,
        5 => ICmpPred::Uge,
        6 => ICmpPred::Slt,
        7 => ICmpPred::Sle,
        8 => ICmpPred::Sgt,
        9 => ICmpPred::Sge,
        _ => return None,
    })
}

fn fcmp(sub: u64) -> Option<FCmpPred> {
    Some(match sub {
        0 => FCmpPred::Oeq,
        1 => FCmpPred::One,
        2 => FCmpPred::Olt,
        3 => FCmpPred::Ole,
        4 => FCmpPred::Ogt,
        5 => FCmpPred::Oge,
        6 => FCmpPred::Ord,
        7 => FCmpPred::Uno,
        8 => FCmpPred::Ueq,
        9 => FCmpPred::Une,
        10 => FCmpPred::Ult,
        11 => FCmpPred::Ule,
        12 => FCmpPred::Ugt,
        13 => FCmpPred::Uge,
        _ => return None,
    })
}

fn checked(sub: u64) -> Option<CheckedOp> {
    Some(match sub {
        0 => CheckedOp::UAdd,
        1 => CheckedOp::USub,
        2 => CheckedOp::UMul,
        3 => CheckedOp::SAdd,
        4 => CheckedOp::SSub,
        5 => CheckedOp::SMul,
        _ => return None,
    })
}

struct Materialization {
    types: [Option<Type>; 3],
    literal: Option<ConstValue>,
    expression: Option<ConstExpr>,
    incomings: Vec<(ValueId, BlockId)>,
    values: Vec<ValueId>,
    cases: Vec<(ConstValue, BlockId)>,
}

impl Materialization {
    fn new() -> Self {
        Self {
            types: [None, None, None],
            literal: None,
            expression: None,
            incomings: Vec::new(),
            values: Vec::new(),
            cases: Vec::new(),
        }
    }
}

enum Body {
    Instruction(ParsedInstruction),
    Terminator(Terminator),
}

impl Parser<'_> {
    fn instruction_backend(&self) -> ParseError {
        ParseError {
            location: self.location(),
            kind: ParseErrorKind::Backend("instruction parser"),
        }
    }

    fn instruction_string(&mut self, index: u64) -> Result<String, ParseError> {
        match self.tokens.get_mut(index as usize).map(|token| &mut token.kind) {
            Some(Kind::String(value)) => Ok(std::mem::take(value)),
            _ => Err(self.instruction_backend()),
        }
    }

    fn instruction_equal(&mut self, role: u64, values: &Materialization) -> Result<(), ParseError> {
        let location = self.location();
        let invalid =
            || ParseError { location, kind: ParseErrorKind::Backend("instruction parser") };
        let annotation = || values.types[0].as_ref().ok_or_else(invalid);
        let primary = || values.types[1].as_ref().ok_or_else(invalid);
        let secondary = || values.types[2].as_ref().ok_or_else(invalid);
        match role {
            0 => self.equal_type(annotation()?, primary()?),
            1 => self.equal_type(annotation()?, &Type::Bool),
            2 => self.equal_type(annotation()?, &Type::I128),
            3 => self.equal_type(annotation()?, &Type::ptr_to(primary()?.clone())),
            4 => self.equal_type(annotation()?, &Type::Tuple(vec![primary()?.clone(), Type::Bool])),
            5 => self.equal_type(annotation()?, secondary()?),
            6 => self.equal_type(primary()?, secondary()?),
            7 => self.equal_type(&Type::ptr_to(primary()?.clone()), secondary()?),
            8 => self.equal_type(primary()?, &values.expression.as_ref().ok_or_else(invalid)?.ty),
            9 => {
                self.equal_type(annotation()?, &values.expression.as_ref().ok_or_else(invalid)?.ty)
            },
            _ => Err(invalid()),
        }
    }

    fn instruction_materialize(
        &mut self,
        event: &Event,
        mut m: Materialization,
    ) -> Result<Body, ParseError> {
        let location = self.location();
        let invalid =
            || ParseError { location, kind: ParseErrorKind::Backend("instruction parser") };
        if event.has_dst > 1 || (event.op == 29 && (event.sub > 1 || event.e > 1)) {
            return Err(invalid());
        }
        let a = u32::try_from(event.a).map_err(|_| invalid())?;
        let b = u32::try_from(event.b).map_err(|_| invalid())?;
        let c = u32::try_from(event.c).map_err(|_| invalid())?;
        let d = u32::try_from(event.d).map_err(|_| invalid())?;
        let dst = ValueId(u32::try_from(event.dst).map_err(|_| invalid())?);
        if event.op >= 31 {
            return Ok(Body::Terminator(match event.op {
                31 => Terminator::Br { target: BlockId(a) },
                32 => {
                    Terminator::CBr { cond: ValueId(a), then_bb: BlockId(b), else_bb: BlockId(c) }
                },
                33 => Terminator::Switch {
                    ty: m.types[1].take().ok_or_else(invalid)?,
                    value: ValueId(a),
                    default_bb: BlockId(b),
                    cases: m.cases,
                },
                34 => Terminator::Ret {
                    ty: m.types[1].take().ok_or_else(invalid)?,
                    value: (event.aux != 0).then_some(ValueId(a)),
                },
                35 => Terminator::Trap { reason: self.instruction_string(event.name)? },
                _ => return Err(invalid()),
            }));
        }
        let instruction = match event.op {
            0 => Instruction::GlobalAddr {
                dst,
                global: GlobalId(a),
                ty: match &m.types[0] {
                    Some(Type::Ptr(ty)) => (**ty).clone(),
                    _ => return Err(invalid()),
                },
            },
            1 => Instruction::LifetimeEnd { ptr: ValueId(a) },
            2 => Instruction::PtrDiff { dst, lhs: ValueId(a), rhs: ValueId(b) },
            3 => Instruction::ConstDecl {
                dst,
                name: self.instruction_string(event.name)?,
                expression: m.expression.take().ok_or_else(invalid)?,
                value: m.literal.take().ok_or_else(invalid)?,
            },
            4 => Instruction::Const {
                dst,
                ty: m.types[1].take().ok_or_else(invalid)?,
                value: m.literal.take().ok_or_else(invalid)?,
            },
            5 => Instruction::Undef { dst, ty: m.types[1].take().ok_or_else(invalid)? },
            6 => Instruction::Mov {
                dst,
                ty: m.types[1].take().ok_or_else(invalid)?,
                src: ValueId(a),
            },
            7 => Instruction::Bin {
                dst,
                op: bin(event.sub).ok_or_else(invalid)?,
                ty: m.types[1].take().ok_or_else(invalid)?,
                lhs: ValueId(a),
                rhs: ValueId(b),
            },
            8 => Instruction::Neg {
                dst,
                ty: m.types[1].take().ok_or_else(invalid)?,
                src: ValueId(a),
            },
            9 => Instruction::Not {
                dst,
                ty: m.types[1].take().ok_or_else(invalid)?,
                src: ValueId(a),
            },
            10 => Instruction::Cmp {
                dst,
                op: cmp(event.sub).ok_or_else(invalid)?,
                ty: m.types[1].take().ok_or_else(invalid)?,
                lhs: ValueId(a),
                rhs: ValueId(b),
            },
            11 => Instruction::ICmp {
                dst,
                pred: icmp(event.sub).ok_or_else(invalid)?,
                ty: m.types[1].take().ok_or_else(invalid)?,
                lhs: ValueId(a),
                rhs: ValueId(b),
            },
            12 => Instruction::FCmp {
                dst,
                pred: fcmp(event.sub).ok_or_else(invalid)?,
                ty: m.types[1].take().ok_or_else(invalid)?,
                lhs: ValueId(a),
                rhs: ValueId(b),
            },
            13 => Instruction::Select {
                dst,
                ty: m.types[1].take().ok_or_else(invalid)?,
                cond: ValueId(a),
                on_true: ValueId(b),
                on_false: ValueId(c),
            },
            14 => Instruction::Cast {
                dst,
                op: cast(event.sub).ok_or_else(invalid)?,
                dst_ty: m.types[2].take().ok_or_else(invalid)?,
                src_ty: m.types[1].take().ok_or_else(invalid)?,
                src: ValueId(a),
            },
            15 => Instruction::Phi {
                dst,
                ty: m.types[1].take().ok_or_else(invalid)?,
                incomings: m.incomings,
            },
            16 => Instruction::Extract {
                dst,
                dst_ty: m.types[0].as_ref().ok_or_else(invalid)?.clone(),
                tuple: ValueId(a),
                index: d,
            },
            17 => Instruction::Checked {
                dst,
                op: checked(event.sub).ok_or_else(invalid)?,
                ty: m.types[1].take().ok_or_else(invalid)?,
                lhs: ValueId(a),
                rhs: ValueId(b),
            },
            18 => Instruction::Alloca { dst, ty: m.types[1].take().ok_or_else(invalid)?, align: d },
            19 => Instruction::VolatileLoad {
                dst,
                ty: m.types[1].take().ok_or_else(invalid)?,
                ptr: ValueId(b),
                align: d,
            },
            20 => Instruction::VolatileStore {
                ty: m.types[1].take().ok_or_else(invalid)?,
                value: ValueId(a),
                ptr: ValueId(b),
                align: d,
            },
            21 => Instruction::Load {
                dst,
                ty: m.types[1].take().ok_or_else(invalid)?,
                ptr: ValueId(b),
                align: d,
            },
            22 => Instruction::Uninit {
                ty: m.types[1].take().ok_or_else(invalid)?,
                ptr: ValueId(b),
                align: d,
            },
            23 => Instruction::Store {
                ty: m.types[1].take().ok_or_else(invalid)?,
                value: ValueId(a),
                ptr: ValueId(b),
                align: d,
            },
            24 => Instruction::Gep {
                dst,
                dst_ty: m.types[0].as_ref().ok_or_else(invalid)?.clone(),
                base_ptr: ValueId(a),
                indices: m.values,
            },
            25 => Instruction::Memcpy { dst: ValueId(a), src: ValueId(b), n: ValueId(c), align: d },
            26 => Instruction::Memset { dst: ValueId(a), val: ValueId(b), n: ValueId(c), align: d },
            27 | 28 => {
                let signature = match &m.types[0] {
                    Some(Type::FnPtr(signature)) => (**signature).clone(),
                    _ => return Err(invalid()),
                };
                if event.op == 27 {
                    Instruction::NullFunction { dst, signature }
                } else {
                    Instruction::FunctionAddr { dst, function: FunctionId(a), signature }
                }
            },
            29 => Instruction::Call {
                convention: if event.sub == 0 {
                    CallingConvention::Whale
                } else {
                    CallingConvention::SysV64
                },
                dst: (event.has_dst != 0).then_some(dst),
                ret_ty: m.types[1].take().ok_or_else(invalid)?,
                callee: if event.e == 0 {
                    Callee::Direct(FunctionId(a))
                } else {
                    Callee::Indirect(ValueId(a))
                },
                args: m.values,
            },
            30 => Instruction::TrapIf {
                cond: ValueId(a),
                reason: self.instruction_string(event.name)?,
            },
            _ => return Err(invalid()),
        };
        let result = if event.has_dst == 0 {
            None
        } else {
            Some((dst, m.types[0].take().ok_or_else(invalid)?))
        };
        Ok(Body::Instruction((instruction, result)))
    }

    fn instruction_body_wave(&mut self, terminator: bool) -> Result<Body, ParseError> {
        let mut state = State {
            at: self.at as u64,
            format: self.format_version.into(),
            mode: u64::from(terminator),
            ..State::default()
        };
        let mut m = Materialization::new();
        loop {
            state.at = self.at as u64;
            let nodes = self.limits.max_nodes.saturating_sub(self.nodes);
            let work = self.limits.max_analysis_work.saturating_sub(self.grammar_work);
            let mut event = Event::default();
            let status = unsafe {
                whale_ir_instruction_step(
                    self.raw_tokens.as_ptr(),
                    self.raw_tokens.len() as u64,
                    &mut state,
                    nodes as u64,
                    work as u64,
                    &mut event,
                )
            };
            if status != event.status as i32
                || event.nodes > nodes as u64
                || event.work > work as u64
                || event.next >= self.tokens.len() as u64
            {
                return Err(self.instruction_backend());
            }
            if status != 0 {
                return Err(error(&self.tokens, &event, self.limits));
            }
            self.at = event.next as usize;
            self.nodes += event.nodes as usize;
            self.grammar_work += event.work as usize;
            match event.event {
                100 if event.role <= 2 => {
                    let ty = self.ty(0)?;
                    state.type_kind = type_kind(&ty);
                    m.types[event.role as usize] = Some(ty);
                },
                101 => {
                    m.literal =
                        Some(self.literal(
                            m.types[1].as_ref().ok_or_else(|| self.instruction_backend())?,
                        )?)
                },
                102 => m.expression = Some(self.expression(0)?),
                110 => self.instruction_equal(event.role, &m)?,
                200 => m.incomings.push((
                    ValueId(u32::try_from(event.a).map_err(|_| self.instruction_backend())?),
                    BlockId(u32::try_from(event.b).map_err(|_| self.instruction_backend())?),
                )),
                201 => m
                    .values
                    .push(ValueId(u32::try_from(event.b).map_err(|_| self.instruction_backend())?)),
                202 => m.cases.push((
                    m.literal.take().ok_or_else(|| self.instruction_backend())?,
                    BlockId(u32::try_from(event.c).map_err(|_| self.instruction_backend())?),
                )),
                1 => return self.instruction_materialize(&event, m),
                _ => return Err(self.instruction_backend()),
            }
        }
    }

    pub(super) fn instruction_wave(&mut self) -> Result<ParsedInstruction, ParseError> {
        match self.instruction_body_wave(false)? {
            Body::Instruction(result) => Ok(result),
            Body::Terminator(_) => Err(self.instruction_backend()),
        }
    }

    pub(super) fn terminator_wave(&mut self) -> Result<Terminator, ParseError> {
        match self.instruction_body_wave(true)? {
            Body::Terminator(result) => Ok(result),
            Body::Instruction(_) => Err(self.instruction_backend()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parser::lexer;

    fn parse_case(
        text: &str,
        format: u32,
        terminator: bool,
        reference: bool,
        limits: IrLimits,
    ) -> Result<(String, String, usize), ParseError> {
        let tokens =
            if reference { lexer::lex_reference(text, limits)? } else { lexer::lex(text, limits)? };
        let mut parser = Parser::new(tokens, limits);
        parser.reference_types = reference;
        parser.format_version = format;
        let parsed = if terminator {
            format!("{:?}", parser.terminator()?)
        } else {
            format!("{:?}", parser.instruction()?)
        };
        Ok((parsed, parser.peek().into(), parser.nodes))
    }

    fn compare(text: &str, format: u32, terminator: bool, limits: IrLimits) {
        match (parse_case(text, format, terminator, false, limits),
            parse_case(text, format, terminator, true, limits)) {
            (Ok(actual), Ok(expected)) => assert_eq!(actual, expected, "format{format} {text:?}"),
            (Err(actual), Err(expected)) => {
                assert_eq!(actual.location, expected.location, "format{format} {text:?}");
                assert_eq!(actual.to_string(), expected.to_string(), "format{format} {text:?}");
            },
            (actual, expected) => panic!("Wave/reference instruction divergence format{format} {text:?}: {actual:?} / {expected:?}"),
        }
    }

    fn seeds() -> Vec<String> {
        let mut cases = vec![
            "%v3: ptr<i32> = global_addr @g9",
            "lifetime_end %v7",
            "%v4: i128 = ptrdiff %v1, %v2",
            "%v5: i32 = const_decl \"named λ\" add(i32 1, i32 2) => const i32 3",
            "%v2: i32 = const i32 -7",
            "%v2: i32 = undef i32",
            "%v2: i32 = mov i32 %v1",
            "%v2: i32 = neg i32 %v1",
            "%v2: i32 = not i32 %v1",
            "%v2: i32 = select bool %v1, i32 %v3, i32 %v4",
            "%v2: i32 = phi i32",
            "%v2: i32 = phi i32 [%v1, %b0], [%v3, %b2]",
            "%v4: bool = extract %v2, 1",
            "%v2: ptr<i32> = alloca i32, align 4",
            "%v2: i32 = load i32, ptr<i32> %v1, align 4",
            "store i32 %v1, ptr<i32> %v2, align 4",
            "%v2: i32 = volatile_load i32, ptr<i32> %v1, align 4",
            "volatile_store i32 %v1, ptr<i32> %v2, align 4",
            "uninit i32, ptr<i32> %v1, align 4",
            "%v2: ptr<i8> = gep %v1, %v3, %v4",
            "memcpy ptr<u8> %v1, ptr<u8> %v2, u64 %v3, align 1",
            "memset ptr<u8> %v1, u8 %v2, u64 %v3, align 1",
            "%v1: fnptr<whale (i32) -> void> = null_function",
            "%v1: fnptr<whale (i32) -> void> = function_addr @f9",
            "%v2: i32 = call whale i32 @f7(%v1, %v3)",
            "call sysv64 void indirect %v1()",
            "trap_if bool %v1, reason = \"trap\\nmessage\"",
            "%v4294967295: i32 = const i32 0",
            "%v2: f32 = const f32 0x7fa00001",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        for op in
            "add sub mul udiv sdiv urem srem fadd fsub fmul fdiv frem and or xor shl lshr ashr"
                .split_whitespace()
        {
            for ty in ["i1", "u1", "i32", "u128", "f16", "f64", "ptr<i32>"] {
                cases.push(format!("%v2: {ty} = {op} {ty} %v0, %v1"));
            }
        }
        for (op, predicates) in [
            ("cmp", "eq ne slt sle sgt sge ult ule ugt uge feq fne flt fle fgt fge"),
            ("icmp", "eq ne ult ule ugt uge slt sle sgt sge"),
            ("fcmp", "oeq one olt ole ogt oge ord uno ueq une ult ule ugt uge"),
        ] {
            for pred in predicates.split_whitespace() {
                cases.push(format!("%v2: bool = {op} {pred} i32 %v0, %v1"));
                cases.push(format!("%v2: bool = {op} {pred} ptr<i32> %v0, %v1"));
            }
        }
        for op in "checked_int zext sext trunc fext ftrunc itof_s itof_u ftoi_s ftoi_u bitcast ptrtoint inttoptr".split_whitespace() {
            cases.push(format!("%v2: i64 = {op} i32 %v0 to i64"));
        }
        for op in "uadd_chk usub_chk umul_chk sadd_chk ssub_chk smul_chk".split_whitespace() {
            cases.push(format!("%v2: tuple<i32, bool> = {op} i32 %v0, %v1"));
        }
        cases
    }

    #[test]
    fn complete_inventory_and_version_gates_match_reference() {
        for text in seeds() {
            for format in 3..=8 {
                compare(&text, format, false, IrLimits::default());
            }
        }
        for text in [
            "br label %b0",
            "cbr bool %v1, label %b0, label %b3",
            "switch i32 %v1, label %b0 []",
            "switch i32 %v1, label %b0 [1: %b2, -2: %b3]",
            "ret void",
            "ret i32 %v1",
            "trap reason = \"reason\"",
        ] {
            for format in 3..=8 {
                compare(text, format, true, IrLimits::default());
            }
        }
    }

    #[test]
    fn malformed_tokens_and_type_annotations_match_exact_diagnostics() {
        for text in [
            "",
            "unknown %v1",
            "%v0",
            "%v0:",
            "%v: i32 = const i32 0",
            "%v4294967296: i32 = const i32 0",
            "const i32 1",
            "load i32, ptr<i32> %v1, align 4",
            "%v2: i64 = const i32 2",
            "%v2: i32 = store i32 %v0, ptr<i32> %v1, align 4",
            "%v2: i32 = global_addr @g0",
            "%v2: i32 = function_addr @f0",
            "%v2: i32 = phi i32 [%v0, %b1],",
            "%v2: ptr<i32> = gep %v0,",
            "%v2: bool = cmp unknown i32 %v0, %v1",
            "%v2: bool = icmp unknown i32 %v0, %v1",
            "%v2: bool = fcmp unknown i32 %v0, %v1",
            "%v2: bool = cmp unknown ptr<i32> %v0, %v1",
            "%v2: i32 = select bool %v0, i32 %v1, u32 %v2",
            "%v2: i32 = load i32, ptr<u32> %v0, align 4",
            "%v2: i32 = call mystery i32 @f0()",
            "call whale i32 @f0()",
            "%v2: i32 = call whale void @f0()",
            "call whale void @f0(%v1,)",
            "memcpy ptr<u8> %v0, ptr<u8> %v1, u64 %v2, align 4294967296",
        ] {
            for format in [3, 5, 6, 8] {
                compare(text, format, false, IrLimits::default());
            }
        }
        for text in [
            "",
            "unknown",
            "ret",
            "ret i32",
            "cbr bool %v0, label %b1, label %b",
            "switch i32 %v0, label %b0 [1: %b1,]",
            "switch i32 %v0, label %b0 [true: %b1]",
            "trap reason = not_a_string",
        ] {
            compare(text, 8, true, IrLimits::default());
        }
    }

    #[test]
    fn bounded_list_nodes_and_mutations_match_reference() {
        let mut random = 0x41b823dd71_u64;
        for text in seeds() {
            if !text.is_ascii() {
                continue;
            }
            for _ in 0..24 {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                let mut bytes = text.as_bytes().to_vec();
                let index = random as usize % bytes.len();
                if random & 1 == 0 {
                    bytes.truncate(index);
                } else {
                    bytes[index] = b"[]{} =,:\"%v@f0ab"[(random >> 17) as usize % 16];
                }
                compare(std::str::from_utf8(&bytes).unwrap(), 8, false, IrLimits::default());
            }
        }
        let text = format!(
            "%v2: i32 = phi i32 {}",
            (0..200).map(|i| format!("[%v{i}, %b{i}]")).collect::<Vec<_>>().join(", ")
        );
        for nodes in [0, 1, 2, 50, 202, 203, 1000] {
            compare(&text, 8, false, IrLimits { max_nodes: nodes, ..IrLimits::default() });
        }
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    use crate::parser::lexer;

    #[test]
    fn transport_and_unknown_protocol_statuses_are_backend_faults() {
        let tokens = lexer::lex("", IrLimits::default()).unwrap();
        for (status, reason) in [(6, 0), (1, 99), (8, 0)] {
            let event = Event { status, reason, ..Event::default() };
            assert!(matches!(
                error(&tokens, &event, IrLimits::default()).kind,
                ParseErrorKind::Backend("instruction parser")
            ));
        }
    }

    #[test]
    fn instruction_grammar_applies_cumulative_work_limits() {
        let limits = IrLimits { max_analysis_work: 0, ..IrLimits::default() };
        let tokens = lexer::lex("lifetime_end %v0", limits).unwrap();
        let mut parser = Parser::new(tokens, limits);
        parser.format_version = 8;
        assert!(matches!(
            parser.instruction().unwrap_err().kind,
            ParseErrorKind::ResourceLimit(LimitError {
                resource: Resource::AnalysisWork,
                limit: 0
            })
        ));
    }
}
