// SPDX-License-Identifier: MPL-2.0

use crate::{BinOp, CmpOp, Type};

use super::{frontend, LowerError};

pub(crate) fn socket_type_to_whale(t: &frontend::TypeRef) -> Result<Type, LowerError> {
    use frontend::TypeRef as S;

    Ok(match t {
        S::Void => Type::Void,
        S::Bool => Type::I1,

        S::Int { bits, signed } => int_type(*bits, *signed)?,
        S::Float { bits } => float_type(*bits)?,

        S::Ptr(inner) => Type::Ptr(Box::new(socket_type_to_whale(inner)?)),
        S::Array { elem, len } => Type::Array(Box::new(socket_type_to_whale(elem)?), *len),

        S::Opaque(name) => {
            return Err(LowerError::UnsupportedType(format!("opaque {name}")));
        }
    })
}

pub(crate) fn int_type(bits: u16, signed: bool) -> Result<Type, LowerError> {
    Ok(match (bits, signed) {
        (1, _) => Type::I1,

        (8, true) => Type::I8,
        (16, true) => Type::I16,
        (32, true) => Type::I32,
        (64, true) => Type::I64,
        (128, true) => Type::I128,

        (8, false) => Type::U8,
        (16, false) => Type::U16,
        (32, false) => Type::U32,
        (64, false) => Type::U64,
        (128, false) => Type::U128,

        _ => {
            return Err(LowerError::UnsupportedType(format!(
                "int bits={bits} signed={signed}"
            )))
        }
    })
}

pub(crate) fn float_type(bits: u16) -> Result<Type, LowerError> {
    Ok(match bits {
        16 => Type::F16,
        32 => Type::F32,
        64 => Type::F64,
        _ => return Err(LowerError::UnsupportedType(format!("float bits={bits}"))),
    })
}

pub(crate) fn map_binop(op: frontend::BinOpRef, ty: &Type) -> Result<BinOp, LowerError> {
    if is_float(ty) {
        Ok(match op {
            frontend::BinOpRef::Add => BinOp::FAdd,
            frontend::BinOpRef::Sub => BinOp::FSub,
            frontend::BinOpRef::Mul => BinOp::FMul,
        })
    } else if is_int_like(ty) {
        Ok(match op {
            frontend::BinOpRef::Add => BinOp::Add,
            frontend::BinOpRef::Sub => BinOp::Sub,
            frontend::BinOpRef::Mul => BinOp::Mul,
        })
    } else {
        Err(LowerError::UnsupportedExpr)
    }
}

pub(crate) fn align_of(ty: &Type, ptr_bits: u32) -> u32 {
    match ty {
        Type::Void => 1,

        Type::I1 | Type::I8 | Type::U8 => 1,
        Type::I16 | Type::U16 | Type::F16 => 2,
        Type::I32 | Type::U32 | Type::F32 => 4,
        Type::I64 | Type::U64 | Type::F64 => 8,
        Type::I128 | Type::U128 => 16,

        Type::Ptr(_) => (ptr_bits / 8).max(1),

        Type::Array(elem, _) => align_of(elem, ptr_bits),

        Type::Struct(fields) | Type::Tuple(fields) => fields
            .iter()
            .map(|t| align_of(t, ptr_bits))
            .max()
            .unwrap_or(1),
    }
}

fn is_float(ty: &Type) -> bool {
    matches!(ty, Type::F16 | Type::F32 | Type::F64)
}

fn is_int_like(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I1
            | Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::I128
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::U128
    )
}

pub(crate) fn map_cmp(op: frontend::CmpOpRef, ty: &Type) -> Result<CmpOp, LowerError> {
    if is_float(ty) {
        return Ok(match op {
            frontend::CmpOpRef::Eq => CmpOp::FEq,
            frontend::CmpOpRef::Ne => CmpOp::FNe,
            frontend::CmpOpRef::Lt => CmpOp::FLt,
            frontend::CmpOpRef::Le => CmpOp::FLe,
            frontend::CmpOpRef::Gt => CmpOp::FGt,
            frontend::CmpOpRef::Ge => CmpOp::FGe,
        });
    }

    if is_int_like(ty) {
        if matches!(ty, Type::I1) {
            return Ok(match op {
                frontend::CmpOpRef::Eq => CmpOp::Eq,
                frontend::CmpOpRef::Ne => CmpOp::Ne,
                _ => return Err(LowerError::UnsupportedExpr),
            });
        }

        let signed = matches!(ty, Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128);

        return Ok(match (op, signed) {
            (frontend::CmpOpRef::Eq, _) => CmpOp::Eq,
            (frontend::CmpOpRef::Ne, _) => CmpOp::Ne,

            (frontend::CmpOpRef::Lt, true) => CmpOp::SLt,
            (frontend::CmpOpRef::Le, true) => CmpOp::SLe,
            (frontend::CmpOpRef::Gt, true) => CmpOp::SGt,
            (frontend::CmpOpRef::Ge, true) => CmpOp::SGe,

            (frontend::CmpOpRef::Lt, false) => CmpOp::ULt,
            (frontend::CmpOpRef::Le, false) => CmpOp::ULe,
            (frontend::CmpOpRef::Gt, false) => CmpOp::UGt,
            (frontend::CmpOpRef::Ge, false) => CmpOp::UGe,
        });
    }

    Err(LowerError::UnsupportedExpr)
}
