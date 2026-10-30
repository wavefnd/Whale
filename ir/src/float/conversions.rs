// SPDX-License-Identifier: MPL-2.0
//! Numeric conversion using the arithmetic packer's single nearest-even round.
use super::Format;

use crate::{CastOp, ConstValue, FloatBits, Type};

fn format(ty: &Type) -> Option<Format> {
    Some(Format::of(match ty {
        Type::F16 => FloatBits::F16(0),
        Type::F32 => FloatBits::F32(0),
        Type::F64 => FloatBits::F64(0),
        _ => return None,
    }))
}

/// None is a defined invalid numerical conversion, never a poison value.
pub fn convert(op: &CastOp, src: &Type, dst: &Type, value: &ConstValue) -> Option<ConstValue> {
    use crate::interpreter::integer;
    if !crate::constant::valid_constant(src, value) {
        return None;
    }
    let src_float = format(src);
    let dst_float = format(dst);
    let src_integer = integer::shape(src);
    let dst_integer = integer::shape(dst);
    let valid = match op {
        CastOp::FExt => src_float.zip(dst_float).is_some_and(|(s, d)| s.width < d.width),
        CastOp::FTrunc => src_float.zip(dst_float).is_some_and(|(s, d)| s.width > d.width),
        CastOp::IToF_S => src_integer.is_some_and(|(_, signed)| signed) && dst_float.is_some(),
        CastOp::IToF_U => src_integer.is_some_and(|(_, signed)| !signed) && dst_float.is_some(),
        CastOp::FToI_S => src_float.is_some() && dst_integer.is_some_and(|(_, signed)| signed),
        CastOp::FToI_U => src_float.is_some() && dst_integer.is_some_and(|(_, signed)| !signed),
        _ => false,
    };
    if !valid {
        return None;
    }
    match op {
        CastOp::FExt | CastOp::FTrunc => {
            let ConstValue::F(value) = value else { return None };
            let parts = Format::of(*value).unpack(*value);
            let f = dst_float?;
            Some(ConstValue::F(if parts.nan {
                f.nan()
            } else if parts.infinity {
                f.special(parts.negative, true)
            } else {
                f.pack(parts.negative, parts.sig, parts.exp)
            }))
        },
        CastOp::IToF_S | CastOp::IToF_U => {
            let (bits, signed) = src_integer?;
            let raw = integer::raw(value, bits);
            let negative = signed && raw & (1u128 << (bits - 1)) != 0;
            let magnitude = if negative { 0u128.wrapping_sub(raw) } else { raw };
            // Mask after taking two's-complement magnitude for narrow negatives.
            let magnitude = if bits < 128 { magnitude & ((1u128 << bits) - 1) } else { magnitude };
            Some(ConstValue::F(dst_float?.pack(negative, magnitude, 0)))
        },
        CastOp::FToI_S | CastOp::FToI_U => {
            let ConstValue::F(value) = value else { return None };
            let p = Format::of(*value).unpack(*value);
            if p.nan || p.infinity {
                return None;
            }
            let magnitude = if p.sig == 0 {
                0
            } else if p.exp >= 0 {
                let shift = p.exp as u32;
                if shift >= 128 || p.sig > u128::MAX >> shift {
                    return None;
                }
                p.sig << shift
            } else {
                p.sig.checked_shr((-p.exp) as u32).unwrap_or(0)
            };
            let (bits, signed) = dst_integer?;
            let maximum = if signed {
                (1u128 << (bits - 1)) - u128::from(!p.negative)
            } else if bits == 128 {
                u128::MAX
            } else {
                (1u128 << bits) - 1
            };
            if (p.negative && !signed && magnitude != 0) || magnitude > maximum {
                return None;
            }
            Some(integer::pack(
                dst,
                if p.negative { 0u128.wrapping_sub(magnitude) } else { magnitude },
            ))
        },
        _ => None,
    }
}
