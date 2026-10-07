// SPDX-License-Identifier: MPL-2.0

use super::TrapReason;
use crate::{BinOp, CastOp, CheckedOp, ConstValue, Type};

pub(super) fn shape(ty: &Type) -> Option<(u32, bool)> {
    use Type::*;
    Some(match ty {
        I1 => (1, true),
        U1 => (1, false),
        I8 => (8, true),
        U8 => (8, false),
        I16 => (16, true),
        U16 => (16, false),
        I32 => (32, true),
        U32 => (32, false),
        I64 => (64, true),
        U64 => (64, false),
        I128 => (128, true),
        U128 => (128, false),
        _ => return None,
    })
}
fn mask(bits: u32) -> u128 {
    if bits == 128 {
        u128::MAX
    } else {
        (1u128 << bits) - 1
    }
}
pub(super) fn raw(value: &ConstValue, bits: u32) -> u128 {
    (match value {
        ConstValue::I(v) => *v as u128,
        ConstValue::U(v) => *v,
        ConstValue::Bool(v) => u128::from(*v),
        ConstValue::F(_) => unreachable!("scalar subset checked before execution"),
    }) & mask(bits)
}
fn signed(value: u128, bits: u32) -> i128 {
    if bits < 128 && value & (1u128 << (bits - 1)) != 0 {
        (value | !mask(bits)) as i128
    } else {
        value as i128
    }
}
pub(super) fn pack(ty: &Type, value: u128) -> ConstValue {
    let (bits, is_signed) = shape(ty).expect("integer subset checked");
    let value = value & mask(bits);
    if is_signed {
        ConstValue::I(signed(value, bits))
    } else {
        ConstValue::U(value)
    }
}
pub(super) fn bin(
    op: &BinOp,
    ty: &Type,
    lhs: &ConstValue,
    rhs: &ConstValue,
) -> Result<ConstValue, TrapReason> {
    let (bits, _) = shape(ty).expect("integer subset checked");
    let a = raw(lhs, bits);
    let b = raw(rhs, bits);
    // Reduce the complete unsigned N-bit pattern before narrowing to a shift count.
    let count = (b % u128::from(bits)) as u32;
    let value = match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        BinOp::UDiv | BinOp::SDiv if b == 0 => return Err(TrapReason::DivisionByZero),
        BinOp::URem | BinOp::SRem if b == 0 => return Err(TrapReason::RemainderByZero),
        BinOp::UDiv => a / b,
        BinOp::URem => a % b,
        BinOp::SDiv => signed(a, bits).wrapping_div(signed(b, bits)) as u128,
        BinOp::SRem => signed(a, bits).wrapping_rem(signed(b, bits)) as u128,
        BinOp::And => a & b,
        BinOp::Or => a | b,
        BinOp::Xor => a ^ b,
        BinOp::Shl => a << count,
        BinOp::LShr => a >> count,
        BinOp::AShr => (signed(a, bits) >> count) as u128,
        _ => unreachable!("integer subset checked"),
    };
    Ok(pack(ty, value))
}
pub(super) fn checked(
    op: &CheckedOp,
    ty: &Type,
    lhs: &ConstValue,
    rhs: &ConstValue,
) -> (ConstValue, bool) {
    let (bits, _) = shape(ty).expect("integer subset checked");
    let a = raw(lhs, bits);
    let b = raw(rhs, bits);
    let (wrapped, overflow) = match op {
        CheckedOp::UAdd => {
            let (sum, carry) = a.overflowing_add(b);
            (sum, carry || sum > mask(bits))
        }
        CheckedOp::USub => (a.wrapping_sub(b), a < b),
        CheckedOp::UMul => (
            a.wrapping_mul(b),
            a.checked_mul(b).is_none_or(|v| v > mask(bits)),
        ),
        CheckedOp::SAdd | CheckedOp::SSub | CheckedOp::SMul => {
            let sa = signed(a, bits);
            let sb = signed(b, bits);
            let (wrapped, exact) = match op {
                CheckedOp::SAdd => (a.wrapping_add(b), sa.checked_add(sb)),
                CheckedOp::SSub => (a.wrapping_sub(b), sa.checked_sub(sb)),
                _ => (a.wrapping_mul(b), sa.checked_mul(sb)),
            };
            let overflow = exact.is_none_or(|v| {
                bits < 128 && (v < -(1i128 << (bits - 1)) || v >= (1i128 << (bits - 1)))
            });
            (wrapped, overflow)
        }
    };
    (pack(ty, wrapped), overflow)
}
pub(super) fn cast(op: &CastOp, src_ty: &Type, dst_ty: &Type, value: &ConstValue) -> ConstValue {
    let bits = shape(src_ty).map_or(1, |s| s.0);
    let value = raw(value, bits);
    let value = if *op == CastOp::SExt {
        signed(value, bits) as u128
    } else {
        value
    };
    pack(dst_ty, value)
}
pub(super) fn compare(
    pred: &crate::ICmpPred,
    ty: &Type,
    lhs: &ConstValue,
    rhs: &ConstValue,
) -> bool {
    use crate::ICmpPred::*;
    let bits = shape(ty).map_or(1, |s| s.0);
    let a = raw(lhs, bits);
    let b = raw(rhs, bits);
    match pred {
        Eq => a == b,
        Ne => a != b,
        Ult => a < b,
        Ule => a <= b,
        Ugt => a > b,
        Uge => a >= b,
        Slt => signed(a, bits) < signed(b, bits),
        Sle => signed(a, bits) <= signed(b, bits),
        Sgt => signed(a, bits) > signed(b, bits),
        Sge => signed(a, bits) >= signed(b, bits),
    }
}
