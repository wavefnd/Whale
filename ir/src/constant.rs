// SPDX-License-Identifier: MPL-2.0

use crate::{ConstValue, Type};

/// Check a literal's category and range without rounding or wrapping its value.
/// Floating payload widths must match the declared type.
pub(crate) fn valid_constant(ty: &Type, value: &ConstValue) -> bool {
    #[cfg(whale_wave_ir)]
    {
        crate::constant_wave::valid_literal(ty, value)
    }
    #[cfg(not(whale_wave_ir))]
    {
        valid_constant_reference(ty, value)
    }
}

#[cfg(any(not(whale_wave_ir), test))]
pub(crate) fn valid_constant_reference(ty: &Type, value: &ConstValue) -> bool {
    use Type::*;
    match (ty, value) {
        (Bool, ConstValue::Bool(_)) => true,
        (F16, ConstValue::F(crate::FloatBits::F16(_)))
        | (F32, ConstValue::F(crate::FloatBits::F32(_)))
        | (F64, ConstValue::F(crate::FloatBits::F64(_))) => true,
        (I128, ConstValue::I(_)) | (U128, ConstValue::U(_)) => true,
        (I1 | I8 | I16 | I32 | I64, ConstValue::I(v)) => {
            let bits = match ty {
                I1 => 1,
                I8 => 8,
                I16 => 16,
                I32 => 32,
                I64 => 64,
                _ => unreachable!(),
            };
            let limit = 1i128 << (bits - 1);
            (-limit..limit).contains(v)
        },
        (U1 | U8 | U16 | U32 | U64, ConstValue::U(v)) => {
            let bits = match ty {
                U1 => 1,
                U8 => 8,
                U16 => 16,
                U32 => 32,
                U64 => 64,
                _ => unreachable!(),
            };
            *v < (1u128 << bits)
        },
        _ => false,
    }
}
