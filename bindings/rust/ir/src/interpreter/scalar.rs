// SPDX-License-Identifier: MPL-2.0
//! Storage reinterpretation only: no floating-point arithmetic or conversion.
#[cfg(not(whale_wave_ir))]
use super::integer;

use crate::Type;

#[cfg(not(whale_wave_ir))]
use crate::{ConstValue, FloatBits};

pub(super) fn float_type(ty: &Type) -> bool {
    matches!(ty, Type::F16 | Type::F32 | Type::F64)
}

#[cfg(not(whale_wave_ir))]
pub(super) fn raw(ty: &Type, value: &ConstValue) -> u128 {
    match value {
        ConstValue::F(bits) => u128::from(bits.bits()),
        _ => integer::raw(value, integer::shape(ty).map_or(1, |s| s.0)),
    }
}

#[cfg(not(whale_wave_ir))]
pub(super) fn pack(ty: &Type, bits: u128) -> ConstValue {
    let float = match ty {
        Type::F16 => FloatBits::F16(bits as u16),
        Type::F32 => FloatBits::F32(bits as u32),
        Type::F64 => FloatBits::F64(bits as u64),
        _ => return integer::pack(ty, bits),
    };
    ConstValue::F(float)
}

#[cfg(not(whale_wave_ir))]
pub(super) fn compare(predicate: &crate::FCmpPred, lhs: &ConstValue, rhs: &ConstValue) -> bool {
    let (ConstValue::F(lhs), ConstValue::F(rhs)) = (lhs, rhs) else {
        unreachable!("verified floating operands")
    };
    lhs.compare(*rhs, predicate).expect("verified matching floating widths")
}
