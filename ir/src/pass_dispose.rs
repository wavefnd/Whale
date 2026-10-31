// SPDX-License-Identifier: MPL-2.0
//! Iterative disposal of a rejected transform, including malformed deep trees.
use crate::limits::{discard_signature, discard_type};

use crate::{ConstExpr, ConstExprKind, Instruction, Module, Terminator};

fn expression(root: ConstExpr) {
    let mut pending = vec![root];
    while let Some(expression) = pending.pop() {
        discard_type(expression.ty);
        match expression.kind {
            ConstExprKind::Not(value) | ConstExprKind::Neg(value) => pending.push(*value),
            ConstExprKind::Binary { left, right, .. }
            | ConstExprKind::Compare { left, right, .. } => {
                pending.push(*left);
                pending.push(*right);
            },
            ConstExprKind::Literal(_) | ConstExprKind::Reference(_) => {},
        }
    }
}

fn instruction(instruction: Instruction) {
    use Instruction::*;
    match instruction {
        ConstDecl { expression: root, .. } => expression(root),
        GlobalAddr { ty, .. }
        | Const { ty, .. }
        | Undef { ty, .. }
        | Mov { ty, .. }
        | Bin { ty, .. }
        | Neg { ty, .. }
        | Not { ty, .. }
        | Cmp { ty, .. }
        | ICmp { ty, .. }
        | FCmp { ty, .. }
        | Select { ty, .. }
        | Phi { ty, .. }
        | Checked { ty, .. }
        | Alloca { ty, .. }
        | VolatileLoad { ty, .. }
        | VolatileStore { ty, .. }
        | Load { ty, .. }
        | Uninit { ty, .. }
        | Store { ty, .. } => discard_type(ty),
        Cast { src_ty, dst_ty, .. } => {
            discard_type(src_ty);
            discard_type(dst_ty);
        },
        Extract { dst_ty, .. } | Gep { dst_ty, .. } => discard_type(dst_ty),
        NullFunction { signature, .. } | FunctionAddr { signature, .. } => {
            discard_signature(signature);
        },
        Call { ret_ty, .. } => discard_type(ret_ty),
        LifetimeEnd { .. } | PtrDiff { .. } | Memcpy { .. } | Memset { .. } | TrapIf { .. } => {},
    }
}

pub(super) fn discard(module: Module) {
    for global in module.globals {
        discard_type(global.ty);
        expression(global.init_expr);
    }
    for global in module.global_storage {
        discard_type(global.ty);
    }
    for declaration in module.declarations {
        discard_signature(declaration.signature);
    }
    for function in module.functions {
        discard_type(function.ret_ty);
        for parameter in function.params {
            discard_type(parameter.ty);
        }
        for (_, ty) in function.value_types {
            discard_type(ty);
        }
        for block in function.blocks {
            for value in block.instructions {
                instruction(value);
            }
            if let Some(Terminator::Switch { ty, .. } | Terminator::Ret { ty, .. }) =
                block.terminator
            {
                discard_type(ty);
            }
        }
    }
}
