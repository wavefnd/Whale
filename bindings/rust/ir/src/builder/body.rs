// SPDX-License-Identifier: MPL-2.0
//! Complete typed body transport for the authoritative Wave builder model.
use super::BuilderError;

use crate::*;

pub(super) fn discard_expression(root: ConstExpr) {
    let mut pending = vec![root];
    while let Some(expression) = pending.pop() {
        crate::limits::discard_type(expression.ty);
        match expression.kind {
            ConstExprKind::Not(child) | ConstExprKind::Neg(child) => pending.push(*child),
            ConstExprKind::Binary { left, right, .. }
            | ConstExprKind::Compare { left, right, .. } => {
                pending.push(*left);
                pending.push(*right);
            },
            _ => {},
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Atom {
    kind: u64,
    a: u64,
    b: u64,
    c: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Record {
    op: u64,
    block: u64,
    first: u64,
    count: u64,
    byte_first: u64,
    byte_count: u64,
    next: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Block {
    head: u64,
    tail: u64,
    count: u64,
    terminator: u64,
    prefix_tail: u64,
    prefix_count: u64,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            head: u64::MAX,
            tail: u64::MAX,
            count: 0,
            terminator: u64::MAX,
            prefix_tail: u64::MAX,
            prefix_count: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct State {
    atoms: u64,
    bytes: u64,
    records: u64,
    globals: u64,
    work: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GlobalRecord {
    id: u64,
    first: u64,
    count: u64,
    byte_first: u64,
    byte_count: u64,
}

#[repr(C)]
#[derive(Default)]
struct RawResult {
    status: u64,
    detail: u64,
    ordinal: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_builder_global(
        state: *mut State,
        id: u64,
        globals: *mut GlobalRecord,
        global_capacity: u64,
        atoms: *const Atom,
        atom_count: u64,
        bytes: *const u8,
        byte_count: u64,
        depth: u64,
        const_depth: u64,
        node_limit: u64,
        byte_limit: u64,
        work_limit: u64,
        mode: u64,
        output_atoms: *mut Atom,
        atom_capacity: u64,
        output_bytes: *mut u8,
        byte_capacity: u64,
        result: *mut RawResult,
    ) -> i32;

    fn whale_ir_builder_body(
        state: *mut State,
        blocks: *mut Block,
        block_count: u64,
        block: u64,
        op: u64,
        hoist: u64,
        atoms: *const Atom,
        atom_count: u64,
        bytes: *const u8,
        byte_count: u64,
        depth: u64,
        const_depth: u64,
        node_limit: u64,
        byte_limit: u64,
        work_limit: u64,
        mode: u64,
        output_atoms: *mut Atom,
        atom_capacity: u64,
        output_bytes: *mut u8,
        byte_capacity: u64,
        records: *mut Record,
        record_capacity: u64,
        result: *mut RawResult,
    ) -> i32;
}

#[derive(Default)]
struct Input {
    op: u64,
    atoms: Vec<Atom>,
    bytes: Vec<u8>,
    count_only: bool,
    atom_count: usize,
    byte_count: usize,
}

impl Input {
    fn atom(&mut self, atom: Atom) {
        self.atom_count = self.atom_count.saturating_add(1);
        if !self.count_only {
            self.atoms.push(atom);
        }
    }

    fn uint(&mut self, value: u64) {
        self.atom(Atom { kind: 0, a: value, b: 0, c: 0 });
    }

    fn id(&mut self, value: ValueId) {
        self.uint(value.0.into());
    }

    fn constant(&mut self, value: &ConstValue) {
        let (a, bits) = match value {
            ConstValue::Bool(value) => (0, u128::from(*value)),
            ConstValue::I(value) => (1, *value as u128),
            ConstValue::U(value) => (2, *value),
            ConstValue::F(FloatBits::F16(value)) => (3, u128::from(*value)),
            ConstValue::F(FloatBits::F32(value)) => (4, u128::from(*value)),
            ConstValue::F(FloatBits::F64(value)) => (5, u128::from(*value)),
        };
        self.atom(Atom { kind: 1, a, b: bits as u64, c: (bits >> 64) as u64 });
    }

    fn string(&mut self, value: &str) {
        self.atom(Atom { kind: 2, a: self.byte_count as u64, b: value.len() as u64, c: 0 });
        self.byte_count = self.byte_count.saturating_add(value.len());
        if !self.count_only {
            self.bytes.extend_from_slice(value.as_bytes());
        }
    }

    fn ty(&mut self, ty: &Type) {
        let (kind, children, auxiliary) = match ty {
            Type::Void => (0, 0, 0),
            Type::Bool => (1, 0, 0),
            Type::I1 => (2, 0, 0),
            Type::U1 => (3, 0, 0),
            Type::I8 => (4, 0, 0),
            Type::I16 => (5, 0, 0),
            Type::I32 => (6, 0, 0),
            Type::I64 => (7, 0, 0),
            Type::I128 => (8, 0, 0),
            Type::U8 => (9, 0, 0),
            Type::U16 => (10, 0, 0),
            Type::U32 => (11, 0, 0),
            Type::U64 => (12, 0, 0),
            Type::U128 => (13, 0, 0),
            Type::F16 => (14, 0, 0),
            Type::F32 => (15, 0, 0),
            Type::F64 => (16, 0, 0),
            Type::Ptr(_) => (17, 1, 0),
            Type::FnPtr(signature) => (
                18,
                signature.params.len() + 1,
                u64::from(signature.variadic) * 2 + convention(&signature.convention),
            ),
            Type::Array(_, count) => (19, 1, *count),
            Type::Struct(fields) => (20, fields.len(), 0),
            Type::Tuple(fields) => (21, fields.len(), 0),
        };
        self.atom(Atom { kind: 3, a: kind, b: children as u64, c: auxiliary });
        match ty {
            Type::Ptr(child) | Type::Array(child, _) => self.ty(child),
            Type::FnPtr(signature) => {
                self.ty(&signature.ret);
                for ty in &signature.params {
                    self.ty(ty);
                }
            },
            Type::Struct(fields) | Type::Tuple(fields) => {
                for ty in fields {
                    self.ty(ty);
                }
            },
            _ => {},
        }
    }

    fn expression(&mut self, expression: &ConstExpr) {
        let (kind, op, children) = match &expression.kind {
            ConstExprKind::Literal(_) => (0, 0, 0),
            ConstExprKind::Reference(ConstRef::Global(_)) => (1, 0, 0),
            ConstExprKind::Reference(ConstRef::Local(_)) => (2, 0, 0),
            ConstExprKind::Not(_) => (3, 0, 1),
            ConstExprKind::Neg(_) => (4, 0, 1),
            ConstExprKind::Binary { op, .. } => (5, const_binary(op), 2),
            ConstExprKind::Compare { op, .. } => (6, const_compare(op), 2),
        };
        self.atom(Atom { kind: 4, a: kind, b: op, c: children });
        self.ty(&expression.ty);
        match &expression.kind {
            ConstExprKind::Literal(value) => self.constant(value),
            ConstExprKind::Reference(ConstRef::Global(id)) => self.uint(id.0.into()),
            ConstExprKind::Reference(ConstRef::Local(id)) => self.id(*id),
            ConstExprKind::Not(child) | ConstExprKind::Neg(child) => self.expression(child),
            ConstExprKind::Binary { left, right, .. }
            | ConstExprKind::Compare { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            },
        }
    }

    fn list(&mut self, kind: u64, count: usize) {
        self.atom(Atom { kind: 5, a: kind, b: count as u64, c: 0 });
    }

    fn optional(&mut self, value: Option<ValueId>) {
        self.uint(u64::from(value.is_some()));
        if let Some(value) = value {
            self.id(value);
        }
    }

    fn global(
        name: &str,
        expression: &ConstExpr,
        init: &ConstValue,
        align: u32,
        count_only: bool,
    ) -> Self {
        let mut input = Self { count_only, ..Self::default() };
        input.string(name);
        input.expression(expression);
        input.constant(init);
        input.uint(align.into());
        input
    }

    fn instruction(instruction: &Instruction, count_only: bool) -> Self {
        let mut input = Self { count_only, ..Self::default() };
        input.op = match instruction {
            Instruction::GlobalAddr { dst, global, ty } => {
                input.id(*dst);
                input.uint(global.0.into());
                input.ty(ty);
                0
            },
            Instruction::LifetimeEnd { ptr } => {
                input.id(*ptr);
                1
            },
            Instruction::PtrDiff { dst, lhs, rhs } => {
                input.id(*dst);
                input.id(*lhs);
                input.id(*rhs);
                2
            },
            Instruction::ConstDecl { dst, name, expression, value } => {
                input.id(*dst);
                input.string(name);
                input.expression(expression);
                input.constant(value);
                3
            },
            Instruction::Const { dst, ty, value } => {
                input.id(*dst);
                input.ty(ty);
                input.constant(value);
                4
            },
            Instruction::Undef { dst, ty } => {
                input.id(*dst);
                input.ty(ty);
                5
            },
            Instruction::Mov { dst, ty, src } => {
                input.id(*dst);
                input.ty(ty);
                input.id(*src);
                6
            },
            Instruction::Bin { dst, op, ty, lhs, rhs } => {
                input.id(*dst);
                input.uint(bin(op));
                input.ty(ty);
                input.id(*lhs);
                input.id(*rhs);
                7
            },
            Instruction::Neg { dst, ty, src } => {
                input.id(*dst);
                input.ty(ty);
                input.id(*src);
                8
            },
            Instruction::Not { dst, ty, src } => {
                input.id(*dst);
                input.ty(ty);
                input.id(*src);
                9
            },
            Instruction::Cmp { dst, op, ty, lhs, rhs } => {
                input.id(*dst);
                input.uint(cmp(op));
                input.ty(ty);
                input.id(*lhs);
                input.id(*rhs);
                10
            },
            Instruction::ICmp { dst, pred, ty, lhs, rhs } => {
                input.id(*dst);
                input.uint(icmp(pred));
                input.ty(ty);
                input.id(*lhs);
                input.id(*rhs);
                11
            },
            Instruction::FCmp { dst, pred, ty, lhs, rhs } => {
                input.id(*dst);
                input.uint(fcmp(pred));
                input.ty(ty);
                input.id(*lhs);
                input.id(*rhs);
                12
            },
            Instruction::Select { dst, ty, cond, on_true, on_false } => {
                input.id(*dst);
                input.ty(ty);
                input.id(*cond);
                input.id(*on_true);
                input.id(*on_false);
                13
            },
            Instruction::Cast { dst, op, src_ty, dst_ty, src } => {
                input.id(*dst);
                input.uint(cast(op));
                input.ty(src_ty);
                input.ty(dst_ty);
                input.id(*src);
                14
            },
            Instruction::Phi { dst, ty, incomings } => {
                input.id(*dst);
                input.ty(ty);
                input.list(1, incomings.len());
                for (value, block) in incomings {
                    input.id(*value);
                    input.uint(block.0.into());
                }
                15
            },
            Instruction::Extract { dst, dst_ty, tuple, index } => {
                input.id(*dst);
                input.ty(dst_ty);
                input.id(*tuple);
                input.uint((*index).into());
                16
            },
            Instruction::Checked { dst, op, ty, lhs, rhs } => {
                input.id(*dst);
                input.uint(checked(op));
                input.ty(ty);
                input.id(*lhs);
                input.id(*rhs);
                17
            },
            Instruction::Alloca { dst, ty, align } => {
                input.id(*dst);
                input.ty(ty);
                input.uint((*align).into());
                18
            },
            Instruction::VolatileLoad { dst, ty, ptr, align } => {
                input.id(*dst);
                input.ty(ty);
                input.id(*ptr);
                input.uint((*align).into());
                19
            },
            Instruction::VolatileStore { ty, value, ptr, align } => {
                input.ty(ty);
                input.id(*value);
                input.id(*ptr);
                input.uint((*align).into());
                20
            },
            Instruction::Load { dst, ty, ptr, align } => {
                input.id(*dst);
                input.ty(ty);
                input.id(*ptr);
                input.uint((*align).into());
                21
            },
            Instruction::Uninit { ty, ptr, align } => {
                input.ty(ty);
                input.id(*ptr);
                input.uint((*align).into());
                22
            },
            Instruction::Store { ty, value, ptr, align } => {
                input.ty(ty);
                input.id(*value);
                input.id(*ptr);
                input.uint((*align).into());
                23
            },
            Instruction::Gep { dst, dst_ty, base_ptr, indices } => {
                input.id(*dst);
                input.ty(dst_ty);
                input.id(*base_ptr);
                input.list(0, indices.len());
                for value in indices {
                    input.id(*value);
                }
                24
            },
            Instruction::Memcpy { dst, src, n, align } => {
                input.id(*dst);
                input.id(*src);
                input.id(*n);
                input.uint((*align).into());
                25
            },
            Instruction::Memset { dst, val, n, align } => {
                input.id(*dst);
                input.id(*val);
                input.id(*n);
                input.uint((*align).into());
                26
            },
            Instruction::NullFunction { dst, signature } => {
                input.id(*dst);
                input.ty(&Type::FnPtr(Box::new(signature.clone())));
                27
            },
            Instruction::FunctionAddr { dst, function, signature } => {
                input.id(*dst);
                input.uint(function.0.into());
                input.ty(&Type::FnPtr(Box::new(signature.clone())));
                28
            },
            Instruction::Call { dst, ret_ty, convention: cc, callee, args } => {
                input.optional(*dst);
                input.ty(ret_ty);
                input.uint(convention(cc));
                match callee {
                    Callee::Direct(id) => {
                        input.uint(0);
                        input.uint(id.0.into());
                    },
                    Callee::Indirect(value) => {
                        input.uint(1);
                        input.id(*value);
                    },
                }
                input.list(0, args.len());
                for value in args {
                    input.id(*value);
                }
                29
            },
            Instruction::TrapIf { cond, reason } => {
                input.id(*cond);
                input.string(reason);
                30
            },
        };
        input
    }

    fn terminator(term: &Terminator, count_only: bool) -> Self {
        let mut input = Self { count_only, ..Self::default() };
        input.op = match term {
            Terminator::Br { target } => {
                input.uint(target.0.into());
                31
            },
            Terminator::CBr { cond, then_bb, else_bb } => {
                input.id(*cond);
                input.uint(then_bb.0.into());
                input.uint(else_bb.0.into());
                32
            },
            Terminator::Switch { ty, value, default_bb, cases } => {
                input.ty(ty);
                input.id(*value);
                input.uint(default_bb.0.into());
                input.list(2, cases.len());
                for (value, block) in cases {
                    input.constant(value);
                    input.uint(block.0.into());
                }
                33
            },
            Terminator::Ret { ty, value } => {
                input.ty(ty);
                input.optional(*value);
                34
            },
            Terminator::Trap { reason } => {
                input.string(reason);
                35
            },
        };
        input
    }
}

fn bin(value: &BinOp) -> u64 {
    match value {
        BinOp::Add => 0,
        BinOp::Sub => 1,
        BinOp::Mul => 2,
        BinOp::UDiv => 3,
        BinOp::SDiv => 4,
        BinOp::URem => 5,
        BinOp::SRem => 6,
        BinOp::FAdd => 7,
        BinOp::FSub => 8,
        BinOp::FMul => 9,
        BinOp::FDiv => 10,
        BinOp::FRem => 11,
        BinOp::And => 12,
        BinOp::Or => 13,
        BinOp::Xor => 14,
        BinOp::Shl => 15,
        BinOp::LShr => 16,
        BinOp::AShr => 17,
    }
}

fn decode_bin(value: u64) -> Option<BinOp> {
    Some(match value {
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

fn cmp(value: &CmpOp) -> u64 {
    match value {
        CmpOp::Eq => 0,
        CmpOp::Ne => 1,
        CmpOp::SLt => 2,
        CmpOp::SLe => 3,
        CmpOp::SGt => 4,
        CmpOp::SGe => 5,
        CmpOp::ULt => 6,
        CmpOp::ULe => 7,
        CmpOp::UGt => 8,
        CmpOp::UGe => 9,
        CmpOp::FEq => 10,
        CmpOp::FNe => 11,
        CmpOp::FLt => 12,
        CmpOp::FLe => 13,
        CmpOp::FGt => 14,
        CmpOp::FGe => 15,
    }
}

fn decode_cmp(value: u64) -> Option<CmpOp> {
    Some(match value {
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

fn icmp(value: &ICmpPred) -> u64 {
    match value {
        ICmpPred::Eq => 0,
        ICmpPred::Ne => 1,
        ICmpPred::Ult => 2,
        ICmpPred::Ule => 3,
        ICmpPred::Ugt => 4,
        ICmpPred::Uge => 5,
        ICmpPred::Slt => 6,
        ICmpPred::Sle => 7,
        ICmpPred::Sgt => 8,
        ICmpPred::Sge => 9,
    }
}

fn decode_icmp(value: u64) -> Option<ICmpPred> {
    Some(match value {
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

fn fcmp(value: &FCmpPred) -> u64 {
    match value {
        FCmpPred::Oeq => 0,
        FCmpPred::One => 1,
        FCmpPred::Olt => 2,
        FCmpPred::Ole => 3,
        FCmpPred::Ogt => 4,
        FCmpPred::Oge => 5,
        FCmpPred::Ord => 6,
        FCmpPred::Uno => 7,
        FCmpPred::Ueq => 8,
        FCmpPred::Une => 9,
        FCmpPred::Ult => 10,
        FCmpPred::Ule => 11,
        FCmpPred::Ugt => 12,
        FCmpPred::Uge => 13,
    }
}

fn decode_fcmp(value: u64) -> Option<FCmpPred> {
    Some(match value {
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

fn cast(value: &CastOp) -> u64 {
    match value {
        CastOp::CheckedInt => 0,
        CastOp::ZExt => 1,
        CastOp::SExt => 2,
        CastOp::Trunc => 3,
        CastOp::FExt => 4,
        CastOp::FTrunc => 5,
        CastOp::IToF_S => 6,
        CastOp::IToF_U => 7,
        CastOp::FToI_S => 8,
        CastOp::FToI_U => 9,
        CastOp::Bitcast => 10,
        CastOp::PtrToInt => 11,
        CastOp::IntToPtr => 12,
    }
}

fn decode_cast(value: u64) -> Option<CastOp> {
    Some(match value {
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

fn checked(value: &CheckedOp) -> u64 {
    match value {
        CheckedOp::UAdd => 0,
        CheckedOp::USub => 1,
        CheckedOp::UMul => 2,
        CheckedOp::SAdd => 3,
        CheckedOp::SSub => 4,
        CheckedOp::SMul => 5,
    }
}

fn decode_checked(value: u64) -> Option<CheckedOp> {
    Some(match value {
        0 => CheckedOp::UAdd,
        1 => CheckedOp::USub,
        2 => CheckedOp::UMul,
        3 => CheckedOp::SAdd,
        4 => CheckedOp::SSub,
        5 => CheckedOp::SMul,
        _ => return None,
    })
}

fn const_binary(value: &ConstBinaryOp) -> u64 {
    match value {
        ConstBinaryOp::Add => 0,
        ConstBinaryOp::Sub => 1,
        ConstBinaryOp::Mul => 2,
        ConstBinaryOp::BitAnd => 3,
        ConstBinaryOp::BitOr => 4,
        ConstBinaryOp::BitXor => 5,
        ConstBinaryOp::Div => 6,
        ConstBinaryOp::Rem => 7,
        ConstBinaryOp::ShiftLeft => 8,
        ConstBinaryOp::ShiftRight => 9,
        ConstBinaryOp::LogicalAnd => 10,
        ConstBinaryOp::LogicalOr => 11,
    }
}

fn decode_const_binary(value: u64) -> Option<ConstBinaryOp> {
    Some(match value {
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
    })
}

fn const_compare(value: &ConstCompareOp) -> u64 {
    match value {
        ConstCompareOp::Eq => 0,
        ConstCompareOp::Ne => 1,
        ConstCompareOp::Lt => 2,
        ConstCompareOp::Le => 3,
        ConstCompareOp::Gt => 4,
        ConstCompareOp::Ge => 5,
    }
}

fn decode_const_compare(value: u64) -> Option<ConstCompareOp> {
    Some(match value {
        0 => ConstCompareOp::Eq,
        1 => ConstCompareOp::Ne,
        2 => ConstCompareOp::Lt,
        3 => ConstCompareOp::Le,
        4 => ConstCompareOp::Gt,
        5 => ConstCompareOp::Ge,
        _ => return None,
    })
}

fn convention(value: &CallingConvention) -> u64 {
    match value {
        CallingConvention::Whale => 0,
        CallingConvention::SysV64 => 1,
    }
}

fn decode_convention(value: u64) -> Option<CallingConvention> {
    Some(match value {
        0 => CallingConvention::Whale,
        1 => CallingConvention::SysV64,
        _ => return None,
    })
}

struct Reader<'a> {
    atoms: &'a [Atom],
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn atom(&mut self, kind: u64) -> Option<Atom> {
        let atom = *self.atoms.get(self.at)?;
        if atom.kind != kind {
            return None;
        }
        self.at += 1;
        Some(atom)
    }

    fn uint(&mut self) -> Option<u64> {
        Some(self.atom(0)?.a)
    }

    fn id(&mut self) -> Option<ValueId> {
        Some(ValueId(self.uint()?.try_into().ok()?))
    }

    fn block(&mut self) -> Option<BlockId> {
        Some(BlockId(self.uint()?.try_into().ok()?))
    }

    fn constant(&mut self) -> Option<ConstValue> {
        let a = self.atom(1)?;
        let bits = u128::from(a.b) | (u128::from(a.c) << 64);
        Some(match a.a {
            0 => ConstValue::Bool(a.b != 0),
            1 => ConstValue::I(bits as i128),
            2 => ConstValue::U(bits),
            3 => ConstValue::F(FloatBits::F16(a.b.try_into().ok()?)),
            4 => ConstValue::F(FloatBits::F32(a.b.try_into().ok()?)),
            5 => ConstValue::F(FloatBits::F64(a.b)),
            _ => return None,
        })
    }

    fn string(&mut self) -> Option<String> {
        let a = self.atom(2)?;
        let first = usize::try_from(a.a).ok()?;
        let end = first.checked_add(usize::try_from(a.b).ok()?)?;
        Some(std::str::from_utf8(self.bytes.get(first..end)?).ok()?.into())
    }

    fn ty(&mut self) -> Option<Type> {
        let a = self.atom(3)?;
        Some(match a.a {
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
            17 => Type::ptr_to(self.ty()?),
            19 => Type::Array(Box::new(self.ty()?), a.c),
            18 => {
                let ret = self.ty()?;
                let mut params = Vec::new();
                for _ in 1..a.b {
                    params.push(self.ty()?);
                }
                Type::FnPtr(Box::new(FunctionSignature {
                    params,
                    ret,
                    convention: decode_convention(a.c & 1)?,
                    variadic: a.c & 2 != 0,
                }))
            },
            20 | 21 => {
                let mut fields = Vec::new();
                for _ in 0..a.b {
                    fields.push(self.ty()?);
                }
                if a.a == 20 {
                    Type::Struct(fields)
                } else {
                    Type::Tuple(fields)
                }
            },
            _ => return None,
        })
    }

    fn signature(&mut self) -> Option<FunctionSignature> {
        let Type::FnPtr(signature) = self.ty()? else { return None };
        Some(*signature)
    }

    fn expression(&mut self) -> Option<ConstExpr> {
        let a = self.atom(4)?;
        let ty = self.ty()?;
        let kind = match a.a {
            0 => ConstExprKind::Literal(self.constant()?),
            1 => {
                ConstExprKind::Reference(ConstRef::Global(GlobalId(self.uint()?.try_into().ok()?)))
            },
            2 => ConstExprKind::Reference(ConstRef::Local(self.id()?)),
            3 => ConstExprKind::Not(Box::new(self.expression()?)),
            4 => ConstExprKind::Neg(Box::new(self.expression()?)),
            5 => ConstExprKind::Binary {
                op: decode_const_binary(a.b)?,
                left: Box::new(self.expression()?),
                right: Box::new(self.expression()?),
            },
            6 => ConstExprKind::Compare {
                op: decode_const_compare(a.b)?,
                left: Box::new(self.expression()?),
                right: Box::new(self.expression()?),
            },
            _ => return None,
        };
        Some(ConstExpr { ty, kind })
    }

    fn list(&mut self, kind: u64) -> Option<usize> {
        let a = self.atom(5)?;
        if a.a != kind {
            return None;
        }
        a.b.try_into().ok()
    }

    fn values(&mut self) -> Option<Vec<ValueId>> {
        let count = self.list(0)?;
        (0..count).map(|_| self.id()).collect()
    }

    fn phi(&mut self) -> Option<Vec<(ValueId, BlockId)>> {
        let count = self.list(1)?;
        (0..count).map(|_| Some((self.id()?, self.block()?))).collect()
    }

    fn cases(&mut self) -> Option<Vec<(ConstValue, BlockId)>> {
        let count = self.list(2)?;
        (0..count).map(|_| Some((self.constant()?, self.block()?))).collect()
    }

    fn optional(&mut self) -> Option<Option<ValueId>> {
        Some(if self.uint()? == 0 { None } else { Some(self.id()?) })
    }

    fn instruction(&mut self, op: u64) -> Option<Instruction> {
        Some(match op {
            0 => Instruction::GlobalAddr {
                dst: self.id()?,
                global: GlobalId(self.uint()?.try_into().ok()?),
                ty: self.ty()?,
            },
            1 => Instruction::LifetimeEnd { ptr: self.id()? },
            2 => Instruction::PtrDiff { dst: self.id()?, lhs: self.id()?, rhs: self.id()? },
            3 => Instruction::ConstDecl {
                dst: self.id()?,
                name: self.string()?,
                expression: self.expression()?,
                value: self.constant()?,
            },
            4 => Instruction::Const { dst: self.id()?, ty: self.ty()?, value: self.constant()? },
            5 => Instruction::Undef { dst: self.id()?, ty: self.ty()? },
            6 => Instruction::Mov { dst: self.id()?, ty: self.ty()?, src: self.id()? },
            7 => Instruction::Bin {
                dst: self.id()?,
                op: decode_bin(self.uint()?)?,
                ty: self.ty()?,
                lhs: self.id()?,
                rhs: self.id()?,
            },
            8 => Instruction::Neg { dst: self.id()?, ty: self.ty()?, src: self.id()? },
            9 => Instruction::Not { dst: self.id()?, ty: self.ty()?, src: self.id()? },
            10 => Instruction::Cmp {
                dst: self.id()?,
                op: decode_cmp(self.uint()?)?,
                ty: self.ty()?,
                lhs: self.id()?,
                rhs: self.id()?,
            },
            11 => Instruction::ICmp {
                dst: self.id()?,
                pred: decode_icmp(self.uint()?)?,
                ty: self.ty()?,
                lhs: self.id()?,
                rhs: self.id()?,
            },
            12 => Instruction::FCmp {
                dst: self.id()?,
                pred: decode_fcmp(self.uint()?)?,
                ty: self.ty()?,
                lhs: self.id()?,
                rhs: self.id()?,
            },
            13 => Instruction::Select {
                dst: self.id()?,
                ty: self.ty()?,
                cond: self.id()?,
                on_true: self.id()?,
                on_false: self.id()?,
            },
            14 => Instruction::Cast {
                dst: self.id()?,
                op: decode_cast(self.uint()?)?,
                src_ty: self.ty()?,
                dst_ty: self.ty()?,
                src: self.id()?,
            },
            15 => Instruction::Phi { dst: self.id()?, ty: self.ty()?, incomings: self.phi()? },
            16 => Instruction::Extract {
                dst: self.id()?,
                dst_ty: self.ty()?,
                tuple: self.id()?,
                index: self.uint()?.try_into().ok()?,
            },
            17 => Instruction::Checked {
                dst: self.id()?,
                op: decode_checked(self.uint()?)?,
                ty: self.ty()?,
                lhs: self.id()?,
                rhs: self.id()?,
            },
            18 => Instruction::Alloca {
                dst: self.id()?,
                ty: self.ty()?,
                align: self.uint()?.try_into().ok()?,
            },
            19 => Instruction::VolatileLoad {
                dst: self.id()?,
                ty: self.ty()?,
                ptr: self.id()?,
                align: self.uint()?.try_into().ok()?,
            },
            20 => Instruction::VolatileStore {
                ty: self.ty()?,
                value: self.id()?,
                ptr: self.id()?,
                align: self.uint()?.try_into().ok()?,
            },
            21 => Instruction::Load {
                dst: self.id()?,
                ty: self.ty()?,
                ptr: self.id()?,
                align: self.uint()?.try_into().ok()?,
            },
            22 => Instruction::Uninit {
                ty: self.ty()?,
                ptr: self.id()?,
                align: self.uint()?.try_into().ok()?,
            },
            23 => Instruction::Store {
                ty: self.ty()?,
                value: self.id()?,
                ptr: self.id()?,
                align: self.uint()?.try_into().ok()?,
            },
            24 => Instruction::Gep {
                dst: self.id()?,
                dst_ty: self.ty()?,
                base_ptr: self.id()?,
                indices: self.values()?,
            },
            25 => Instruction::Memcpy {
                dst: self.id()?,
                src: self.id()?,
                n: self.id()?,
                align: self.uint()?.try_into().ok()?,
            },
            26 => Instruction::Memset {
                dst: self.id()?,
                val: self.id()?,
                n: self.id()?,
                align: self.uint()?.try_into().ok()?,
            },
            27 => Instruction::NullFunction { dst: self.id()?, signature: self.signature()? },
            28 => Instruction::FunctionAddr {
                dst: self.id()?,
                function: FunctionId(self.uint()?.try_into().ok()?),
                signature: self.signature()?,
            },
            29 => {
                let dst = self.optional()?;
                let ret_ty = self.ty()?;
                let convention = decode_convention(self.uint()?)?;
                let indirect = self.uint()?;
                let target = self.uint()?.try_into().ok()?;
                let callee = if indirect == 0 {
                    Callee::Direct(FunctionId(target))
                } else {
                    Callee::Indirect(ValueId(target))
                };
                Instruction::Call { dst, ret_ty, convention, callee, args: self.values()? }
            },
            30 => Instruction::TrapIf { cond: self.id()?, reason: self.string()? },
            _ => return None,
        })
    }

    fn terminator(&mut self, op: u64) -> Option<Terminator> {
        Some(match op {
            31 => Terminator::Br { target: self.block()? },
            32 => {
                Terminator::CBr { cond: self.id()?, then_bb: self.block()?, else_bb: self.block()? }
            },
            33 => Terminator::Switch {
                ty: self.ty()?,
                value: self.id()?,
                default_bb: self.block()?,
                cases: self.cases()?,
            },
            34 => Terminator::Ret { ty: self.ty()?, value: self.optional()? },
            35 => Terminator::Trap { reason: self.string()? },
            _ => return None,
        })
    }
}

pub(super) struct Store {
    state: State,
    atoms: Vec<Atom>,
    bytes: Vec<u8>,
    records: Vec<Record>,
    globals: Vec<GlobalRecord>,
    blocks: Vec<Block>,
    limits: IrLimits,
}

impl Store {
    fn validate_instruction(&self, instruction: &Instruction) -> Result<(), BuilderError> {
        let mut budget =
            crate::limits::Budget::new(self.limits).map_err(BuilderError::ResourceLimit)?;
        let check = match instruction {
            Instruction::ConstDecl { expression, .. } => budget.expression(expression),
            Instruction::Cast { src_ty, dst_ty, .. } => {
                budget.ty(src_ty).and_then(|_| budget.ty(dst_ty))
            },
            Instruction::Const { ty, .. }
            | Instruction::Undef { ty, .. }
            | Instruction::GlobalAddr { ty, .. }
            | Instruction::Mov { ty, .. }
            | Instruction::Bin { ty, .. }
            | Instruction::Neg { ty, .. }
            | Instruction::Not { ty, .. }
            | Instruction::Cmp { ty, .. }
            | Instruction::ICmp { ty, .. }
            | Instruction::FCmp { ty, .. }
            | Instruction::Select { ty, .. }
            | Instruction::Checked { ty, .. }
            | Instruction::Alloca { ty, .. }
            | Instruction::VolatileLoad { ty, .. }
            | Instruction::Load { ty, .. }
            | Instruction::Uninit { ty, .. }
            | Instruction::Store { ty, .. }
            | Instruction::VolatileStore { ty, .. } => budget.ty(ty),
            Instruction::Phi { ty, incomings, .. } => {
                budget.ty(ty).and_then(|_| budget.nodes(incomings.len().saturating_mul(2)))
            },
            Instruction::Extract { dst_ty, .. } => budget.ty(dst_ty),
            Instruction::Gep { dst_ty, indices, .. } => {
                budget.ty(dst_ty).and_then(|_| budget.nodes(indices.len()))
            },
            Instruction::NullFunction { signature, .. }
            | Instruction::FunctionAddr { signature, .. } => budget.signature(signature),
            Instruction::Call { ret_ty, args, .. } => {
                budget.ty(ret_ty).and_then(|_| budget.nodes(args.len()))
            },
            _ => Ok(()),
        };
        check.map_err(BuilderError::ResourceLimit)
    }

    pub(super) fn new(limits: IrLimits) -> Self {
        Self {
            state: State::default(),
            atoms: Vec::new(),
            bytes: Vec::new(),
            records: Vec::new(),
            globals: Vec::new(),
            blocks: Vec::new(),
            limits,
        }
    }

    #[cfg(feature = "socket")]
    pub(super) fn allocation_prefix(&self, id: BlockId) -> usize {
        self.blocks[id.0 as usize].prefix_count as usize
    }

    pub(super) fn block(&mut self, id: BlockId) {
        assert_eq!(id.0 as usize, self.blocks.len());
        self.blocks.push(Block::default());
    }

    fn error(&self, status: i32) -> BuilderError {
        match status {
            2 => BuilderError::ResourceLimit(LimitError {
                resource: Resource::TypeDepth,
                limit: self.limits.max_type_depth,
            }),
            3 => BuilderError::ResourceLimit(LimitError {
                resource: Resource::Nodes,
                limit: self.limits.max_nodes,
            }),
            4 => BuilderError::ResourceLimit(LimitError {
                resource: Resource::AnalysisWork,
                limit: self.limits.max_analysis_work,
            }),
            10 => BuilderError::ResourceLimit(LimitError {
                resource: Resource::InputBytes,
                limit: self.limits.max_input_bytes,
            }),
            _ => BuilderError::InvalidType,
        }
    }

    // A logical IR node expands to a bounded number of typed wire atoms. This
    // representation budget is separate from the source-model admission count.
    fn node_limit(&self) -> u64 {
        self.limits.max_nodes.saturating_mul(64) as u64
    }

    fn work_limit(&self) -> u64 {
        self.limits.max_analysis_work.saturating_mul(64) as u64
    }

    pub(super) fn room(&self, atoms: usize, bytes: usize) -> Result<(), BuilderError> {
        let atoms = atoms as u64;
        let bytes = bytes as u64;
        if atoms > self.node_limit().saturating_sub(self.state.atoms) {
            return Err(self.error(3));
        }
        if bytes > (self.limits.max_input_bytes as u64).saturating_sub(self.state.bytes) {
            return Err(self.error(10));
        }
        let cost =
            atoms.saturating_mul(3).saturating_add(bytes.saturating_mul(3)).saturating_add(64);
        if cost > self.work_limit().saturating_sub(self.state.work) {
            return Err(self.error(4));
        }
        Ok(())
    }

    pub(super) fn type_room(&self, ty: &Type, lists: usize) -> Result<(), BuilderError> {
        let mut budget =
            crate::limits::Budget::new(self.limits).map_err(BuilderError::ResourceLimit)?;
        budget.ty(ty).map_err(BuilderError::ResourceLimit)?;
        let mut input = Input { count_only: true, ..Input::default() };
        input.ty(ty);
        self.room(input.atom_count.saturating_add(lists).saturating_add(64), 0)
    }

    pub(super) fn instruction(
        &mut self,
        block: BlockId,
        instruction: &Instruction,
        hoist: bool,
        probe: bool,
    ) -> Result<usize, BuilderError> {
        self.validate_instruction(instruction)?;
        let count = Input::instruction(instruction, true);
        self.room(count.atom_count, count.byte_count)?;
        let input = Input::instruction(instruction, false);
        self.apply(block, &input, hoist, probe)
    }

    pub(super) fn terminator(
        &mut self,
        block: BlockId,
        terminator: &Terminator,
        probe: bool,
    ) -> Result<(), BuilderError> {
        match terminator {
            Terminator::Switch { ty, cases, .. } => {
                crate::limits::Budget::new(self.limits)
                    .and_then(|mut b| {
                        b.ty(ty)?;
                        b.nodes(cases.len())
                    })
                    .map_err(BuilderError::ResourceLimit)?;
            },
            Terminator::Ret { ty, .. } => {
                crate::limits::Budget::new(self.limits)
                    .and_then(|mut b| b.ty(ty))
                    .map_err(BuilderError::ResourceLimit)?;
            },
            _ => {},
        }
        let count = Input::terminator(terminator, true);
        self.room(count.atom_count, count.byte_count)?;
        let input = Input::terminator(terminator, false);
        self.apply(block, &input, false, probe).map(|_| ())
    }

    fn apply(
        &mut self,
        block: BlockId,
        input: &Input,
        hoist: bool,
        probe: bool,
    ) -> Result<usize, BuilderError> {
        let before = self.state;
        if before.atoms != self.atoms.len() as u64
            || before.bytes != self.bytes.len() as u64
            || before.records != self.records.len() as u64
        {
            return Err(BuilderError::InvalidType);
        }
        let block_before =
            *self.blocks.get(block.0 as usize).ok_or(BuilderError::UnknownBlock(block))?;
        let run = |mode: u64, this: &mut Self| {
            let mut result = RawResult::default();
            let node_limit = this.node_limit();
            let work_limit = this.work_limit();
            let status = unsafe {
                whale_ir_builder_body(
                    &mut this.state,
                    this.blocks.as_mut_ptr(),
                    this.blocks.len() as u64,
                    block.0.into(),
                    input.op,
                    u64::from(hoist),
                    input.atoms.as_ptr(),
                    input.atoms.len() as u64,
                    input.bytes.as_ptr(),
                    input.bytes.len() as u64,
                    this.limits.max_type_depth as u64 + 1,
                    this.limits.max_const_depth as u64,
                    node_limit,
                    this.limits.max_input_bytes as u64,
                    work_limit,
                    mode,
                    this.atoms.as_mut_ptr(),
                    this.atoms.len() as u64,
                    this.bytes.as_mut_ptr(),
                    this.bytes.len() as u64,
                    this.records.as_mut_ptr(),
                    this.records.len() as u64,
                    &mut result,
                )
            };
            (status, result)
        };
        let (status, result) = run(0, self);
        if status != result.status as i32
            || self.state != before
            || self.blocks[block.0 as usize] != block_before
            || result.work > self.work_limit().saturating_sub(before.work)
            || result.ordinal > block_before.count
        {
            self.state = before;
            self.blocks[block.0 as usize] = block_before;
            return Err(BuilderError::InvalidType);
        }
        if status != 0 {
            return Err(self.error(status));
        }
        if probe {
            return Ok(result.ordinal as usize);
        }
        let expected_atoms =
            self.atoms.len().checked_add(input.atoms.len()).ok_or_else(|| self.error(3))?;
        let expected_bytes =
            self.bytes.len().checked_add(input.bytes.len()).ok_or_else(|| self.error(10))?;
        let expected_records = self.records.len().checked_add(1).ok_or_else(|| self.error(3))?;
        let expected_work = before.work.checked_add(result.work).ok_or_else(|| self.error(4))?;
        let ordinal = result.ordinal;
        let probe_work = result.work;
        let mut previous = None;
        let mut next = u64::MAX;
        if input.op < 31 {
            if hoist {
                if ordinal != block_before.prefix_count {
                    return Err(BuilderError::InvalidType);
                }
                next = block_before.head;
                if block_before.prefix_tail != u64::MAX {
                    let index = usize::try_from(block_before.prefix_tail)
                        .map_err(|_| BuilderError::InvalidType)?;
                    let record = *self.records.get(index).ok_or(BuilderError::InvalidType)?;
                    if record.block != u64::from(block.0) || record.op != 18 {
                        return Err(BuilderError::InvalidType);
                    }
                    previous = Some((index, record));
                    next = record.next;
                }
            } else if block_before.tail != u64::MAX {
                let index =
                    usize::try_from(block_before.tail).map_err(|_| BuilderError::InvalidType)?;
                let record = *self.records.get(index).ok_or(BuilderError::InvalidType)?;
                if record.block != u64::from(block.0) || record.op >= 31 || record.next != u64::MAX
                {
                    return Err(BuilderError::InvalidType);
                }
                previous = Some((index, record));
            }
        }
        let new_index = before.records;
        let mut expected_block = block_before;
        if input.op < 31 {
            if input.op == 18 && (hoist || block_before.prefix_count == block_before.count) {
                expected_block.prefix_tail = new_index;
                expected_block.prefix_count =
                    block_before.prefix_count.checked_add(1).ok_or(BuilderError::InvalidType)?;
            }
            expected_block.count =
                expected_block.count.checked_add(1).ok_or(BuilderError::InvalidType)?;
            if previous.is_none() {
                expected_block.head = new_index;
            }
            if next == u64::MAX {
                expected_block.tail = new_index;
            }
        } else {
            expected_block.terminator = new_index;
        }
        let expected_record = Record {
            op: input.op,
            block: u64::from(block.0),
            first: before.atoms,
            count: input.atoms.len() as u64,
            byte_first: before.bytes,
            byte_count: input.bytes.len() as u64,
            next,
        };
        self.atoms.try_reserve(input.atoms.len()).map_err(|_| self.error(3))?;
        self.bytes.try_reserve(input.bytes.len()).map_err(|_| self.error(10))?;
        self.records.try_reserve(1).map_err(|_| self.error(3))?;
        // Every slot is initialized before crossing FFI. Returned counters can
        // never make Rust expose uninitialized memory, even on a bad backend.
        self.atoms.resize(expected_atoms, Atom::default());
        self.bytes.resize(expected_bytes, 0);
        self.records.resize(expected_records, Record::default());
        let (status, result) = run(1, self);
        if status != 0
            || result.status != 0
            || result.ordinal != ordinal
            || result.work != probe_work
            || self.blocks[block.0 as usize] != expected_block
            || self.records[before.records as usize] != expected_record
            || previous.is_some_and(|(index, mut record)| {
                record.next = new_index;
                self.records[index] != record
            })
            || self.state
                != (State {
                    atoms: expected_atoms as u64,
                    bytes: expected_bytes as u64,
                    records: expected_records as u64,
                    globals: before.globals,
                    work: expected_work,
                })
        {
            self.atoms.truncate(before.atoms as usize);
            self.bytes.truncate(before.bytes as usize);
            self.records.truncate(before.records as usize);
            if let Some((index, record)) = previous {
                self.records[index] = record;
            }
            self.state = before;
            self.blocks[block.0 as usize] = block_before;
            return Err(BuilderError::InvalidType);
        }
        Ok(result.ordinal as usize)
    }

    pub(super) fn global(
        &mut self,
        id: GlobalId,
        name: &str,
        expression: &ConstExpr,
        init: &ConstValue,
        align: u32,
        probe: bool,
    ) -> Result<(), BuilderError> {
        crate::limits::Budget::new(self.limits)
            .and_then(|mut budget| budget.expression(expression))
            .map_err(BuilderError::ResourceLimit)?;
        let count = Input::global(name, expression, init, align, true);
        self.room(count.atom_count, count.byte_count)?;
        let input = Input::global(name, expression, init, align, false);
        self.apply_global(id, &input, probe)
    }

    fn apply_global(
        &mut self,
        id: GlobalId,
        input: &Input,
        probe: bool,
    ) -> Result<(), BuilderError> {
        let before = self.state;
        if before.atoms != self.atoms.len() as u64
            || before.bytes != self.bytes.len() as u64
            || before.records != self.records.len() as u64
            || before.globals != self.globals.len() as u64
        {
            return Err(BuilderError::InvalidType);
        }
        let run = |mode, this: &mut Self| {
            let mut result = RawResult::default();
            let status = unsafe {
                whale_ir_builder_global(
                    &mut this.state,
                    id.0.into(),
                    this.globals.as_mut_ptr(),
                    this.globals.len() as u64,
                    input.atoms.as_ptr(),
                    input.atoms.len() as u64,
                    input.bytes.as_ptr(),
                    input.bytes.len() as u64,
                    this.limits.max_type_depth as u64 + 1,
                    this.limits.max_const_depth as u64,
                    this.node_limit(),
                    this.limits.max_input_bytes as u64,
                    this.work_limit(),
                    mode,
                    this.atoms.as_mut_ptr(),
                    this.atoms.len() as u64,
                    this.bytes.as_mut_ptr(),
                    this.bytes.len() as u64,
                    &mut result,
                )
            };
            (status, result)
        };
        let (status, result) = run(0, self);
        if status != result.status as i32 || self.state != before {
            self.state = before;
            return Err(BuilderError::InvalidType);
        }
        if status != 0 {
            return Err(self.error(status));
        }
        if result.ordinal != before.globals
            || result.work > self.work_limit().saturating_sub(before.work)
        {
            return Err(BuilderError::InvalidType);
        }
        if probe {
            return Ok(());
        }
        let atoms = self.atoms.len().checked_add(input.atoms.len()).ok_or_else(|| self.error(3))?;
        let bytes =
            self.bytes.len().checked_add(input.bytes.len()).ok_or_else(|| self.error(10))?;
        let globals = self.globals.len().checked_add(1).ok_or_else(|| self.error(3))?;
        let work = before.work.checked_add(result.work).ok_or_else(|| self.error(4))?;
        let expected = State {
            atoms: atoms as u64,
            bytes: bytes as u64,
            globals: globals as u64,
            work,
            ..before
        };
        let expected_record = GlobalRecord {
            id: id.0.into(),
            first: before.atoms,
            count: input.atoms.len() as u64,
            byte_first: before.bytes,
            byte_count: input.bytes.len() as u64,
        };
        self.atoms.try_reserve(input.atoms.len()).map_err(|_| self.error(3))?;
        self.bytes.try_reserve(input.bytes.len()).map_err(|_| self.error(10))?;
        self.globals.try_reserve(1).map_err(|_| self.error(3))?;
        self.atoms.resize(atoms, Atom::default());
        self.bytes.resize(bytes, 0);
        self.globals.resize(globals, GlobalRecord::default());
        let probe_work = result.work;
        let (status, result) = run(1, self);
        if status != 0
            || result.status != 0
            || result.ordinal != before.globals
            || result.work != probe_work
            || self.state != expected
            || self.globals[before.globals as usize] != expected_record
        {
            self.atoms.truncate(before.atoms as usize);
            self.bytes.truncate(before.bytes as usize);
            self.globals.truncate(before.globals as usize);
            self.state = before;
            return Err(BuilderError::InvalidType);
        }
        Ok(())
    }

    pub(super) fn restore_globals(&self, globals: &mut Vec<Global>) -> Result<(), BuilderError> {
        let mut restored = Vec::new();
        restored.try_reserve(self.globals.len()).map_err(|_| self.error(3))?;
        for record in &self.globals {
            let first = usize::try_from(record.first).map_err(|_| BuilderError::InvalidType)?;
            let end = first
                .checked_add(usize::try_from(record.count).map_err(|_| BuilderError::InvalidType)?)
                .ok_or(BuilderError::InvalidType)?;
            let mut reader = Reader {
                atoms: self.atoms.get(first..end).ok_or(BuilderError::InvalidType)?,
                bytes: &self.bytes,
                at: 0,
            };
            let name = reader.string().ok_or(BuilderError::InvalidType)?;
            let expression = reader.expression().ok_or(BuilderError::InvalidType)?;
            let init = reader.constant().ok_or(BuilderError::InvalidType)?;
            let align = u32::try_from(reader.uint().ok_or(BuilderError::InvalidType)?)
                .map_err(|_| BuilderError::InvalidType)?;
            if reader.at != reader.atoms.len() {
                return Err(BuilderError::InvalidType);
            }
            restored.push(Global {
                id: GlobalId(u32::try_from(record.id).map_err(|_| BuilderError::InvalidType)?),
                name,
                ty: expression.ty.clone(),
                init,
                init_expr: expression,
                align,
            });
        }
        *globals = restored;
        Ok(())
    }

    fn reader(&self, index: u64) -> Option<(Reader<'_>, u64)> {
        let record = self.records.get(usize::try_from(index).ok()?)?;
        let first = usize::try_from(record.first).ok()?;
        let end = first.checked_add(usize::try_from(record.count).ok()?)?;
        Some((Reader { atoms: self.atoms.get(first..end)?, bytes: &self.bytes, at: 0 }, record.op))
    }

    pub(super) fn restore(&self, function: &mut Function) -> Result<(), BuilderError> {
        for block in &mut function.blocks {
            let model = self.blocks.get(block.id.0 as usize).ok_or(BuilderError::InvalidType)?;
            let mut index = model.head;
            if model.count > self.records.len() as u64 {
                return Err(BuilderError::InvalidType);
            }
            let mut instructions = Vec::new();
            instructions.try_reserve(model.count as usize).map_err(|_| self.error(3))?;
            while index != u64::MAX {
                if instructions.len() >= model.count as usize {
                    return Err(BuilderError::InvalidType);
                }
                let (mut reader, op) = self.reader(index).ok_or(BuilderError::InvalidType)?;
                let instruction = reader.instruction(op).ok_or(BuilderError::InvalidType)?;
                if reader.at != reader.atoms.len() {
                    return Err(BuilderError::InvalidType);
                }
                instructions.push(instruction);
                index = self.records[index as usize].next;
            }
            if instructions.len() != model.count as usize {
                return Err(BuilderError::InvalidType);
            }
            let terminator = if model.terminator == u64::MAX {
                None
            } else {
                let (mut reader, op) =
                    self.reader(model.terminator).ok_or(BuilderError::InvalidType)?;
                let terminator = reader.terminator(op).ok_or(BuilderError::InvalidType)?;
                if reader.at != reader.atoms.len() {
                    return Err(BuilderError::InvalidType);
                }
                Some(terminator)
            };
            block.instructions = instructions;
            block.terminator = terminator;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory() -> Vec<Instruction> {
        let dst = ValueId(17);
        let x = ValueId(3);
        let y = ValueId(8);
        let ty = Type::I128;
        let signature =
            FunctionSignature::whale(vec![Type::I128, Type::ptr_to(Type::Void)], Type::I128);
        let expression = ConstExpr {
            ty: ty.clone(),
            kind: ConstExprKind::Binary {
                op: ConstBinaryOp::Add,
                left: Box::new(ConstExpr::literal(ty.clone(), ConstValue::I(i128::MIN))),
                right: Box::new(ConstExpr {
                    ty: ty.clone(),
                    kind: ConstExprKind::Reference(ConstRef::Global(GlobalId(99))),
                }),
            },
        };
        vec![
            Instruction::GlobalAddr { dst, global: GlobalId(29), ty: ty.clone() },
            Instruction::LifetimeEnd { ptr: x },
            Instruction::PtrDiff { dst, lhs: x, rhs: y },
            Instruction::ConstDecl {
                dst,
                name: "λ🐋\0 declaration".into(),
                expression,
                value: ConstValue::I(i128::MIN + 7),
            },
            Instruction::Const {
                dst,
                ty: Type::F64,
                value: ConstValue::F(FloatBits::F64(0xfff8_0123_4567_89ab)),
            },
            Instruction::Undef { dst, ty: Type::Tuple(vec![ty.clone(), Type::Bool]) },
            Instruction::Mov { dst, ty: ty.clone(), src: x },
            Instruction::Bin { dst, op: BinOp::AShr, ty: ty.clone(), lhs: x, rhs: y },
            Instruction::Neg { dst, ty: ty.clone(), src: x },
            Instruction::Not { dst, ty: ty.clone(), src: y },
            Instruction::Cmp { dst, op: CmpOp::FGe, ty: Type::F32, lhs: x, rhs: y },
            Instruction::ICmp { dst, pred: ICmpPred::Sge, ty: ty.clone(), lhs: x, rhs: y },
            Instruction::FCmp { dst, pred: FCmpPred::Uge, ty: Type::F16, lhs: x, rhs: y },
            Instruction::Select { dst, ty: ty.clone(), cond: x, on_true: x, on_false: y },
            Instruction::Cast {
                dst,
                op: CastOp::Bitcast,
                src_ty: Type::F64,
                dst_ty: Type::U64,
                src: x,
            },
            Instruction::Phi {
                dst,
                ty: ty.clone(),
                incomings: vec![(x, BlockId(0)), (y, BlockId(4))],
            },
            Instruction::Extract { dst, dst_ty: ty.clone(), tuple: x, index: u32::MAX },
            Instruction::Checked { dst, op: CheckedOp::SMul, ty: ty.clone(), lhs: x, rhs: y },
            Instruction::Alloca {
                dst,
                ty: Type::Struct(vec![
                    Type::Array(Box::new(Type::U8), u64::MAX),
                    Type::FnPtr(Box::new(signature.clone())),
                ]),
                align: 64,
            },
            Instruction::VolatileLoad { dst, ty: ty.clone(), ptr: x, align: 16 },
            Instruction::VolatileStore { ty: ty.clone(), value: y, ptr: x, align: 16 },
            Instruction::Load { dst, ty: ty.clone(), ptr: x, align: 16 },
            Instruction::Uninit { ty: ty.clone(), ptr: x, align: 16 },
            Instruction::Store { ty: ty.clone(), value: y, ptr: x, align: 16 },
            Instruction::Gep {
                dst,
                dst_ty: Type::ptr_to(Type::Array(Box::new(Type::U32), 5)),
                base_ptr: x,
                indices: vec![x, y],
            },
            Instruction::Memcpy { dst: x, src: y, n: dst, align: 8 },
            Instruction::Memset { dst: x, val: y, n: dst, align: 8 },
            Instruction::NullFunction { dst, signature: signature.clone() },
            Instruction::FunctionAddr { dst, function: FunctionId(32), signature },
            Instruction::Call {
                dst: Some(dst),
                ret_ty: ty,
                convention: CallingConvention::SysV64,
                callee: Callee::Indirect(y),
                args: vec![x, y],
            },
            Instruction::TrapIf { cond: x, reason: "reason\0🐋\n\"".into() },
        ]
    }

    fn function(blocks: usize) -> Function {
        Function {
            id: FunctionId(0),
            name: "inventory".into(),
            params: Vec::new(),
            ret_ty: Type::Void,
            entry: BlockId(0),
            value_types: Vec::new(),
            blocks: (0..blocks)
                .map(|index| BasicBlock::new(BlockId(index as u32), format!("b{index}")))
                .collect(),
        }
    }

    #[test]
    fn wave_global_records_preserve_exact_typed_literals_and_ids() {
        let mut store = Store::new(IrLimits::default());
        let literals = [
            (Type::I128, ConstValue::I(i128::MIN)),
            (Type::U128, ConstValue::U(u128::MAX)),
            (Type::F16, ConstValue::F(FloatBits::F16(0xfe55))),
            (Type::F32, ConstValue::F(FloatBits::F32(0xffc1_2345))),
            (Type::F64, ConstValue::F(FloatBits::F64(0xfff8_0123_4567_89ab))),
            (Type::Bool, ConstValue::Bool(true)),
        ];
        let mut expected = Vec::new();
        for (index, (ty, value)) in literals.into_iter().enumerate() {
            let id = GlobalId(index as u32 * 2);
            let name = format!("λ\0 literal {index}");
            let expression = ConstExpr::literal(ty.clone(), value.clone());
            store.global(id, &name, &expression, &value, 16, false).unwrap();
            expected.push(Global { id, name, ty, init: value, init_expr: expression, align: 16 });
        }
        let before = (store.state, store.atoms.clone(), store.bytes.clone(), store.globals.clone());
        let mut invalid = Input::global(
            "bad",
            &ConstExpr::literal(Type::I32, ConstValue::I(0)),
            &ConstValue::I(0),
            4,
            false,
        );
        invalid.atoms.pop();
        assert!(store.apply_global(GlobalId(99), &invalid, false).is_err());
        assert_eq!(
            (store.state, store.atoms.clone(), store.bytes.clone(), store.globals.clone()),
            before
        );
        let mut globals = Vec::new();
        store.restore_globals(&mut globals).unwrap();
        assert_eq!(globals, expected);
    }

    #[test]
    fn wave_body_is_authoritative_for_the_complete_instruction_and_terminator_inventory() {
        let mut store = Store::new(IrLimits::default());
        for index in 0..5 {
            store.block(BlockId(index));
        }
        let instructions = inventory();
        for instruction in &instructions {
            store.instruction(BlockId(0), instruction, false, false).unwrap();
        }
        let terms = [
            Terminator::Br { target: BlockId(4) },
            Terminator::CBr { cond: ValueId(3), then_bb: BlockId(0), else_bb: BlockId(4) },
            Terminator::Switch {
                ty: Type::U128,
                value: ValueId(8),
                default_bb: BlockId(0),
                cases: vec![(ConstValue::U(u128::MAX), BlockId(4)), (ConstValue::U(0), BlockId(2))],
            },
            Terminator::Ret { ty: Type::I128, value: Some(ValueId(17)) },
            Terminator::Trap { reason: "trap λ\0".into() },
        ];
        for (index, term) in terms.iter().enumerate() {
            store.terminator(BlockId(index as u32), term, false).unwrap();
        }
        let mut output = function(5);
        // Poison the compatibility mirror: reconstruction must use the complete
        // Wave model, including literals, names, expression/list/callee fields.
        output.blocks[0].instructions.push(Instruction::LifetimeEnd { ptr: ValueId(999) });
        store.restore(&mut output).unwrap();
        assert_eq!(output.blocks[0].instructions, instructions);
        for (index, term) in terms.into_iter().enumerate() {
            assert_eq!(output.blocks[index].terminator, Some(term));
        }
    }

    #[test]
    fn wave_body_many_hoists_use_constant_work_and_preserve_payload_order() {
        let count = 10000;
        let mut store =
            Store::new(IrLimits { max_analysis_work: count / 4, ..IrLimits::default() });
        store.block(BlockId(0));
        let literal = Instruction::Const {
            dst: ValueId(count as u32),
            ty: Type::I32,
            value: ConstValue::I(42),
        };
        store.instruction(BlockId(0), &literal, false, false).unwrap();
        store
            .terminator(BlockId(0), &Terminator::Ret { ty: Type::Void, value: None }, false)
            .unwrap();
        let mut cost = None;
        for index in 0..count {
            let before = store.state.work;
            let allocation =
                Instruction::Alloca { dst: ValueId(index as u32), ty: Type::I32, align: 4 };
            assert_eq!(store.instruction(BlockId(0), &allocation, true, false), Ok(index));
            let work = store.state.work - before;
            assert_eq!(*cost.get_or_insert(work), work);
        }
        assert!(store.state.work <= count as u64 * 16);
        let mut output = function(1);
        store.restore(&mut output).unwrap();
        for (index, instruction) in output.blocks[0].instructions[..count].iter().enumerate() {
            assert_eq!(
                instruction,
                &Instruction::Alloca { dst: ValueId(index as u32), ty: Type::I32, align: 4 }
            );
        }
        assert_eq!(output.blocks[0].instructions[count], literal);
        assert_eq!(
            output.blocks[0].terminator,
            Some(Terminator::Ret { ty: Type::Void, value: None })
        );
    }

    #[test]
    fn wave_body_hoisting_and_failed_appends_preserve_all_existing_payload() {
        let mut store = Store::new(IrLimits::default());
        store.block(BlockId(0));
        let allocation = Instruction::Alloca { dst: ValueId(0), ty: Type::I32, align: 4 };
        let literal =
            Instruction::Const { dst: ValueId(1), ty: Type::U128, value: ConstValue::U(u128::MAX) };
        store.instruction(BlockId(0), &allocation, false, false).unwrap();
        store.instruction(BlockId(0), &literal, false, false).unwrap();
        store
            .terminator(BlockId(0), &Terminator::Ret { ty: Type::Void, value: None }, false)
            .unwrap();
        let hoisted = Instruction::Alloca { dst: ValueId(2), ty: Type::Bool, align: 1 };
        assert_eq!(store.instruction(BlockId(0), &hoisted, true, false), Ok(1));
        let before = (
            store.state,
            store.atoms.clone(),
            store.bytes.clone(),
            store.records.clone(),
            store.blocks.clone(),
        );
        assert!(store.instruction(BlockId(0), &literal, false, false).is_err());
        let mut malformed = Input::instruction(&allocation, false);
        malformed.atoms.pop();
        assert!(store.apply(BlockId(0), &malformed, true, false).is_err());
        assert_eq!(
            (
                store.state,
                store.atoms.clone(),
                store.bytes.clone(),
                store.records.clone(),
                store.blocks.clone()
            ),
            before
        );
        let mut output = function(1);
        store.restore(&mut output).unwrap();
        assert_eq!(output.blocks[0].instructions, vec![allocation, hoisted, literal]);
        assert_eq!(
            output.blocks[0].terminator,
            Some(Terminator::Ret { ty: Type::Void, value: None })
        );
    }
}
