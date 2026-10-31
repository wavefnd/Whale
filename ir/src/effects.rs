// SPDX-License-Identifier: MPL-2.0
//! Conservative observable-effect classification, independent of optimizer policy.
#[cfg(any(not(whale_wave_ir), test))]
use crate::{BinOp, CastOp, Type};

use crate::{Instruction, Terminator};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub reads_memory: bool,
    pub writes_memory: bool,
    pub volatile: bool,
    pub calls: bool,
    pub may_trap: bool,
    pub changes_lifetime: bool,
}

impl Effects {
    /// Necessary, never sufficient, for deleting an unused instruction. A pass
    /// must also prove no SSA/source/CFG identity or required observation is lost.
    pub fn permits_unused_removal(self) -> bool {
        #[cfg(whale_wave_ir)]
        {
            crate::effects_wave::removable(self).expect("Wave effect classification")
        }
        #[cfg(not(whale_wave_ir))]
        {
            self == Self::default()
        }
    }
}

impl Instruction {
    pub fn effects(&self) -> Effects {
        #[cfg(whale_wave_ir)]
        {
            crate::effects_wave::instruction(self).expect("Wave instruction effects")
        }
        #[cfg(not(whale_wave_ir))]
        {
            self.effects_reference()
        }
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub(crate) fn effects_reference(&self) -> Effects {
        use Instruction::*;
        let mut e = Effects::default();
        match self {
            VolatileLoad { .. } => {
                e.reads_memory = true;
                e.volatile = true;
                e.may_trap = true;
            },
            VolatileStore { .. } => {
                e.writes_memory = true;
                e.volatile = true;
                e.may_trap = true;
            },
            Load { .. } => {
                e.reads_memory = true;
                e.may_trap = true;
            },
            Store { .. } | Uninit { .. } | Memset { .. } => {
                e.writes_memory = true;
                e.may_trap = true;
            },
            Memcpy { .. } => {
                e.reads_memory = true;
                e.writes_memory = true;
                e.may_trap = true;
            },
            Alloca { .. } | LifetimeEnd { .. } => {
                e.changes_lifetime = true;
                e.may_trap = true;
            },
            Call { .. } => {
                e.calls = true;
                e.reads_memory = true;
                e.writes_memory = true;
                e.may_trap = true;
            },
            Gep { .. } | PtrDiff { .. } | TrapIf { .. } | Undef { .. } => e.may_trap = true,
            Bin { op: BinOp::UDiv | BinOp::SDiv | BinOp::URem | BinOp::SRem, .. } => {
                e.may_trap = true
            },
            Cast { op: CastOp::CheckedInt | CastOp::FToI_S | CastOp::FToI_U, .. } => {
                e.may_trap = true
            },
            Cmp { ty: Type::Ptr(_), .. } | ICmp { ty: Type::Ptr(_), .. } => e.may_trap = true,
            Const { .. }
            | ConstDecl { .. }
            | Mov { .. }
            | Bin { .. }
            | Neg { .. }
            | Not { .. }
            | Cmp { .. }
            | ICmp { .. }
            | FCmp { .. }
            | Select { .. }
            | Cast { .. }
            | Phi { .. }
            | Extract { .. }
            | Checked { .. }
            | NullFunction { .. }
            | FunctionAddr { .. }
            | GlobalAddr { .. } => {},
        }
        e
    }
}

impl Terminator {
    pub fn effects(&self) -> Effects {
        #[cfg(whale_wave_ir)]
        {
            crate::effects_wave::terminator(self).expect("Wave terminator effects")
        }
        #[cfg(not(whale_wave_ir))]
        {
            self.effects_reference()
        }
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub(crate) fn effects_reference(&self) -> Effects {
        Effects { may_trap: matches!(self, Self::Trap { .. }), ..Effects::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        BlockId, Callee, CallingConvention, ConstExpr, ConstValue, FunctionId, FunctionSignature,
        GlobalId, ValueId,
    };

    #[test]
    fn all_instruction_and_terminator_effects_match_the_independent_inventory() {
        use Instruction::*;
        let id = ValueId(0);
        let ty = Type::I32;
        let signature = FunctionSignature::whale(vec![], Type::Void);
        let mut instructions = vec![
            GlobalAddr { dst: id, global: GlobalId(0), ty: ty.clone() },
            LifetimeEnd { ptr: id },
            PtrDiff { dst: id, lhs: id, rhs: id },
            ConstDecl {
                dst: id,
                name: "x".into(),
                expression: ConstExpr::literal(ty.clone(), ConstValue::I(0)),
                value: ConstValue::I(0),
            },
            Const { dst: id, ty: ty.clone(), value: ConstValue::I(0) },
            Undef { dst: id, ty: ty.clone() },
            Mov { dst: id, ty: ty.clone(), src: id },
            Neg { dst: id, ty: ty.clone(), src: id },
            Not { dst: id, ty: ty.clone(), src: id },
            Cmp { dst: id, op: crate::CmpOp::Eq, ty: ty.clone(), lhs: id, rhs: id },
            ICmp { dst: id, pred: crate::ICmpPred::Eq, ty: ty.clone(), lhs: id, rhs: id },
            FCmp { dst: id, pred: crate::FCmpPred::Oeq, ty: Type::F32, lhs: id, rhs: id },
            Select { dst: id, ty: ty.clone(), cond: id, on_true: id, on_false: id },
            Phi { dst: id, ty: ty.clone(), incomings: vec![(id, BlockId(0))] },
            Extract { dst: id, dst_ty: ty.clone(), tuple: id, index: 0 },
            Checked { dst: id, op: crate::CheckedOp::UAdd, ty: ty.clone(), lhs: id, rhs: id },
            Alloca { dst: id, ty: ty.clone(), align: 4 },
            VolatileLoad { dst: id, ty: ty.clone(), ptr: id, align: 4 },
            VolatileStore { ty: ty.clone(), value: id, ptr: id, align: 4 },
            Load { dst: id, ty: ty.clone(), ptr: id, align: 4 },
            Store { ty: ty.clone(), value: id, ptr: id, align: 4 },
            Uninit { ty: ty.clone(), ptr: id, align: 4 },
            Gep {
                dst: id,
                dst_ty: Type::Ptr(Box::new(ty.clone())),
                base_ptr: id,
                indices: vec![id],
            },
            Memcpy { dst: id, src: id, n: id, align: 4 },
            Memset { dst: id, val: id, n: id, align: 4 },
            NullFunction { dst: id, signature: signature.clone() },
            FunctionAddr { dst: id, function: FunctionId(0), signature },
            Call {
                convention: CallingConvention::Whale,
                dst: None,
                ret_ty: Type::Void,
                callee: Callee::Direct(FunctionId(0)),
                args: vec![],
            },
            TrapIf { cond: id, reason: "test".into() },
        ];
        for op in [
            BinOp::Add,
            BinOp::Sub,
            BinOp::Mul,
            BinOp::UDiv,
            BinOp::SDiv,
            BinOp::URem,
            BinOp::SRem,
            BinOp::FAdd,
            BinOp::FSub,
            BinOp::FMul,
            BinOp::FDiv,
            BinOp::FRem,
            BinOp::And,
            BinOp::Or,
            BinOp::Xor,
            BinOp::Shl,
            BinOp::LShr,
            BinOp::AShr,
        ] {
            instructions.push(Bin { dst: id, op, ty: ty.clone(), lhs: id, rhs: id });
        }
        for op in [
            CastOp::CheckedInt,
            CastOp::ZExt,
            CastOp::SExt,
            CastOp::Trunc,
            CastOp::FExt,
            CastOp::FTrunc,
            CastOp::IToF_S,
            CastOp::IToF_U,
            CastOp::FToI_S,
            CastOp::FToI_U,
            CastOp::Bitcast,
            CastOp::PtrToInt,
            CastOp::IntToPtr,
        ] {
            instructions.push(Cast {
                dst: id,
                op,
                dst_ty: ty.clone(),
                src_ty: ty.clone(),
                src: id,
            });
        }
        let pointer = Type::Ptr(Box::new(Type::I32));
        instructions.push(Cmp {
            dst: id,
            op: crate::CmpOp::Eq,
            ty: pointer.clone(),
            lhs: id,
            rhs: id,
        });
        instructions.push(ICmp {
            dst: id,
            pred: crate::ICmpPred::Eq,
            ty: pointer,
            lhs: id,
            rhs: id,
        });
        for instruction in instructions {
            let expected = instruction.effects_reference();
            assert_eq!(instruction.effects(), expected, "{instruction:?}");
            assert_eq!(expected.permits_unused_removal(), expected == Effects::default());
        }
        for terminator in [
            Terminator::Br { target: BlockId(0) },
            Terminator::CBr { cond: id, then_bb: BlockId(0), else_bb: BlockId(1) },
            Terminator::Switch { ty, value: id, default_bb: BlockId(0), cases: vec![] },
            Terminator::Ret { ty: Type::Void, value: None },
            Terminator::Trap { reason: "test".into() },
        ] {
            assert_eq!(terminator.effects(), terminator.effects_reference());
        }
    }
}
