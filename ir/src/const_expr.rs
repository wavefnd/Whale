// SPDX-License-Identifier: MPL-2.0

use crate::{ConstValue, GlobalId, Type, ValueId};

/// Resolved constant identity. Local identities belong to the containing function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstRef {
    Global(GlobalId),
    Local(ValueId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstBinaryOp {
    Add,
    Sub,
    Mul,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstCompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A typed compile-time expression, preserved independently of its evaluated
/// result. This tree is not a sequence of runtime instructions.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstExpr {
    pub ty: Type,
    pub kind: ConstExprKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConstExprKind {
    Literal(ConstValue),
    Reference(ConstRef),
    Binary {
        op: ConstBinaryOp,
        left: Box<ConstExpr>,
        right: Box<ConstExpr>,
    },
    Compare {
        op: ConstCompareOp,
        left: Box<ConstExpr>,
        right: Box<ConstExpr>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstEvalError {
    ResourceLimit(crate::LimitError),
    InvalidLiteral,
    TypeMismatch,
    UnsupportedOperation,
    UnknownReference(ConstRef),
    DuplicateDefinition(ConstRef),
    CyclicReference,
    ResultMismatch,
}

impl ConstExpr {
    pub fn literal(ty: Type, value: ConstValue) -> Self {
        Self {
            ty,
            kind: ConstExprKind::Literal(value),
        }
    }

    pub fn references(&self) -> Vec<ConstRef> {
        let mut pending = vec![self];
        let mut references = Vec::new();
        while let Some(expr) = pending.pop() {
            match &expr.kind {
                ConstExprKind::Literal(_) => {}
                ConstExprKind::Reference(reference) => references.push(*reference),
                ConstExprKind::Binary { left, right, .. }
                | ConstExprKind::Compare { left, right, .. } => {
                    pending.push(right);
                    pending.push(left);
                }
            }
        }
        references
    }

    pub fn evaluate(
        &self,
        resolve: &impl Fn(ConstRef) -> Option<(Type, ConstValue)>,
    ) -> Result<ConstValue, ConstEvalError> {
        self.evaluate_with_limits(resolve, crate::IrLimits::default())
    }

    pub fn evaluate_with_limits(
        &self,
        resolve: &impl Fn(ConstRef) -> Option<(Type, ConstValue)>,
        limits: crate::IrLimits,
    ) -> Result<ConstValue, ConstEvalError> {
        let mut budget =
            crate::limits::Budget::new(limits).map_err(ConstEvalError::ResourceLimit)?;
        budget
            .expression(self)
            .map_err(ConstEvalError::ResourceLimit)?;
        // Explicit postorder evaluation preserves left-to-right evaluation and
        // does not recurse on the caller's expression tree.
        let mut pending = vec![(self, false)];
        let mut values = Vec::new();
        while let Some((expr, finish)) = pending.pop() {
            let value = match &expr.kind {
                ConstExprKind::Literal(value) => value.clone(),
                ConstExprKind::Reference(reference) => {
                    let (ty, value) =
                        resolve(*reference).ok_or(ConstEvalError::UnknownReference(*reference))?;
                    if let Err(error) = budget.ty(&ty) {
                        crate::limits::discard_type(ty);
                        return Err(ConstEvalError::ResourceLimit(error));
                    }
                    if ty != expr.ty {
                        return Err(ConstEvalError::TypeMismatch);
                    }
                    value
                }
                ConstExprKind::Binary { left, right, .. }
                | ConstExprKind::Compare { left, right, .. }
                    if !finish =>
                {
                    if left.ty != right.ty
                        || match expr.kind {
                            ConstExprKind::Compare { .. } => expr.ty != Type::Bool,
                            _ => expr.ty != left.ty,
                        }
                    {
                        return Err(ConstEvalError::TypeMismatch);
                    }
                    pending.push((expr, true));
                    pending.push((right, false));
                    pending.push((left, false));
                    continue;
                }
                ConstExprKind::Binary { op, left, .. } => {
                    let right = values.pop().expect("postorder right operand");
                    let left_value = values.pop().expect("postorder left operand");
                    const_bin(*op, &left.ty, &left_value, &right)?
                }
                ConstExprKind::Compare { op, left, .. } => {
                    let right = values.pop().expect("postorder right operand");
                    let left_value = values.pop().expect("postorder left operand");
                    ConstValue::Bool(const_cmp(*op, &left.ty, &left_value, &right)?)
                }
            };
            if !crate::constant::valid_constant(&expr.ty, &value) {
                return Err(ConstEvalError::InvalidLiteral);
            }
            values.push(value);
        }
        Ok(values.pop().expect("root expression result"))
    }
}

pub(crate) fn same_value(left: &ConstValue, right: &ConstValue) -> bool {
    match (left, right) {
        // Compare the stored payload, including signed zero and NaN bits.
        (ConstValue::F(a), ConstValue::F(b)) => a == b,
        _ => left == right,
    }
}

// Preserve the existing scalar constant evaluation rules.
fn ty_int_bits(ty: &Type) -> Option<u32> {
    Some(match ty {
        Type::I1 | Type::U1 => 1,
        Type::I8 | Type::U8 => 8,
        Type::I16 | Type::U16 => 16,
        Type::I32 | Type::U32 => 32,
        Type::I64 | Type::U64 => 64,
        Type::I128 | Type::U128 => 128,
        _ => return None,
    })
}

fn is_signed_int(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128
    )
}

fn wrap_u(v: u128, ty: &Type) -> u128 {
    let bits = ty_int_bits(ty).unwrap_or(128);
    if bits >= 128 {
        return v;
    }
    let mask = (1u128 << bits) - 1;
    v & mask
}

fn wrap_i(v: i128, ty: &Type) -> i128 {
    let bits = ty_int_bits(ty).unwrap_or(128);
    if bits >= 128 {
        return v;
    }
    let mask = (1u128 << bits) - 1;
    let u = (v as u128) & mask;
    let sign = 1u128 << (bits - 1);
    if (u & sign) != 0 {
        // sign-extend
        (u | (!mask)) as i128
    } else {
        u as i128
    }
}

fn const_bin(
    op: ConstBinaryOp,
    ty: &Type,
    l: &ConstValue,
    r: &ConstValue,
) -> Result<ConstValue, ConstEvalError> {
    // float
    if matches!(ty, Type::F16 | Type::F32 | Type::F64) {
        let (ConstValue::F(a), ConstValue::F(b)) = (l, r) else {
            return Err(ConstEvalError::UnsupportedOperation);
        };
        let width = a.width();
        let (a, b) = (a.to_f64(), b.to_f64());
        let out = match op {
            ConstBinaryOp::Add => a + b,
            ConstBinaryOp::Sub => a - b,
            ConstBinaryOp::Mul => a * b,
        };
        let value = if out.is_nan() {
            match width {
                16 => crate::FloatBits::F16(0x7e00),
                32 => crate::FloatBits::F32(0x7fc00000),
                _ => crate::FloatBits::F64(0x7ff8000000000000),
            }
        } else {
            crate::FloatBits::from_f64(width, out).expect("supported float width")
        };
        return Ok(ConstValue::F(value));
    }

    // int
    if ty_int_bits(ty).is_some() && !matches!(ty, Type::Bool) {
        if is_signed_int(ty) {
            let (ConstValue::I(a), ConstValue::I(b)) = (l, r) else {
                return Err(ConstEvalError::UnsupportedOperation);
            };
            let raw = match op {
                ConstBinaryOp::Add => a.wrapping_add(*b),
                ConstBinaryOp::Sub => a.wrapping_sub(*b),
                ConstBinaryOp::Mul => a.wrapping_mul(*b),
            };
            return Ok(ConstValue::I(wrap_i(raw, ty)));
        } else {
            let (ConstValue::U(a), ConstValue::U(b)) = (l, r) else {
                return Err(ConstEvalError::UnsupportedOperation);
            };
            let raw = match op {
                ConstBinaryOp::Add => a.wrapping_add(*b),
                ConstBinaryOp::Sub => a.wrapping_sub(*b),
                ConstBinaryOp::Mul => a.wrapping_mul(*b),
            };
            return Ok(ConstValue::U(wrap_u(raw, ty)));
        }
    }

    Err(ConstEvalError::UnsupportedOperation)
}

fn const_cmp(
    op: ConstCompareOp,
    ty: &Type,
    l: &ConstValue,
    r: &ConstValue,
) -> Result<bool, ConstEvalError> {
    // float
    if matches!(ty, Type::F16 | Type::F32 | Type::F64) {
        let (ConstValue::F(a), ConstValue::F(b)) = (l, r) else {
            return Err(ConstEvalError::UnsupportedOperation);
        };
        let (a, b) = (a.to_f64(), b.to_f64());
        return Ok(match op {
            ConstCompareOp::Eq => a == b,
            ConstCompareOp::Ne => a != b,
            ConstCompareOp::Lt => a < b,
            ConstCompareOp::Le => a <= b,
            ConstCompareOp::Gt => a > b,
            ConstCompareOp::Ge => a >= b,
        });
    }

    // bool은 eq/ne만
    if matches!(ty, Type::Bool) {
        let (ConstValue::Bool(a), ConstValue::Bool(b)) = (l, r) else {
            return Err(ConstEvalError::UnsupportedOperation);
        };
        return Ok(match op {
            ConstCompareOp::Eq => a == b,
            ConstCompareOp::Ne => a != b,
            _ => return Err(ConstEvalError::UnsupportedOperation),
        });
    }

    // int
    if ty_int_bits(ty).is_some() && !matches!(ty, Type::Bool) {
        if is_signed_int(ty) {
            let (ConstValue::I(a), ConstValue::I(b)) = (l, r) else {
                return Err(ConstEvalError::UnsupportedOperation);
            };
            return Ok(match op {
                ConstCompareOp::Eq => a == b,
                ConstCompareOp::Ne => a != b,
                ConstCompareOp::Lt => a < b,
                ConstCompareOp::Le => a <= b,
                ConstCompareOp::Gt => a > b,
                ConstCompareOp::Ge => a >= b,
            });
        } else {
            let (ConstValue::U(a), ConstValue::U(b)) = (l, r) else {
                return Err(ConstEvalError::UnsupportedOperation);
            };
            return Ok(match op {
                ConstCompareOp::Eq => a == b,
                ConstCompareOp::Ne => a != b,
                ConstCompareOp::Lt => a < b,
                ConstCompareOp::Le => a <= b,
                ConstCompareOp::Gt => a > b,
                ConstCompareOp::Ge => a >= b,
            });
        }
    }

    Err(ConstEvalError::UnsupportedOperation)
}
