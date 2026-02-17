// SPDX-License-Identifier: MPL-2.0

use crate::{BlockId, Type, ValueId};

#[derive(Clone, Debug, PartialEq)]
pub enum ConstValue {
    Bool(bool),
    I(i128),
    U(u128),
    F(f64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ICmpPred {
    Eq,
    Ne,
    Ult,
    Ule,
    Ugt,
    Uge,
    Slt,
    Sle,
    Sgt,
    Sge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FCmpPred {
    Oeq,
    One,
    Olt,
    Ole,
    Ogt,
    Oge,
    Ord,
    Uno,
    Ueq,
    Une,
    Ult,
    Ule,
    Ugt,
    Uge,
}

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, PartialEq)]
pub enum CastOp {
    ZExt,
    SExt,
    Trunc,
    FExt,
    FTrunc,
    IToF_S,
    IToF_U,
    FToI_S,
    FToI_U,
    Bitcast,
    PtrToInt,
    IntToPtr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BinOp {
    // int
    Add,
    Sub,
    Mul,
    UDiv,
    SDiv,
    URem,
    SRem,

    // float
    FAdd,
    FSub,
    FMul,
    FDiv,
    FRem,

    // bit
    And,
    Or,
    Xor,
    Shl,
    LShr,
    AShr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CheckedOp {
    // returns tuple<T, i1>
    UAdd,
    USub,
    UMul,
    SAdd,
    SSub,
    SMul,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Callee {
    Symbol(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Instruction {
    Const {
        dst: ValueId,
        ty: Type,
        value: ConstValue,
    },
    Undef {
        dst: ValueId,
        ty: Type,
    },
    Mov {
        dst: ValueId,
        ty: Type,
        src: ValueId,
    },

    Bin {
        dst: ValueId,
        op: BinOp,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    },

    Not {
        dst: ValueId,
        ty: Type,
        src: ValueId,
    },

    Cmp { dst: ValueId, op: CmpOp, ty: Type, lhs: ValueId, rhs: ValueId },

    ICmp {
        dst: ValueId,
        pred: ICmpPred,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    },
    FCmp {
        dst: ValueId,
        pred: FCmpPred,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    },

    Select {
        dst: ValueId,
        ty: Type,
        cond: ValueId,
        on_true: ValueId,
        on_false: ValueId,
    },

    Cast {
        dst: ValueId,
        op: CastOp,
        dst_ty: Type,
        src_ty: Type,
        src: ValueId,
    },

    Phi {
        dst: ValueId,
        ty: Type,
        incomings: Vec<(ValueId, BlockId)>,
    },

    // tuple extract
    Extract {
        dst: ValueId,
        dst_ty: Type,
        tuple: ValueId,
        index: u32,
    },

    // checked arithmetic
    Checked {
        dst: ValueId,
        op: CheckedOp,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    },

    // memory
    Alloca {
        dst: ValueId,
        ty: Type,
        align: u32,
    },
    Load {
        dst: ValueId,
        ty: Type,
        ptr: ValueId,
        align: u32,
    },
    Store {
        ty: Type,
        value: ValueId,
        ptr: ValueId,
        align: u32,
    },

    Gep {
        dst: ValueId,
        dst_ty: Type,
        base_ptr: ValueId,
        indices: Vec<ValueId>,
    },

    Memcpy {
        dst: ValueId,
        src: ValueId,
        n: ValueId,
        align: u32,
    },
    Memset {
        dst: ValueId,
        val: ValueId,
        n: ValueId,
        align: u32,
    },

    Call {
        dst: Option<ValueId>,
        ret_ty: Type,
        callee: Callee,
        args: Vec<ValueId>,
    },

    TrapIf {
        cond: ValueId,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Terminator {
    Br {
        target: BlockId,
    },
    CBr {
        cond: ValueId,
        then_bb: BlockId,
        else_bb: BlockId,
    },
    Switch {
        ty: Type,
        value: ValueId,
        default_bb: BlockId,
        cases: Vec<(ConstValue, BlockId)>,
    },
    Ret {
        ty: Type,
        value: Option<ValueId>,
    },
    Trap {
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CmpOp {
    // int/ptr equality
    Eq,
    Ne,

    // signed int ordering
    SLt,
    SLe,
    SGt,
    SGe,

    // unsigned int ordering
    ULt,
    ULe,
    UGt,
    UGe,

    // float ordering
    FEq,
    FNe,
    FLt,
    FLe,
    FGt,
    FGe,
}