// SPDX-License-Identifier: MPL-2.0
//! Fixed-width scalar transport. Production numeric decisions run in Wave.
#[cfg(any(test, not(whale_wave_ir)))]
use super::TrapReason;

use crate::Type;

#[cfg(any(test, not(whale_wave_ir)))]
use crate::{BinOp, CastOp, CheckedOp, ConstValue, ICmpPred};

#[cfg(any(test, not(whale_wave_ir)))]
#[path = "integer_reference.rs"]
pub(crate) mod reference;

#[cfg(any(test, not(whale_wave_ir)))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IntegerError {
    Trap(TrapReason),
    Backend(&'static str),
}

#[cfg(any(test, not(whale_wave_ir)))]
impl From<TrapReason> for IntegerError {
    fn from(value: TrapReason) -> Self {
        Self::Trap(value)
    }
}

/// Mapping enum tags into the scalar wire inventory is a typed transport step.
pub(crate) fn shape(ty: &Type) -> Option<(u32, bool)> {
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

#[cfg(all(test, whale_wave_ir))]
mod wave {
    use super::*;

    #[repr(C)]
    #[derive(Default)]
    struct ResultRecord {
        status: u64,
        low: u64,
        high: u64,
        flag: u64,
    }

    unsafe extern "C" {
        fn whale_ir_integer(
            mode: u64,
            op: u64,
            source_kind: u64,
            destination_kind: u64,
            a_low: u64,
            a_high: u64,
            b_low: u64,
            b_high: u64,
            result: *mut ResultRecord,
        ) -> i32;

        fn whale_ir_integer_bits(
            low: u64,
            high: u64,
            width: u64,
            signed: u64,
            result: *mut ResultRecord,
        ) -> i32;
    }

    fn backend() -> IntegerError {
        IntegerError::Backend("integer semantics transport")
    }

    fn kind(ty: &Type) -> Result<u64, IntegerError> {
        Ok(match ty {
            Type::Bool => 1,
            Type::I1 => 2,
            Type::U1 => 3,
            Type::I8 => 4,
            Type::I16 => 5,
            Type::I32 => 6,
            Type::I64 => 7,
            Type::I128 => 8,
            Type::U8 => 9,
            Type::U16 => 10,
            Type::U32 => 11,
            Type::U64 => 12,
            Type::U128 => 13,
            _ => return Err(backend()),
        })
    }

    fn payload(value: &ConstValue) -> Result<u128, IntegerError> {
        Ok(match value {
            ConstValue::Bool(value) => u128::from(*value),
            ConstValue::I(value) => *value as u128,
            ConstValue::U(value) => *value,
            _ => return Err(backend()),
        })
    }

    fn check(
        status: i32,
        record: &ResultRecord,
        flags: bool,
        traps: u8,
    ) -> Result<(u128, bool), IntegerError> {
        if i32::try_from(record.status).ok() != Some(status)
            || record.flag > u64::from(flags)
            || (status != 0 && (record.low != 0 || record.high != 0 || record.flag != 0))
        {
            return Err(backend());
        }
        if (1..=3).contains(&status) && traps & (1u8 << status) == 0 {
            return Err(backend());
        }
        match status {
            0 => Ok((u128::from(record.low) | (u128::from(record.high) << 64), record.flag != 0)),
            1 => Err(IntegerError::Trap(TrapReason::DivisionByZero)),
            2 => Err(IntegerError::Trap(TrapReason::RemainderByZero)),
            3 => Err(IntegerError::Trap(TrapReason::InvalidConversion)),
            _ => Err(backend()),
        }
    }

    fn decode(ty: &Type, raw: u128) -> Result<ConstValue, IntegerError> {
        let value = match ty {
            Type::Bool if raw <= 1 => ConstValue::Bool(raw != 0),
            _ if shape(ty).is_some_and(|(_, signed)| signed) => ConstValue::I(raw as i128),
            _ if shape(ty).is_some() => ConstValue::U(raw),
            _ => return Err(backend()),
        };
        if !crate::constant::valid_constant(ty, &value) {
            return Err(backend());
        }
        Ok(value)
    }

    pub(super) fn bits(value: u128, width: u32, signed: bool) -> Result<u128, IntegerError> {
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_integer_bits(
                value as u64,
                (value >> 64) as u64,
                width.into(),
                u64::from(signed),
                &mut result,
            )
        };
        check(status, &result, false, 0).map(|(value, _)| value)
    }

    pub(super) fn call(
        mode: u64,
        op: u64,
        source: &Type,
        destination: &Type,
        a: &ConstValue,
        b: &ConstValue,
    ) -> Result<(ConstValue, bool), IntegerError> {
        let (source_kind, destination_kind) = (kind(source)?, kind(destination)?);
        let (a, b) = (payload(a)?, payload(b)?);
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_integer(
                mode,
                op,
                source_kind,
                destination_kind,
                a as u64,
                (a >> 64) as u64,
                b as u64,
                (b >> 64) as u64,
                &mut result,
            )
        };
        let traps = if mode == 4 {
            1 << 3
        } else if mode == 0 && matches!(op, 3 | 4) {
            1 << 1
        } else if mode == 0 && matches!(op, 5 | 6) {
            1 << 2
        } else {
            0
        };
        let (raw, flag) = check(status, &result, mode == 1, traps)?;
        Ok((decode(destination, raw)?, flag))
    }

    #[cfg(test)]
    mod transport_tests {
        use super::*;

        #[test]
        fn malformed_numeric_responses_cannot_be_fabricated_data_traps() {
            for status in 1..=3 {
                let record = ResultRecord { status: status as u64, ..Default::default() };
                assert!(matches!(check(status, &record, false, 0), Err(IntegerError::Backend(_))));
                assert!(matches!(
                    check(status, &record, false, 1 << status),
                    Err(IntegerError::Trap(_))
                ));
            }
            let record = ResultRecord { status: 1, low: 9, ..Default::default() };
            assert!(matches!(check(1, &record, false, 2), Err(IntegerError::Backend(_))));
            let record = ResultRecord { flag: 2, ..Default::default() };
            assert!(matches!(check(0, &record, true, 0), Err(IntegerError::Backend(_))));
            assert!(matches!(check(0, &ResultRecord::default(), false, 0), Ok((0, false))));
            assert!(matches!(
                check(1, &ResultRecord::default(), false, 2),
                Err(IntegerError::Backend(_))
            ));
        }
    }
}

#[cfg(any(test, not(whale_wave_ir)))]
pub(crate) fn try_raw(value: &ConstValue, bits: u32) -> Result<u128, IntegerError> {
    if bits == 0 || bits > 128 || matches!(value, ConstValue::F(_)) {
        return Err(IntegerError::Backend("integer storage category"));
    }
    #[cfg(whale_wave_ir)]
    {
        let value = match value {
            ConstValue::I(value) => *value as u128,
            ConstValue::U(value) => *value,
            ConstValue::Bool(value) => u128::from(*value),
            _ => return Err(IntegerError::Backend("integer storage category")),
        };
        wave::bits(value, bits, false)
    }
    #[cfg(not(whale_wave_ir))]
    {
        Ok(reference::raw(value, bits))
    }
}

#[cfg(any(test, not(whale_wave_ir)))]
pub(crate) fn raw(value: &ConstValue, bits: u32) -> u128 {
    try_raw(value, bits).expect("Wave integer storage transport")
}

#[cfg(any(test, not(whale_wave_ir)))]
pub(crate) fn try_pack(ty: &Type, value: u128) -> Result<ConstValue, IntegerError> {
    #[cfg(whale_wave_ir)]
    {
        wave::call(5, 0, ty, ty, &ConstValue::U(value), &ConstValue::U(0)).map(|(value, _)| value)
    }
    #[cfg(not(whale_wave_ir))]
    {
        Ok(reference::pack(ty, value))
    }
}

#[cfg(any(test, not(whale_wave_ir)))]
pub(crate) fn pack(ty: &Type, value: u128) -> ConstValue {
    try_pack(ty, value).expect("Wave integer storage transport")
}

#[cfg(any(test, not(whale_wave_ir)))]
pub(crate) fn try_bin(
    op: &BinOp,
    ty: &Type,
    lhs: &ConstValue,
    rhs: &ConstValue,
) -> Result<ConstValue, IntegerError> {
    #[cfg(whale_wave_ir)]
    {
        let op = match op {
            BinOp::Add => 0,
            BinOp::Sub => 1,
            BinOp::Mul => 2,
            BinOp::UDiv => 3,
            BinOp::SDiv => 4,
            BinOp::URem => 5,
            BinOp::SRem => 6,
            BinOp::And => 12,
            BinOp::Or => 13,
            BinOp::Xor => 14,
            BinOp::Shl => 15,
            BinOp::LShr => 16,
            BinOp::AShr => 17,
            _ => return Err(IntegerError::Backend("noninteger binary opcode")),
        };
        wave::call(0, op, ty, ty, lhs, rhs).map(|(value, _)| value)
    }
    #[cfg(not(whale_wave_ir))]
    {
        reference::bin(op, ty, lhs, rhs).map_err(Into::into)
    }
}

#[cfg(any(test, not(whale_wave_ir)))]
pub(crate) fn try_checked(
    op: &CheckedOp,
    ty: &Type,
    lhs: &ConstValue,
    rhs: &ConstValue,
) -> Result<(ConstValue, bool), IntegerError> {
    #[cfg(whale_wave_ir)]
    {
        wave::call(
            1,
            match op {
                CheckedOp::UAdd => 0,
                CheckedOp::USub => 1,
                CheckedOp::UMul => 2,
                CheckedOp::SAdd => 3,
                CheckedOp::SSub => 4,
                CheckedOp::SMul => 5,
            },
            ty,
            ty,
            lhs,
            rhs,
        )
    }
    #[cfg(not(whale_wave_ir))]
    {
        Ok(reference::checked(op, ty, lhs, rhs))
    }
}

#[cfg(any(test, not(whale_wave_ir)))]
pub(crate) fn try_cast(
    op: &CastOp,
    source: &Type,
    destination: &Type,
    value: &ConstValue,
) -> Result<ConstValue, IntegerError> {
    #[cfg(whale_wave_ir)]
    {
        let op = match op {
            CastOp::ZExt => 0,
            CastOp::SExt => 1,
            CastOp::Trunc => 2,
            CastOp::Bitcast => 3,
            _ => return Err(IntegerError::Backend("noninteger cast opcode")),
        };
        wave::call(2, op, source, destination, value, &ConstValue::U(0)).map(|(value, _)| value)
    }
    #[cfg(not(whale_wave_ir))]
    {
        Ok(reference::cast(op, source, destination, value))
    }
}

#[cfg(any(test, not(whale_wave_ir)))]
pub(crate) fn try_compare(
    pred: &ICmpPred,
    ty: &Type,
    lhs: &ConstValue,
    rhs: &ConstValue,
) -> Result<bool, IntegerError> {
    #[cfg(whale_wave_ir)]
    {
        let op = match pred {
            ICmpPred::Eq => 0,
            ICmpPred::Ne => 1,
            ICmpPred::Ult => 2,
            ICmpPred::Ule => 3,
            ICmpPred::Ugt => 4,
            ICmpPred::Uge => 5,
            ICmpPred::Slt => 6,
            ICmpPred::Sle => 7,
            ICmpPred::Sgt => 8,
            ICmpPred::Sge => 9,
        };
        let (ConstValue::Bool(value), _) = wave::call(3, op, ty, &Type::Bool, lhs, rhs)? else {
            return Err(IntegerError::Backend("integer comparison transport"));
        };
        Ok(value)
    }
    #[cfg(not(whale_wave_ir))]
    {
        Ok(reference::compare(pred, ty, lhs, rhs))
    }
}

#[cfg(any(test, not(whale_wave_ir)))]
pub(crate) fn try_checked_cast(
    source: &Type,
    destination: &Type,
    value: &ConstValue,
) -> Result<ConstValue, IntegerError> {
    #[cfg(whale_wave_ir)]
    {
        wave::call(4, 0, source, destination, value, &ConstValue::U(0)).map(|(value, _)| value)
    }
    #[cfg(not(whale_wave_ir))]
    {
        reference::checked_cast(source, destination, value).map_err(Into::into)
    }
}

#[cfg(any(test, not(whale_wave_ir)))]
pub(crate) fn try_unary(
    negate: bool,
    ty: &Type,
    value: &ConstValue,
) -> Result<ConstValue, IntegerError> {
    #[cfg(whale_wave_ir)]
    {
        wave::call(6, u64::from(!negate), ty, ty, value, &ConstValue::U(0)).map(|(value, _)| value)
    }
    #[cfg(not(whale_wave_ir))]
    {
        let raw = reference::raw(value, shape(ty).expect("integer unary type").0);
        Ok(reference::pack(ty, if negate { 0u128.wrapping_sub(raw) } else { !raw }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types() -> [Type; 12] {
        [
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
        ]
    }

    #[test]
    fn numeric_bridge_matches_reference_at_all_widths_and_signed_boundaries() {
        let mut random = 0x0035_2035_2128_u128;
        for ty in types() {
            let width = shape(&ty).unwrap().0;
            let bit = 1u128 << (width - 1);
            let patterns = [0, 1, bit, bit.wrapping_sub(1), u128::MAX, u64::MAX as u128];
            for raw in patterns {
                let a = reference::pack(&ty, raw);
                assert_eq!(pack(&ty, raw), a);
                assert_eq!(
                    super::raw(&a, shape(&ty).unwrap().0),
                    reference::raw(&a, shape(&ty).unwrap().0)
                );
                assert_eq!(
                    try_unary(true, &ty, &a).unwrap(),
                    reference::pack(&ty, 0u128.wrapping_sub(raw))
                );
                assert_eq!(try_unary(false, &ty, &a).unwrap(), reference::pack(&ty, !raw));
                for _ in 0..20 {
                    random ^= random << 13;
                    random ^= random >> 17;
                    random ^= random << 5;
                    let b = reference::pack(&ty, random);
                    for op in [
                        BinOp::Add,
                        BinOp::Sub,
                        BinOp::Mul,
                        BinOp::UDiv,
                        BinOp::SDiv,
                        BinOp::URem,
                        BinOp::SRem,
                        BinOp::And,
                        BinOp::Or,
                        BinOp::Xor,
                        BinOp::Shl,
                        BinOp::LShr,
                        BinOp::AShr,
                    ] {
                        assert_eq!(
                            try_bin(&op, &ty, &a, &b),
                            reference::bin(&op, &ty, &a, &b).map_err(IntegerError::Trap),
                            "{ty:?} {op:?} {a:?} {b:?}"
                        );
                    }
                    for op in [
                        CheckedOp::UAdd,
                        CheckedOp::USub,
                        CheckedOp::UMul,
                        CheckedOp::SAdd,
                        CheckedOp::SSub,
                        CheckedOp::SMul,
                    ] {
                        assert_eq!(
                            try_checked(&op, &ty, &a, &b).unwrap(),
                            reference::checked(&op, &ty, &a, &b)
                        );
                    }
                    for pred in [
                        ICmpPred::Eq,
                        ICmpPred::Ne,
                        ICmpPred::Ult,
                        ICmpPred::Ule,
                        ICmpPred::Ugt,
                        ICmpPred::Uge,
                        ICmpPred::Slt,
                        ICmpPred::Sle,
                        ICmpPred::Sgt,
                        ICmpPred::Sge,
                    ] {
                        assert_eq!(
                            try_compare(&pred, &ty, &a, &b).unwrap(),
                            reference::compare(&pred, &ty, &a, &b)
                        );
                    }
                    for destination in types() {
                        assert_eq!(
                            try_checked_cast(&ty, &destination, &a),
                            reference::checked_cast(&ty, &destination, &a)
                                .map_err(IntegerError::Trap)
                        );
                        let dest_width = shape(&destination).unwrap().0;
                        for op in if width < dest_width {
                            vec![CastOp::ZExt, CastOp::SExt]
                        } else if width > dest_width {
                            vec![CastOp::Trunc]
                        } else {
                            vec![CastOp::Bitcast]
                        } {
                            assert_eq!(
                                try_cast(&op, &ty, &destination, &a).unwrap(),
                                reference::cast(&op, &ty, &destination, &a)
                            );
                        }
                    }
                }
            }
        }
        for pred in [ICmpPred::Eq, ICmpPred::Ne] {
            for a in [false, true] {
                for b in [false, true] {
                    assert_eq!(
                        try_compare(&pred, &Type::Bool, &ConstValue::Bool(a), &ConstValue::Bool(b))
                            .unwrap(),
                        reference::compare(
                            &pred,
                            &Type::Bool,
                            &ConstValue::Bool(a),
                            &ConstValue::Bool(b)
                        )
                    );
                }
            }
        }
    }

    #[cfg(whale_wave_ir)]
    #[test]
    fn invalid_backend_requests_remain_distinct_from_defined_data_traps() {
        assert_eq!(
            try_bin(&BinOp::FAdd, &Type::I32, &ConstValue::I(1), &ConstValue::I(2)),
            Err(IntegerError::Backend("noninteger binary opcode"))
        );
        assert!(matches!(try_pack(&Type::F64, 1), Err(IntegerError::Backend(_))));
        assert_eq!(
            try_bin(&BinOp::UDiv, &Type::U128, &ConstValue::U(1), &ConstValue::U(0)),
            Err(IntegerError::Trap(TrapReason::DivisionByZero))
        );
        assert_eq!(
            try_bin(&BinOp::SRem, &Type::I128, &ConstValue::I(1), &ConstValue::I(0)),
            Err(IntegerError::Trap(TrapReason::RemainderByZero))
        );
    }
}
