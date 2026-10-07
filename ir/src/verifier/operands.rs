use super::VerifyError;
use crate::{BinOp, CmpOp, ConstValue, Function, ICmpPred, Instruction, Type, ValueId};

fn is_signed(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128
    )
}

fn is_unsigned(ty: &Type) -> bool {
    matches!(
        ty,
        Type::U1 | Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128
    )
}

pub(super) fn is_integer(ty: &Type) -> bool {
    is_signed(ty) || is_unsigned(ty)
}

fn is_float(ty: &Type) -> bool {
    matches!(ty, Type::F16 | Type::F32 | Type::F64)
}

fn integer_width(ty: &Type) -> Option<u32> {
    match ty {
        Type::I1 | Type::U1 => Some(1),
        Type::I8 | Type::U8 => Some(8),
        Type::I16 | Type::U16 => Some(16),
        Type::I32 | Type::U32 => Some(32),
        Type::I64 | Type::U64 => Some(64),
        Type::I128 | Type::U128 => Some(128),
        _ => None,
    }
}

fn float_width(ty: &Type) -> Option<u32> {
    match ty {
        Type::F16 => Some(16),
        Type::F32 => Some(32),
        Type::F64 => Some(64),
        _ => None,
    }
}

fn valid_cast(op: &crate::CastOp, source: &Type, destination: &Type) -> bool {
    use crate::CastOp::*;
    let integer_pair = integer_width(source).zip(integer_width(destination));
    let float_pair = float_width(source).zip(float_width(destination));
    match op {
        // Bool has a numerical 0/1 conversion, not signed i1's sign bit.
        ZExt if *source == Type::Bool => is_integer(destination) && *destination != Type::I1,
        ZExt | SExt => integer_pair.is_some_and(|(s, d)| s < d),
        Trunc => integer_pair.is_some_and(|(s, d)| s > d),
        FExt => float_pair.is_some_and(|(s, d)| s < d),
        FTrunc => float_pair.is_some_and(|(s, d)| s > d),
        IToF_S => is_signed(source) && is_float(destination),
        IToF_U => is_unsigned(source) && is_float(destination),
        FToI_S => is_float(source) && is_signed(destination),
        FToI_U => is_float(source) && is_unsigned(destination),
        Bitcast => {
            integer_width(source)
                .or_else(|| float_width(source))
                .zip(integer_width(destination).or_else(|| float_width(destination)))
                .is_some_and(|(s, d)| s == d)
                || matches!((source, destination), (Type::Ptr(_), Type::Ptr(_)))
        }
        // These are type checks only. Runtime address validity and metadata
        // cannot be established by an integer/pointer cast (see memory rules).
        PtrToInt => matches!(source, Type::Ptr(_)) && is_integer(destination),
        IntToPtr => is_integer(source) && matches!(destination, Type::Ptr(_)),
    }
}

fn supports_equality(ty: &Type) -> bool {
    is_integer(ty) || matches!(ty, Type::Bool | Type::Ptr(_))
}

// A pointer to void may describe an opaque address, but void itself has no
// stored value. Concrete aggregate layout and dynamic access checks are separate.
pub(super) fn is_storable(ty: &Type) -> bool {
    match ty {
        Type::Void => false,
        Type::Array(element, _) => is_storable(element),
        Type::Struct(fields) | Type::Tuple(fields) => fields.iter().all(is_storable),
        _ => true,
    }
}

pub(super) fn verify_type_category(
    f: &Function,
    operation: &'static str,
    ty: &Type,
    valid: bool,
) -> Result<(), VerifyError> {
    if valid {
        Ok(())
    } else {
        Err(VerifyError::InvalidInstructionType {
            func: f.name.clone(),
            operation,
            ty: ty.clone(),
        })
    }
}

pub(super) fn verify_operand(
    f: &Function,
    value: ValueId,
    expected: &Type,
) -> Result<(), VerifyError> {
    let got = f
        .value_type(value)
        .ok_or_else(|| VerifyError::MissingValueType {
            func: f.name.clone(),
            value,
        })?;
    if got != expected {
        return Err(VerifyError::OperandTypeMismatch {
            func: f.name.clone(),
            value,
            expected: expected.clone(),
            got: got.clone(),
        });
    }
    Ok(())
}

fn verify_alignment(f: &Function, align: u32) -> Result<(), VerifyError> {
    if !align.is_power_of_two() {
        return Err(VerifyError::InvalidMemoryAlignment {
            func: f.name.clone(),
            align,
        });
    }
    Ok(())
}

fn verify_gep(
    f: &Function,
    dst: ValueId,
    dst_ty: &Type,
    base: ValueId,
    indices: &[ValueId],
) -> Result<(), VerifyError> {
    let invalid = |index, reason| VerifyError::InvalidGep {
        func: f.name.clone(),
        value: dst,
        index,
        reason,
    };
    let base_ty = f
        .value_type(base)
        .ok_or_else(|| VerifyError::MissingValueType {
            func: f.name.clone(),
            value: base,
        })?;
    let Type::Ptr(mut selected) = base_ty.clone() else {
        return Err(invalid(None, "base must be a pointer"));
    };
    if !indices.is_empty() && !is_storable(&selected) {
        return Err(invalid(
            None,
            "element addressing requires a concrete stored type",
        ));
    }
    for (position, value) in indices.iter().enumerate() {
        let ty = f
            .value_type(*value)
            .ok_or_else(|| VerifyError::MissingValueType {
                func: f.name.clone(),
                value: *value,
            })?;
        if !is_integer(ty) {
            return Err(invalid(Some(position), "index must be an integer"));
        }
        // Pointer arithmetic selects another instance of the original pointee.
        if position == 0 {
            continue;
        }
        selected = match *selected {
            Type::Array(element, _) => element,
            Type::Struct(fields) | Type::Tuple(fields) => {
                let ordinal = f
                    .blocks
                    .iter()
                    .flat_map(|b| &b.instructions)
                    .find_map(|ins| {
                        if let Instruction::Const {
                            dst,
                            value: constant,
                            ..
                        }
                        | Instruction::ConstDecl {
                            dst,
                            value: constant,
                            ..
                        } = ins
                        {
                            if dst == value {
                                return match constant {
                                    ConstValue::I(n) => usize::try_from(*n).ok(),
                                    ConstValue::U(n) => usize::try_from(*n).ok(),
                                    _ => None,
                                };
                            }
                        }
                        None
                    })
                    .ok_or_else(|| {
                        invalid(
                            Some(position),
                            "field index must be a nonnegative integer constant",
                        )
                    })?;
                Box::new(
                    fields
                        .get(ordinal)
                        .ok_or_else(|| {
                            invalid(Some(position), "field index is outside the aggregate type")
                        })?
                        .clone(),
                )
            }
            _ => return Err(invalid(
                Some(position),
                "further indices require an array, struct, or tuple; GEP does not load pointers",
            )),
        };
    }
    let expected = Type::Ptr(selected);
    if dst_ty != &expected {
        return Err(VerifyError::OperandTypeMismatch {
            func: f.name.clone(),
            value: dst,
            expected,
            got: dst_ty.clone(),
        });
    }
    Ok(())
}

pub(super) fn verify_instruction(f: &Function, ins: &Instruction) -> Result<(), VerifyError> {
    match ins {
        Instruction::Cast {
            dst,
            op,
            src_ty,
            src,
            dst_ty,
        } => {
            verify_operand(f, *src, src_ty)?;
            if !valid_cast(op, src_ty, dst_ty) {
                return Err(VerifyError::InvalidCast {
                    func: f.name.clone(),
                    value: *dst,
                    op: op.clone(),
                    source: src_ty.clone(),
                    destination: dst_ty.clone(),
                });
            }
        }
        Instruction::Checked {
            op, ty, lhs, rhs, ..
        } => {
            let valid = match op {
                crate::CheckedOp::SAdd | crate::CheckedOp::SSub | crate::CheckedOp::SMul => {
                    is_signed(ty)
                }
                crate::CheckedOp::UAdd | crate::CheckedOp::USub | crate::CheckedOp::UMul => {
                    is_unsigned(ty)
                }
            };
            verify_type_category(f, "checked arithmetic", ty, valid)?;
            verify_operand(f, *lhs, ty)?;
            verify_operand(f, *rhs, ty)?;
        }
        Instruction::Bin {
            op, ty, lhs, rhs, ..
        } => {
            let valid = match op {
                BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv | BinOp::FRem => is_float(ty),
                BinOp::SDiv | BinOp::SRem => is_signed(ty),
                BinOp::UDiv | BinOp::URem => is_unsigned(ty),
                _ => is_integer(ty),
            };
            verify_type_category(f, "binary operation", ty, valid)?;
            verify_operand(f, *lhs, ty)?;
            verify_operand(f, *rhs, ty)?;
        }
        Instruction::Not { ty, src, .. } => {
            verify_type_category(f, "not", ty, is_integer(ty))?;
            verify_operand(f, *src, ty)?;
        }
        Instruction::Mov { ty, src, .. } => {
            verify_type_category(f, "mov", ty, is_storable(ty))?;
            verify_operand(f, *src, ty)?;
        }
        Instruction::Cmp {
            op, ty, lhs, rhs, ..
        } => {
            let valid = match op {
                CmpOp::Eq | CmpOp::Ne => supports_equality(ty),
                CmpOp::SLt | CmpOp::SLe | CmpOp::SGt | CmpOp::SGe => is_signed(ty),
                CmpOp::ULt | CmpOp::ULe | CmpOp::UGt | CmpOp::UGe => is_unsigned(ty),
                _ => is_float(ty),
            };
            verify_type_category(f, "comparison", ty, valid)?;
            verify_operand(f, *lhs, ty)?;
            verify_operand(f, *rhs, ty)?;
        }
        Instruction::ICmp {
            pred, ty, lhs, rhs, ..
        } => {
            let valid = match pred {
                ICmpPred::Eq | ICmpPred::Ne => supports_equality(ty),
                ICmpPred::Slt | ICmpPred::Sle | ICmpPred::Sgt | ICmpPred::Sge => is_signed(ty),
                _ => is_unsigned(ty),
            };
            verify_type_category(f, "integer comparison", ty, valid)?;
            verify_operand(f, *lhs, ty)?;
            verify_operand(f, *rhs, ty)?;
        }
        Instruction::FCmp { ty, lhs, rhs, .. } => {
            verify_type_category(f, "floating comparison", ty, is_float(ty))?;
            verify_operand(f, *lhs, ty)?;
            verify_operand(f, *rhs, ty)?;
        }
        Instruction::Select {
            ty,
            cond,
            on_true,
            on_false,
            ..
        } => {
            super::verify_condition(f, *cond)?;
            verify_type_category(f, "select", ty, is_storable(ty))?;
            verify_operand(f, *on_true, ty)?;
            verify_operand(f, *on_false, ty)?;
        }
        Instruction::Extract {
            dst_ty,
            tuple,
            index,
            ..
        } => {
            let field = match f.value_type(*tuple) {
                Some(Type::Tuple(fields)) => fields.get(*index as usize),
                _ => None,
            };
            verify_type_category(f, "extract", dst_ty, field == Some(dst_ty))?;
        }
        Instruction::Alloca { ty, align, .. } => {
            verify_alignment(f, *align)?;
            verify_type_category(f, "alloca", ty, is_storable(ty))?;
        }
        Instruction::Load { ty, ptr, align, .. } => {
            verify_alignment(f, *align)?;
            verify_type_category(f, "load", ty, is_storable(ty))?;
            verify_operand(f, *ptr, &Type::ptr_to(ty.clone()))?;
        }
        Instruction::Store {
            ty,
            ptr,
            value,
            align,
        } => {
            verify_alignment(f, *align)?;
            verify_type_category(f, "store", ty, is_storable(ty))?;
            verify_operand(f, *ptr, &Type::ptr_to(ty.clone()))?;
            verify_operand(f, *value, ty)?;
        }
        Instruction::Gep {
            dst,
            dst_ty,
            base_ptr,
            indices,
        } => {
            verify_gep(f, *dst, dst_ty, *base_ptr, indices)?;
        }
        // These operand types are already explicit in the textual IR printer.
        // Bounds, lifetimes and overlapping-copy behavior remain separate rules.
        Instruction::Memcpy { dst, src, n, align } => {
            verify_alignment(f, *align)?;
            verify_operand(f, *dst, &Type::ptr_to(Type::U8))?;
            verify_operand(f, *src, &Type::ptr_to(Type::U8))?;
            verify_operand(f, *n, &Type::U64)?;
        }
        Instruction::Memset { dst, val, n, align } => {
            verify_alignment(f, *align)?;
            verify_operand(f, *dst, &Type::ptr_to(Type::U8))?;
            verify_operand(f, *val, &Type::U8)?;
            verify_operand(f, *n, &Type::U64)?;
        }
        _ => {}
    }
    Ok(())
}
