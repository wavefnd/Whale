// SPDX-License-Identifier: MPL-2.0

use crate::{BlockId, ConstExpr, Type, ValueId};

#[derive(Clone, Debug, PartialEq)]
pub enum ConstValue {
    Bool(bool),
    I(i128),
    U(u128),
    F(crate::FloatBits),
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

/// Integer add/sub/mul wrap modulo 2^N. Division/remainder by zero trap;
/// signed MIN / -1 wraps to MIN and MIN % -1 is zero. Shift counts are
/// interpreted as unsigned N-bit patterns and reduced modulo N. LShr fills
/// with zero; AShr replicates the high bit, irrespective of result signedness.
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

/// Produces tuple<T, bool>: wrapped result, then mathematical range overflow.
/// Overflow is reported, not trapped; a frontend can emit explicit TrapIf.
#[derive(Clone, Debug, PartialEq)]
pub enum CheckedOp {
    // returns tuple<T, bool>
    UAdd,
    USub,
    UMul,
    SAdd,
    SSub,
    SMul,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Callee {
    Direct(crate::FunctionId),
    Indirect(ValueId),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Instruction {
    /// A named compile-time declaration retained at its original IR position.
    /// Its value is already evaluated; the expression is preserved for O0
    /// inspection and validation, never executed as runtime arithmetic.
    ConstDecl {
        dst: ValueId,
        name: String,
        expression: ConstExpr,
        value: ConstValue,
    },
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

    Cmp {
        dst: ValueId,
        op: CmpOp,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    },

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

    /// Selects a value by predecessor block. Phis form a block's instruction
    /// prefix, with one input per distinct predecessor (even if that block has
    /// several edges to this block). Inputs are available at predecessor exit.
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

    /// Computes an address without loading memory. The first integer index
    /// offsets the base in units of its pointee type; subsequent indices select
    /// array elements or literal struct/tuple fields. The result points to the
    /// selected type. Empty indices preserve the original pointer type.
    /// This type contract does not establish dynamic bounds or lifetime safety.
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

    NullFunction {
        dst: ValueId,
        signature: crate::FunctionSignature,
    },
    FunctionAddr {
        dst: ValueId,
        function: crate::FunctionId,
        signature: crate::FunctionSignature,
    },
    /// Null and invalid indirect targets trap before entering a callee.
    Call {
        convention: crate::CallingConvention,
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
