// SPDX-License-Identifier: MPL-2.0
//! Defined IEEE arithmetic. Wave owns the product path; the explicit Rust
//! reference remains available for conformance during migration.
use crate::{BinOp, CastOp, ConstValue, FloatBackendError, FloatBits, Type};

#[cfg(any(not(whale_wave_ir), test))]
#[path = "arithmetic_reference.rs"]
pub(crate) mod reference;

pub fn try_arithmetic(
    op: &BinOp,
    lhs: FloatBits,
    rhs: FloatBits,
) -> Result<Option<FloatBits>, FloatBackendError> {
    #[cfg(whale_wave_ir)]
    {
        super::wave::arithmetic(op, lhs, rhs)
    }
    #[cfg(not(whale_wave_ir))]
    {
        Ok(reference::arithmetic(op, lhs, rhs))
    }
}

pub fn arithmetic(op: &BinOp, lhs: FloatBits, rhs: FloatBits) -> Option<FloatBits> {
    try_arithmetic(op, lhs, rhs).expect("Wave floating arithmetic backend invariant")
}

pub fn try_negate(value: FloatBits) -> Result<FloatBits, FloatBackendError> {
    #[cfg(whale_wave_ir)]
    {
        super::wave::negate(value)
    }
    #[cfg(not(whale_wave_ir))]
    {
        Ok(reference::negate(value))
    }
}

pub fn negate(value: FloatBits) -> FloatBits {
    try_negate(value).expect("Wave floating negation backend invariant")
}

pub fn try_convert(
    op: &CastOp,
    src: &Type,
    dst: &Type,
    value: &ConstValue,
) -> Result<Option<ConstValue>, FloatBackendError> {
    #[cfg(whale_wave_ir)]
    {
        super::wave::convert(op, src, dst, value)
    }
    #[cfg(not(whale_wave_ir))]
    {
        Ok(reference::convert(op, src, dst, value))
    }
}

pub fn convert(op: &CastOp, src: &Type, dst: &Type, value: &ConstValue) -> Option<ConstValue> {
    try_convert(op, src, dst, value).expect("Wave floating conversion backend invariant")
}

#[cfg(all(test, whale_wave_ir))]
mod tests {
    use super::*;

    #[test]
    fn wave_matches_retained_ieee_reference_for_random_storage_and_operations() {
        let mut state = 0x0003_5301_2864_u64;
        for _ in 0..1000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let a = state;
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let b = state;
            for (left, right) in [
                (FloatBits::F16(a as u16), FloatBits::F16(b as u16)),
                (FloatBits::F32(a as u32), FloatBits::F32(b as u32)),
                (FloatBits::F64(a), FloatBits::F64(b)),
            ] {
                for op in [BinOp::FAdd, BinOp::FSub, BinOp::FMul, BinOp::FDiv, BinOp::FRem] {
                    assert_eq!(
                        try_arithmetic(&op, left, right).unwrap(),
                        reference::arithmetic(&op, left, right),
                        "{op:?} {left:?} {right:?}"
                    );
                }
                assert_eq!(try_negate(left).unwrap(), reference::negate(left));
                for destination in [
                    Type::F16,
                    Type::F32,
                    Type::F64,
                    Type::I1,
                    Type::U1,
                    Type::I32,
                    Type::U32,
                    Type::I128,
                    Type::U128,
                ] {
                    let source = match left {
                        FloatBits::F16(_) => Type::F16,
                        FloatBits::F32(_) => Type::F32,
                        FloatBits::F64(_) => Type::F64,
                    };
                    for op in [CastOp::FExt, CastOp::FTrunc, CastOp::FToI_S, CastOp::FToI_U] {
                        let value = ConstValue::F(left);
                        assert_eq!(
                            try_convert(&op, &source, &destination, &value).unwrap(),
                            reference::convert(&op, &source, &destination, &value)
                        );
                    }
                }
            }
        }
    }
}
