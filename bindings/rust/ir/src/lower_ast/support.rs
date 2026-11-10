// SPDX-License-Identifier: MPL-2.0

use crate::{BinOp, CmpOp, Type};

use super::{frontend, LowerError};

pub(crate) fn socket_type_to_whale(t: &frontend::TypeRef) -> Result<Type, LowerError> {
    use frontend::TypeRef as S;

    Ok(match t {
        S::Void => Type::Void,
        S::Bool => Type::Bool,

        S::Int { bits, signed } => int_type(*bits, *signed)?,
        S::Float { bits } => float_type(*bits)?,

        S::FnPtr(sig) => Type::FnPtr(Box::new(signature_to_whale(sig)?)),
        S::Ptr(inner) => Type::Ptr(Box::new(socket_type_to_whale(inner)?)),
        S::Array { elem, len } => Type::Array(Box::new(socket_type_to_whale(elem)?), *len),
        S::Struct { fields } => {
            let mut names = std::collections::HashSet::new();
            let mut types = Vec::new();
            for f in fields {
                if f.name.is_empty() || !names.insert(&f.name) {
                    return Err(LowerError::Aggregate("duplicate or empty struct field".into()));
                }
                let ty = socket_type_to_whale(&f.ty)?;
                if !scalar_storage(&ty) {
                    return Err(LowerError::Aggregate(
                        "initial structs require scalar fields".into(),
                    ));
                }
                types.push(ty);
            }
            Type::Struct(types)
        },
        S::Opaque(name) => {
            return Err(LowerError::UnsupportedType(format!("opaque {name}")));
        },
    })
}

pub(crate) fn int_type(bits: u16, signed: bool) -> Result<Type, LowerError> {
    Ok(match (bits, signed) {
        (1, true) => Type::I1,
        (1, false) => Type::U1,

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

        _ => return Err(LowerError::UnsupportedType(format!("int bits={bits} signed={signed}"))),
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
    use frontend::BinOpRef as B;
    if is_float(ty) {
        Ok(match op {
            B::Add => BinOp::FAdd,
            B::Sub => BinOp::FSub,
            B::Mul => BinOp::FMul,
            B::Div => BinOp::FDiv,
            B::Rem => BinOp::FRem,
            _ => return Err(LowerError::UnsupportedExpr),
        })
    } else if is_int_like(ty) {
        let signed = is_signed(ty);
        Ok(match op {
            B::Add => BinOp::Add,
            B::Sub => BinOp::Sub,
            B::Mul => BinOp::Mul,
            B::BitAnd => BinOp::And,
            B::BitOr => BinOp::Or,
            B::BitXor => BinOp::Xor,
            B::Div if signed => BinOp::SDiv,
            B::Div => BinOp::UDiv,
            B::Rem if signed => BinOp::SRem,
            B::Rem => BinOp::URem,
            B::ShiftLeft => BinOp::Shl,
            B::ShiftRight if signed => BinOp::AShr,
            B::ShiftRight => BinOp::LShr,
            B::LogicalAnd | B::LogicalOr => return Err(LowerError::UnsupportedExpr),
        })
    } else {
        Err(LowerError::UnsupportedExpr)
    }
}

fn is_signed(ty: &Type) -> bool {
    matches!(ty, Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128)
}

fn is_float(ty: &Type) -> bool {
    matches!(ty, Type::F16 | Type::F32 | Type::F64)
}

pub(crate) fn is_int_like(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I1
            | Type::U1
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

    if matches!(ty, Type::Bool) || is_int_like(ty) {
        if matches!(ty, Type::Bool) {
            return Ok(match op {
                frontend::CmpOpRef::Eq => CmpOp::Eq,
                frontend::CmpOpRef::Ne => CmpOp::Ne,
                _ => return Err(LowerError::UnsupportedExpr),
            });
        }

        let signed =
            matches!(ty, Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128);

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

pub(crate) fn signature_to_whale(
    sig: &frontend::SignatureRef,
) -> Result<crate::FunctionSignature, LowerError> {
    let sig = crate::FunctionSignature {
        params: sig.params.iter().map(socket_type_to_whale).collect::<Result<_, _>>()?,
        ret: socket_type_to_whale(&sig.ret)?,
        convention: sig.convention,
        variadic: sig.variadic,
    };
    crate::function::validate_signature(&sig).map_err(LowerError::Call)?;
    Ok(sig)
}

pub(super) fn scalar_storage(ty: &Type) -> bool {
    is_int_like(ty)
        || matches!(
            ty,
            Type::Bool | Type::F16 | Type::F32 | Type::F64 | Type::Ptr(_) | Type::FnPtr(_)
        )
}
