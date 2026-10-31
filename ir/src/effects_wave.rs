// SPDX-License-Identifier: MPL-2.0
//! Enum/data transport for the Wave effect classifier.
use super::effects::Effects;

use crate::{BinOp, CastOp, Instruction, Terminator, Type};

unsafe extern "C" {
    fn whale_ir_effects(op: u64, sub: u64, pointer: u64, terminator: u64, output: *mut u64) -> i32;

    fn whale_ir_effects_removable(mask: u64) -> i32;
}

fn decode(mask: u64) -> Option<Effects> {
    (mask <= 63).then_some(Effects {
        reads_memory: mask & 1 != 0,
        writes_memory: mask & 2 != 0,
        volatile: mask & 4 != 0,
        calls: mask & 8 != 0,
        may_trap: mask & 16 != 0,
        changes_lifetime: mask & 32 != 0,
    })
}

pub(crate) fn removable(e: Effects) -> Result<bool, &'static str> {
    let mask = u64::from(e.reads_memory)
        | (u64::from(e.writes_memory) << 1)
        | (u64::from(e.volatile) << 2)
        | (u64::from(e.calls) << 3)
        | (u64::from(e.may_trap) << 4)
        | (u64::from(e.changes_lifetime) << 5);
    match unsafe { whale_ir_effects_removable(mask) } {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err("effect classification transport"),
    }
}

pub(crate) fn instruction(i: &Instruction) -> Result<Effects, &'static str> {
    use Instruction::*;
    let op = match i {
        GlobalAddr { .. } => 0,
        LifetimeEnd { .. } => 1,
        PtrDiff { .. } => 2,
        ConstDecl { .. } => 3,
        Const { .. } => 4,
        Undef { .. } => 5,
        Mov { .. } => 6,
        Bin { .. } => 7,
        Neg { .. } => 8,
        Not { .. } => 9,
        Cmp { .. } => 10,
        ICmp { .. } => 11,
        FCmp { .. } => 12,
        Select { .. } => 13,
        Cast { .. } => 14,
        Phi { .. } => 15,
        Extract { .. } => 16,
        Checked { .. } => 17,
        Alloca { .. } => 18,
        VolatileLoad { .. } => 19,
        VolatileStore { .. } => 20,
        Load { .. } => 21,
        Store { .. } => 22,
        Uninit { .. } => 23,
        Gep { .. } => 24,
        Memcpy { .. } => 25,
        Memset { .. } => 26,
        NullFunction { .. } => 27,
        FunctionAddr { .. } => 28,
        Call { .. } => 29,
        TrapIf { .. } => 30,
    };
    let sub = match i {
        Bin { op, .. } => match op {
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
        },
        Cast { op, .. } => match op {
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
        },
        _ => 0,
    };
    let pointer = match i {
        Cmp { ty: Type::Ptr(_), .. } | ICmp { ty: Type::Ptr(_), .. } => 1,
        _ => 0,
    };
    run(op, sub, pointer, 0)
}

pub(crate) fn terminator(t: &Terminator) -> Result<Effects, &'static str> {
    run(
        match t {
            Terminator::Br { .. } => 0,
            Terminator::CBr { .. } => 1,
            Terminator::Switch { .. } => 2,
            Terminator::Ret { .. } => 3,
            Terminator::Trap { .. } => 4,
        },
        0,
        0,
        1,
    )
}

fn run(op: u64, sub: u64, pointer: u64, terminator: u64) -> Result<Effects, &'static str> {
    let mut mask = u64::MAX;
    let status = unsafe { whale_ir_effects(op, sub, pointer, terminator, &mut mask) };
    if status != 0 {
        return Err("effect classification transport");
    }
    decode(mask).ok_or("effect classification transport")
}
