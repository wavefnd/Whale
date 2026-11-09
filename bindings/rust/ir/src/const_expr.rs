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
    BitAnd,
    BitOr,
    BitXor,
    Div,
    Rem,
    ShiftLeft,
    ShiftRight,
    LogicalAnd,
    LogicalOr,
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
    Not(Box<ConstExpr>),
    Neg(Box<ConstExpr>),
    Literal(ConstValue),
    Reference(ConstRef),
    Binary { op: ConstBinaryOp, left: Box<ConstExpr>, right: Box<ConstExpr> },
    Compare { op: ConstCompareOp, left: Box<ConstExpr>, right: Box<ConstExpr> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstEvalError {
    ResourceLimit(crate::LimitError),
    InvalidLiteral,
    TypeMismatch,
    UnsupportedOperation,
    DivisionByZero,
    RemainderByZero,
    UnknownReference(ConstRef),
    DuplicateDefinition(ConstRef),
    CyclicReference,
    ResultMismatch,
    Backend(&'static str),
}

impl ConstExpr {
    pub fn literal(ty: Type, value: ConstValue) -> Self {
        Self { ty, kind: ConstExprKind::Literal(value) }
    }

    pub fn references(&self) -> Vec<ConstRef> {
        let mut pending = vec![self];
        let mut references = Vec::new();
        while let Some(expr) = pending.pop() {
            match &expr.kind {
                ConstExprKind::Literal(_) => {},
                ConstExprKind::Neg(value) | ConstExprKind::Not(value) => pending.push(value),
                ConstExprKind::Reference(reference) => references.push(*reference),
                ConstExprKind::Binary { left, right, .. }
                | ConstExprKind::Compare { left, right, .. } => {
                    pending.push(right);
                    pending.push(left);
                },
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
        self.evaluate_with_error_site(resolve, limits).map_err(|(error, _)| error)
    }

    /// Preserve the exact failing node without repeating validation, reference
    /// resolution or arithmetic. Frontend annotations stay outside typed trees.
    pub(crate) fn evaluate_with_error_site(
        &self,
        resolve: &impl Fn(ConstRef) -> Option<(Type, ConstValue)>,
        limits: crate::IrLimits,
    ) -> Result<ConstValue, (ConstEvalError, &ConstExpr)> {
        #[cfg(whale_wave_ir)]
        {
            crate::constant_wave::evaluate(self, resolve, limits)
        }
        #[cfg(not(whale_wave_ir))]
        {
            self.evaluate_with_error_site_reference(resolve, limits)
        }
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub(crate) fn evaluate_with_error_site_reference(
        &self,
        resolve: &impl Fn(ConstRef) -> Option<(Type, ConstValue)>,
        limits: crate::IrLimits,
    ) -> Result<ConstValue, (ConstEvalError, &ConstExpr)> {
        let mut budget = crate::limits::Budget::new(limits)
            .map_err(|error| (ConstEvalError::ResourceLimit(error), self))?;
        budget.expression(self).map_err(|error| (ConstEvalError::ResourceLimit(error), self))?;
        // Validate every preserved branch before evaluating lazily. Invalid IR
        // is rejected even if its expression would be skipped at execution.
        let mut inspect = vec![self];
        let mut resolved = std::collections::HashMap::new();
        while let Some(expr) = inspect.pop() {
            match &expr.kind {
                ConstExprKind::Literal(v) => {
                    if !crate::constant::valid_constant_reference(&expr.ty, v) {
                        return Err((ConstEvalError::InvalidLiteral, expr));
                    }
                },
                ConstExprKind::Reference(reference) => {
                    let (ty, value) = resolve(*reference)
                        .ok_or((ConstEvalError::UnknownReference(*reference), expr))?;
                    if let Err(error) = budget.ty(&ty) {
                        crate::limits::discard_type(ty);
                        return Err((ConstEvalError::ResourceLimit(error), expr));
                    }
                    if ty != expr.ty {
                        return Err((ConstEvalError::TypeMismatch, expr));
                    }
                    if !crate::constant::valid_constant_reference(&ty, &value) {
                        return Err((ConstEvalError::InvalidLiteral, expr));
                    }
                    resolved.insert(expr as *const ConstExpr, value);
                },
                ConstExprKind::Neg(value) => {
                    if expr.ty != value.ty {
                        return Err((ConstEvalError::TypeMismatch, expr));
                    }
                    if crate::interpreter::integer::reference::shape(&expr.ty).is_none()
                        && !matches!(expr.ty, Type::F16 | Type::F32 | Type::F64)
                    {
                        return Err((ConstEvalError::UnsupportedOperation, expr));
                    }
                    inspect.push(value);
                },
                ConstExprKind::Not(value) => {
                    if expr.ty != value.ty {
                        return Err((ConstEvalError::TypeMismatch, expr));
                    }
                    if ty_int_bits(&expr.ty).is_none() {
                        return Err((ConstEvalError::UnsupportedOperation, expr));
                    }
                    inspect.push(value);
                },
                ConstExprKind::Binary { op, left, right } => {
                    if left.ty != right.ty || expr.ty != left.ty {
                        return Err((ConstEvalError::TypeMismatch, expr));
                    }
                    let supported =
                        if matches!(op, ConstBinaryOp::LogicalAnd | ConstBinaryOp::LogicalOr) {
                            expr.ty == Type::Bool
                        } else if matches!(expr.ty, Type::F16 | Type::F32 | Type::F64) {
                            matches!(
                                op,
                                ConstBinaryOp::Add
                                    | ConstBinaryOp::Sub
                                    | ConstBinaryOp::Mul
                                    | ConstBinaryOp::Div
                                    | ConstBinaryOp::Rem
                            )
                        } else {
                            ty_int_bits(&expr.ty).is_some()
                        };
                    if !supported {
                        return Err((ConstEvalError::UnsupportedOperation, expr));
                    }
                    inspect.push(right);
                    inspect.push(left);
                },
                ConstExprKind::Compare { op, left, right } => {
                    if left.ty != right.ty || expr.ty != Type::Bool {
                        return Err((ConstEvalError::TypeMismatch, expr));
                    }
                    if left.ty == Type::Bool {
                        if !matches!(op, ConstCompareOp::Eq | ConstCompareOp::Ne) {
                            return Err((ConstEvalError::UnsupportedOperation, expr));
                        }
                    } else if ty_int_bits(&left.ty).is_none()
                        && !matches!(left.ty, Type::F16 | Type::F32 | Type::F64)
                    {
                        return Err((ConstEvalError::UnsupportedOperation, expr));
                    }
                    inspect.push(right);
                    inspect.push(left);
                },
            }
        }
        // An explicit evaluation stack preserves left-to-right order and lazy
        // boolean semantics without recursing on the caller's tree.
        let mut pending = vec![(self, 0u8)];
        let mut values = Vec::new();
        while let Some((expr, phase)) = pending.pop() {
            let value = match &expr.kind {
                ConstExprKind::Literal(value) => value.clone(),
                ConstExprKind::Reference(_) => resolved[&(expr as *const ConstExpr)].clone(),
                ConstExprKind::Neg(value) | ConstExprKind::Not(value) if phase == 0 => {
                    pending.push((expr, 1));
                    pending.push((value, 0));
                    continue;
                },
                ConstExprKind::Neg(_) => match values.pop().expect("neg operand") {
                    ConstValue::F(value) => {
                        ConstValue::F(crate::float::arithmetic::reference::negate(value))
                    },
                    value => crate::interpreter::integer::reference::pack(
                        &expr.ty,
                        0u128.wrapping_sub(crate::interpreter::integer::reference::raw(
                            &value,
                            crate::interpreter::integer::reference::shape(&expr.ty).unwrap().0,
                        )),
                    ),
                },
                ConstExprKind::Not(_) => const_not(&expr.ty, &values.pop().expect("not operand"))
                    .map_err(|error| (error, expr))?,
                ConstExprKind::Binary {
                    op: ConstBinaryOp::LogicalAnd | ConstBinaryOp::LogicalOr,
                    left,
                    right,
                } => {
                    if phase == 0 {
                        pending.push((expr, 1));
                        pending.push((left, 0));
                        continue;
                    }
                    let ConstValue::Bool(value) = values.pop().expect("logical operand") else {
                        return Err((ConstEvalError::TypeMismatch, expr));
                    };
                    let ConstExprKind::Binary { op, .. } = expr.kind else { unreachable!() };
                    if phase == 1
                        && ((op == ConstBinaryOp::LogicalAnd && value)
                            || (op == ConstBinaryOp::LogicalOr && !value))
                    {
                        pending.push((expr, 2));
                        pending.push((right, 0));
                        continue;
                    }
                    ConstValue::Bool(value)
                },
                ConstExprKind::Binary { left, right, .. }
                | ConstExprKind::Compare { left, right, .. }
                    if phase == 0 =>
                {
                    pending.push((expr, 1));
                    pending.push((right, 0));
                    pending.push((left, 0));
                    continue;
                },
                ConstExprKind::Binary { op, left, .. } => {
                    let right = values.pop().expect("right operand");
                    let left_value = values.pop().expect("left operand");
                    const_bin(*op, &left.ty, &left_value, &right).map_err(|error| (error, expr))?
                },
                ConstExprKind::Compare { op, left, .. } => {
                    let right = values.pop().expect("right operand");
                    let left_value = values.pop().expect("left operand");
                    ConstValue::Bool(
                        const_cmp(*op, &left.ty, &left_value, &right)
                            .map_err(|error| (error, expr))?,
                    )
                },
            };
            if !crate::constant::valid_constant_reference(&expr.ty, &value) {
                return Err((ConstEvalError::InvalidLiteral, expr));
            }
            values.push(value);
        }
        Ok(values.pop().expect("root expression result"))
    }
}

#[cfg(any(not(whale_wave_ir), test))]
pub(crate) fn same_value(left: &ConstValue, right: &ConstValue) -> bool {
    match (left, right) {
        // Compare the stored payload, including signed zero and NaN bits.
        (ConstValue::F(a), ConstValue::F(b)) => a == b,
        _ => left == right,
    }
}

// Preserve the existing scalar constant evaluation rules.
#[cfg(any(not(whale_wave_ir), test))]
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

#[cfg(any(not(whale_wave_ir), test))]
fn is_signed_int(ty: &Type) -> bool {
    matches!(ty, Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128)
}

#[cfg(any(not(whale_wave_ir), test))]
fn wrap_u(v: u128, ty: &Type) -> u128 {
    let bits = ty_int_bits(ty).unwrap_or(128);
    if bits >= 128 {
        return v;
    }
    let mask = (1u128 << bits) - 1;
    v & mask
}

#[cfg(any(not(whale_wave_ir), test))]
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

#[cfg(any(not(whale_wave_ir), test))]
fn shift_count(ty: &Type, raw: u128) -> u32 {
    let bits = ty_int_bits(ty).expect("integer expression validated");
    (wrap_u(raw, ty) % u128::from(bits)) as u32
}

#[cfg(any(not(whale_wave_ir), test))]
fn const_not(ty: &Type, value: &ConstValue) -> Result<ConstValue, ConstEvalError> {
    match value {
        ConstValue::I(v) if is_signed_int(ty) => Ok(ConstValue::I(wrap_i(!v, ty))),
        ConstValue::U(v) if ty_int_bits(ty).is_some() => Ok(ConstValue::U(wrap_u(!v, ty))),
        _ => Err(ConstEvalError::UnsupportedOperation),
    }
}

#[cfg(any(not(whale_wave_ir), test))]
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
        let op = match op {
            ConstBinaryOp::Add => crate::BinOp::FAdd,
            ConstBinaryOp::Sub => crate::BinOp::FSub,
            ConstBinaryOp::Mul => crate::BinOp::FMul,
            ConstBinaryOp::Div => crate::BinOp::FDiv,
            ConstBinaryOp::Rem => crate::BinOp::FRem,
            _ => return Err(ConstEvalError::UnsupportedOperation),
        };
        let value = crate::float::arithmetic::reference::arithmetic(&op, *a, *b).unwrap();
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
                ConstBinaryOp::BitAnd => a & b,
                ConstBinaryOp::BitOr => a | b,
                ConstBinaryOp::BitXor => a ^ b,
                ConstBinaryOp::Div if *b == 0 => return Err(ConstEvalError::DivisionByZero),
                ConstBinaryOp::Rem if *b == 0 => return Err(ConstEvalError::RemainderByZero),
                ConstBinaryOp::Div => a.wrapping_div(*b),
                ConstBinaryOp::Rem => a.wrapping_rem(*b),
                ConstBinaryOp::ShiftLeft => a.wrapping_shl(shift_count(ty, *b as u128)),
                ConstBinaryOp::ShiftRight => a.wrapping_shr(shift_count(ty, *b as u128)),
                _ => return Err(ConstEvalError::UnsupportedOperation),
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
                ConstBinaryOp::BitAnd => a & b,
                ConstBinaryOp::BitOr => a | b,
                ConstBinaryOp::BitXor => a ^ b,
                ConstBinaryOp::Div if *b == 0 => return Err(ConstEvalError::DivisionByZero),
                ConstBinaryOp::Rem if *b == 0 => return Err(ConstEvalError::RemainderByZero),
                ConstBinaryOp::Div => a / b,
                ConstBinaryOp::Rem => a % b,
                ConstBinaryOp::ShiftLeft => a.wrapping_shl(shift_count(ty, *b)),
                ConstBinaryOp::ShiftRight => a.wrapping_shr(shift_count(ty, *b)),
                _ => return Err(ConstEvalError::UnsupportedOperation),
            };
            return Ok(ConstValue::U(wrap_u(raw, ty)));
        }
    }

    Err(ConstEvalError::UnsupportedOperation)
}

#[cfg(any(not(whale_wave_ir), test))]
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
        let generic = match op {
            ConstCompareOp::Eq => crate::CmpOp::FEq,
            ConstCompareOp::Ne => crate::CmpOp::FNe,
            ConstCompareOp::Lt => crate::CmpOp::FLt,
            ConstCompareOp::Le => crate::CmpOp::FLe,
            ConstCompareOp::Gt => crate::CmpOp::FGt,
            ConstCompareOp::Ge => crate::CmpOp::FGe,
        };
        return a
            .compare_reference(*b, &generic.float_predicate().unwrap())
            .ok_or(ConstEvalError::TypeMismatch);
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

#[cfg(all(test, whale_wave_ir))]
mod wave_tests {
    use super::*;

    use std::cell::RefCell;

    fn next(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    fn literal(ty: &Type, seed: &mut u64) -> ConstExpr {
        let bits = u128::from(next(seed)) | (u128::from(next(seed)) << 64);
        let value = match ty {
            Type::Bool => ConstValue::Bool(bits & 1 != 0),
            Type::F16 => ConstValue::F(crate::FloatBits::F16(bits as u16)),
            Type::F32 => ConstValue::F(crate::FloatBits::F32(bits as u32)),
            Type::F64 => ConstValue::F(crate::FloatBits::F64(bits as u64)),
            _ => crate::interpreter::integer::reference::pack(ty, bits),
        };
        ConstExpr::literal(ty.clone(), value)
    }

    fn tree(ty: &Type, seed: &mut u64, depth: usize) -> ConstExpr {
        if depth == 0 || next(seed) % 4 == 0 {
            return literal(ty, seed);
        }
        if next(seed) % 10 == 0 {
            return ConstExpr {
                ty: ty.clone(),
                kind: ConstExprKind::Reference(ConstRef::Global(GlobalId((next(seed) % 5) as u32))),
            };
        }
        let kind = if *ty == Type::Bool {
            ConstExprKind::Binary {
                op: if next(seed) & 1 == 0 {
                    ConstBinaryOp::LogicalAnd
                } else {
                    ConstBinaryOp::LogicalOr
                },
                left: Box::new(tree(ty, seed, depth - 1)),
                right: Box::new(tree(ty, seed, depth - 1)),
            }
        } else if next(seed) % 5 == 0 {
            if next(seed) & 1 == 0 {
                ConstExprKind::Neg(Box::new(tree(ty, seed, depth - 1)))
            } else {
                ConstExprKind::Not(Box::new(tree(ty, seed, depth - 1)))
            }
        } else {
            let op = match next(seed) % 10 {
                0 => ConstBinaryOp::Add,
                1 => ConstBinaryOp::Sub,
                2 => ConstBinaryOp::Mul,
                3 => ConstBinaryOp::BitAnd,
                4 => ConstBinaryOp::BitOr,
                5 => ConstBinaryOp::BitXor,
                6 => ConstBinaryOp::Div,
                7 => ConstBinaryOp::Rem,
                8 => ConstBinaryOp::ShiftLeft,
                _ => ConstBinaryOp::ShiftRight,
            };
            ConstExprKind::Binary {
                op,
                left: Box::new(tree(ty, seed, depth - 1)),
                right: Box::new(tree(ty, seed, depth - 1)),
            }
        };
        ConstExpr { ty: ty.clone(), kind }
    }

    fn compare(expression: &ConstExpr) {
        let seen = RefCell::new(Vec::new());
        let resolve = |reference| {
            seen.borrow_mut().push(reference);
            match reference {
                ConstRef::Global(GlobalId(0)) => Some((Type::I32, ConstValue::I(7))),
                ConstRef::Global(GlobalId(1)) => Some((Type::I32, ConstValue::I(0))),
                ConstRef::Global(GlobalId(2)) => Some((Type::U128, ConstValue::U(u128::MAX))),
                ConstRef::Global(GlobalId(3)) => Some((Type::Bool, ConstValue::Bool(true))),
                _ => None,
            }
        };
        let expected =
            expression.evaluate_with_error_site_reference(&resolve, crate::IrLimits::default());
        let expected_calls = seen.replace(Vec::new());
        let actual = expression.evaluate_with_error_site(&resolve, crate::IrLimits::default());
        assert_eq!(*seen.borrow(), expected_calls, "reference order {expression:?}");
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => assert_eq!(actual, expected, "{expression:?}"),
            (Err((actual, site)), Err((expected, expected_site))) => {
                assert_eq!(actual, expected, "{expression:?}");
                assert!(std::ptr::eq(site, expected_site), "error site {expression:?}");
            },
            (actual, expected) => {
                panic!("constant divergence {expression:?}: {actual:?} / {expected:?}")
            },
        }
    }

    #[test]
    fn wave_constants_match_independent_reference_values_errors_sites_and_resolver_order() {
        let types = [
            Type::Bool,
            Type::I1,
            Type::U1,
            Type::I8,
            Type::U8,
            Type::I16,
            Type::U16,
            Type::I32,
            Type::U32,
            Type::I64,
            Type::U64,
            Type::I128,
            Type::U128,
            Type::F16,
            Type::F32,
            Type::F64,
        ];
        let mut seed = 0x0003_5235_3558_u64;
        for _ in 0..4000 {
            let ty = &types[next(&mut seed) as usize % types.len()];
            let mut expression = tree(ty, &mut seed, 5);
            if next(&mut seed) % 3 == 0 {
                let op = match next(&mut seed) % 6 {
                    0 => ConstCompareOp::Eq,
                    1 => ConstCompareOp::Ne,
                    2 => ConstCompareOp::Lt,
                    3 => ConstCompareOp::Le,
                    4 => ConstCompareOp::Gt,
                    _ => ConstCompareOp::Ge,
                };
                expression = ConstExpr {
                    ty: Type::Bool,
                    kind: ConstExprKind::Compare {
                        op,
                        left: Box::new(expression),
                        right: Box::new(tree(ty, &mut seed, 3)),
                    },
                };
            }
            compare(&expression);
        }
        for expression in [
            ConstExpr::literal(Type::I1, ConstValue::I(1)),
            ConstExpr::literal(Type::U8, ConstValue::U(256)),
            ConstExpr::literal(Type::Bool, ConstValue::U(1)),
            ConstExpr::literal(Type::F32, ConstValue::F(crate::FloatBits::F64(0))),
        ] {
            compare(&expression);
        }
    }

    #[test]
    fn constant_work_is_bounded_even_when_validation_would_skip_a_branch() {
        let expression = ConstExpr::literal(Type::I32, ConstValue::I(7));
        assert!(matches!(
            expression.evaluate_with_limits(
                &|_| None,
                crate::IrLimits { max_analysis_work: 0, ..Default::default() }
            ),
            Err(ConstEvalError::ResourceLimit(crate::LimitError {
                resource: crate::Resource::AnalysisWork,
                ..
            }))
        ));
    }
}
