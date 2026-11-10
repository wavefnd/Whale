// SPDX-License-Identifier: MPL-2.0
//! Scalar bit transport. All numerical decisions execute in Wave.
use crate::{BinOp, CastOp, ConstValue, FCmpPred, FloatBackendError, FloatBits, Type};

#[repr(C)]
#[derive(Default)]
struct Request {
    mode: u64,
    operation: u64,
    source: u64,
    destination: u64,
    left_kind: u64,
    left_low: u64,
    left_high: u64,
    right_kind: u64,
    right_low: u64,
    right_high: u64,
}

#[repr(C)]
#[derive(Default)]
struct ResultRecord {
    status: u64,
    low: u64,
    high: u64,
    flag: u64,
}

#[repr(C)]
#[derive(Default)]
struct LiteralResult {
    status: u64,
    low: u64,
    high: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_float(request: *const Request, result: *mut ResultRecord) -> i32;

    fn whale_ir_parse_literal(
        source: *const u8,
        length: u64,
        category: u64,
        width: u64,
        max_work: u64,
        result: *mut LiteralResult,
    ) -> i32;
}

fn backend() -> FloatBackendError {
    FloatBackendError("scalar transport")
}

fn kind(ty: &Type) -> u64 {
    match ty {
        Type::Void => 0,
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
        Type::F16 => 14,
        Type::F32 => 15,
        Type::F64 => 16,
        _ => u64::MAX,
    }
}

fn float_kind(value: FloatBits) -> u64 {
    match value {
        FloatBits::F16(_) => 14,
        FloatBits::F32(_) => 15,
        FloatBits::F64(_) => 16,
    }
}

fn payload(value: &ConstValue) -> (u64, u64, u64) {
    let (tag, bits) = match value {
        ConstValue::I(value) => (1, *value as u128),
        ConstValue::U(value) => (2, *value),
        ConstValue::Bool(value) => (3, u128::from(*value)),
        ConstValue::F(FloatBits::F16(value)) => (4, u128::from(*value)),
        ConstValue::F(FloatBits::F32(value)) => (5, u128::from(*value)),
        ConstValue::F(FloatBits::F64(value)) => (6, u128::from(*value)),
    };
    (tag, bits as u64, (bits >> 64) as u64)
}

fn request(mode: u64, left: FloatBits) -> Request {
    let (left_kind, left_low, left_high) = payload(&ConstValue::F(left));
    Request { mode, source: float_kind(left), left_kind, left_low, left_high, ..Request::default() }
}

fn run(request: &Request) -> Result<Option<ResultRecord>, FloatBackendError> {
    let mut result = ResultRecord::default();
    let status = unsafe { whale_ir_float(request, &mut result) };
    if i32::try_from(result.status).ok() != Some(status) || result.flag > 1 {
        return Err(backend());
    }
    match status {
        0 => {
            if (request.mode != 2 && result.flag != 0)
                || (request.mode == 2 && (result.low != 0 || result.high != 0))
            {
                return Err(backend());
            }
            Ok(Some(result))
        },
        1 if result.low == 0 && result.high == 0 && result.flag == 0 => Ok(None),
        _ => Err(backend()),
    }
}

fn float(kind: u64, record: &ResultRecord) -> Result<FloatBits, FloatBackendError> {
    if record.high != 0 {
        return Err(backend());
    }
    match kind {
        14 => Ok(FloatBits::F16(record.low.try_into().map_err(|_| backend())?)),
        15 => Ok(FloatBits::F32(record.low.try_into().map_err(|_| backend())?)),
        16 => Ok(FloatBits::F64(record.low)),
        _ => Err(backend()),
    }
}

pub(super) fn arithmetic(
    op: &BinOp,
    lhs: FloatBits,
    rhs: FloatBits,
) -> Result<Option<FloatBits>, FloatBackendError> {
    let mut q = request(0, lhs);
    q.destination = float_kind(rhs);
    q.operation = match op {
        BinOp::FAdd => 0,
        BinOp::FSub => 1,
        BinOp::FMul => 2,
        BinOp::FDiv => 3,
        BinOp::FRem => 4,
        _ => u64::MAX,
    };
    (q.right_kind, q.right_low, q.right_high) = payload(&ConstValue::F(rhs));
    run(&q)?.map(|record| float(q.source, &record)).transpose()
}

pub(super) fn negate(value: FloatBits) -> Result<FloatBits, FloatBackendError> {
    let q = request(1, value);
    float(q.source, &run(&q)?.ok_or_else(backend)?)
}

pub(super) fn compare(
    lhs: FloatBits,
    rhs: FloatBits,
    pred: &FCmpPred,
) -> Result<Option<bool>, FloatBackendError> {
    let mut q = request(2, lhs);
    q.destination = float_kind(rhs);
    q.operation = match pred {
        FCmpPred::Oeq => 0,
        FCmpPred::One => 1,
        FCmpPred::Olt => 2,
        FCmpPred::Ole => 3,
        FCmpPred::Ogt => 4,
        FCmpPred::Oge => 5,
        FCmpPred::Ord => 6,
        FCmpPred::Uno => 7,
        FCmpPred::Ueq => 8,
        FCmpPred::Une => 9,
        FCmpPred::Ult => 10,
        FCmpPred::Ule => 11,
        FCmpPred::Ugt => 12,
        FCmpPred::Uge => 13,
    };
    (q.right_kind, q.right_low, q.right_high) = payload(&ConstValue::F(rhs));
    Ok(run(&q)?.map(|r| r.flag != 0))
}

pub(super) fn convert(
    op: &CastOp,
    src: &Type,
    dst: &Type,
    value: &ConstValue,
) -> Result<Option<ConstValue>, FloatBackendError> {
    let mut q =
        Request { mode: 3, source: kind(src), destination: kind(dst), ..Request::default() };
    q.operation = match op {
        CastOp::FExt => 0,
        CastOp::FTrunc => 1,
        CastOp::IToF_S => 2,
        CastOp::IToF_U => 3,
        CastOp::FToI_S => 4,
        CastOp::FToI_U => 5,
        _ => u64::MAX,
    };
    (q.left_kind, q.left_low, q.left_high) = payload(value);
    let Some(r) = run(&q)? else { return Ok(None) };
    let bits = u128::from(r.low) | (u128::from(r.high) << 64);
    let signed = bits as i128;
    let value = match dst {
        Type::F16 | Type::F32 | Type::F64 => ConstValue::F(float(q.destination, &r)?),
        Type::I1 if matches!(signed, -1 | 0) => ConstValue::I(signed),
        Type::I8 if i8::try_from(signed).is_ok() => ConstValue::I(signed),
        Type::I16 if i16::try_from(signed).is_ok() => ConstValue::I(signed),
        Type::I32 if i32::try_from(signed).is_ok() => ConstValue::I(signed),
        Type::I64 if i64::try_from(signed).is_ok() => ConstValue::I(signed),
        Type::I128 => ConstValue::I(signed),
        Type::U1 if bits <= 1 => ConstValue::U(bits),
        Type::U8 if u8::try_from(bits).is_ok() => ConstValue::U(bits),
        Type::U16 if u16::try_from(bits).is_ok() => ConstValue::U(bits),
        Type::U32 if u32::try_from(bits).is_ok() => ConstValue::U(bits),
        Type::U64 if u64::try_from(bits).is_ok() => ConstValue::U(bits),
        Type::U128 => ConstValue::U(bits),
        _ => return Err(backend()),
    };
    Ok(Some(value))
}

pub(super) fn to_binary64(value: FloatBits) -> Result<u64, FloatBackendError> {
    let r = run(&request(4, value))?.ok_or_else(backend)?;
    if r.high != 0 {
        return Err(backend());
    }
    Ok(r.low)
}

pub(super) fn from_f64(width: u16, bits: u64) -> Result<FloatBits, FloatBackendError> {
    let mut q = request(5, FloatBits::F64(bits));
    q.destination = match width {
        16 => 14,
        32 => 15,
        64 => 16,
        _ => return Err(FloatBackendError("float width must be 16, 32 or 64")),
    };
    float(q.destination, &run(&q)?.ok_or_else(backend)?)
}

pub(super) fn parse(width: u16, text: &str) -> Result<FloatBits, String> {
    if !matches!(width, 16 | 32 | 64) {
        return Err("float width must be 16, 32 or 64".into());
    }
    let mut result = LiteralResult::default();
    let status = unsafe {
        whale_ir_parse_literal(
            text.as_ptr(),
            text.len() as u64,
            4,
            width.into(),
            text.len() as u64,
            &mut result,
        )
    };
    if i32::try_from(result.status).ok() != Some(status)
        || result.work != text.len() as u64
        || (status != 0 && (result.low != 0 || result.high != 0))
    {
        return Err(backend().to_string());
    }
    match status {
        0 if result.high == 0 => float(
            match width {
                16 => 14,
                32 => 15,
                _ => 16,
            },
            &ResultRecord { low: result.low, ..ResultRecord::default() },
        )
        .map_err(|e| e.to_string()),
        6 => Err("float value must start with 0x".into()),
        7 => Err(format!("f{width} requires exactly {} hexadecimal digits", width / 4)),
        _ => Err(backend().to_string()),
    }
}
