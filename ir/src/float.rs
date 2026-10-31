// SPDX-License-Identifier: MPL-2.0

pub mod arithmetic;

#[cfg(whale_wave_ir)]
#[path = "float/wave.rs"]
mod wave;

/// A malformed native response is distinct from a defined invalid conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloatBackendError(pub &'static str);

impl std::fmt::Display for FloatBackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wave float backend error: {}", self.0)
    }
}

impl std::error::Error for FloatBackendError {}

/// Exact IEEE storage bits. Equality compares storage, including NaN payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatBits {
    F16(u16),
    F32(u32),
    F64(u64),
}

impl FloatBits {
    pub fn width(self) -> u16 {
        match self {
            Self::F16(_) => 16,
            Self::F32(_) => 32,
            Self::F64(_) => 64,
        }
    }

    pub fn bits(self) -> u64 {
        match self {
            Self::F16(x) => x.into(),
            Self::F32(x) => x.into(),
            Self::F64(x) => x,
        }
    }

    /// Compare equal-width IEEE values; unsupported width pairs return `None`.
    pub fn try_compare(
        self,
        rhs: Self,
        predicate: &crate::FCmpPred,
    ) -> Result<Option<bool>, FloatBackendError> {
        #[cfg(whale_wave_ir)]
        {
            wave::compare(self, rhs, predicate)
        }
        #[cfg(not(whale_wave_ir))]
        {
            Ok(self.compare_reference(rhs, predicate))
        }
    }

    /// Compare storage through the explicit ordered/unordered predicate.
    pub fn compare(self, rhs: Self, predicate: &crate::FCmpPred) -> Option<bool> {
        self.try_compare(rhs, predicate).expect("Wave float comparison backend invariant")
    }

    /// Read exact-width hexadecimal storage bits.
    pub fn parse(width: u16, text: &str) -> Result<Self, String> {
        #[cfg(whale_wave_ir)]
        {
            wave::parse(width, text)
        }
        #[cfg(not(whale_wave_ir))]
        {
            Self::parse_reference(width, text)
        }
    }

    /// Numeric conversion with explicit backend errors; unsupported widths return `None`.
    pub fn try_from_f64(width: u16, value: f64) -> Result<Option<Self>, FloatBackendError> {
        if !matches!(width, 16 | 32 | 64) {
            return Ok(None);
        }
        #[cfg(whale_wave_ir)]
        {
            wave::from_f64(width, value.to_bits()).map(Some)
        }
        #[cfg(not(whale_wave_ir))]
        {
            Ok(Self::from_f64_reference(width, value).ok())
        }
    }

    /// Numeric convenience conversion. Use variants to preserve source storage bits.
    pub fn from_f64(width: u16, value: f64) -> Result<Self, String> {
        Self::try_from_f64(width, value)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "float width must be 16, 32 or 64".into())
    }

    /// Expand storage to binary64 using the defined per-width NaN payload rules.
    pub fn try_to_f64(self) -> Result<f64, FloatBackendError> {
        #[cfg(whale_wave_ir)]
        {
            wave::to_binary64(self).map(f64::from_bits)
        }
        #[cfg(not(whale_wave_ir))]
        {
            Ok(self.to_f64_reference())
        }
    }

    /// Numeric convenience view of the Wave-produced binary64 storage.
    pub fn to_f64(self) -> f64 {
        self.try_to_f64().expect("Wave storage conversion backend invariant")
    }

    /// Compare equal-width IEEE values using an explicit ordered/unordered
    /// predicate. Returns `None` for mismatched widths. Integer bit ordering
    /// avoids host floating exceptions/conversions and treats both zeros equal;
    /// any quiet or signaling NaN makes the operands unordered.
    #[cfg(any(not(whale_wave_ir), test))]
    pub(crate) fn compare_reference(self, rhs: Self, predicate: &crate::FCmpPred) -> Option<bool> {
        use crate::FCmpPred::*;

        use std::cmp::Ordering;
        if self.width() != rhs.width() {
            return None;
        }
        let (exponent, fraction) = match self {
            Self::F16(_) => (0x7c00, 0x03ff),
            Self::F32(_) => (0x7f800000, 0x007fffff),
            Self::F64(_) => (0x7ff0000000000000, 0x000fffffffffffff),
        };
        let (a, b) = (self.bits(), rhs.bits());
        let nan = |bits| bits & exponent == exponent && bits & fraction != 0;
        let ordered = !nan(a) && !nan(b);
        let sign = 1u64 << (self.width() - 1);
        let magnitude = sign - 1;
        let order = if a & magnitude == 0 && b & magnitude == 0 {
            Ordering::Equal
        } else if (a ^ b) & sign != 0 {
            if a & sign != 0 {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        } else if a & sign != 0 {
            b.cmp(&a)
        } else {
            a.cmp(&b)
        };
        let equal = order.is_eq();
        let less = order.is_lt();
        let greater = order.is_gt();
        Some(match predicate {
            Oeq => ordered && equal,
            One => ordered && !equal,
            Olt => ordered && less,
            Ole => ordered && !greater,
            Ogt => ordered && greater,
            Oge => ordered && !less,
            Ord => ordered,
            Uno => !ordered,
            Ueq => !ordered || equal,
            Une => !ordered || !equal,
            Ult => !ordered || less,
            Ule => !ordered || !greater,
            Ugt => !ordered || greater,
            Uge => !ordered || !less,
        })
    }

    /// Exact-width hexadecimal storage, not a decimal floating-point number.
    #[cfg(any(not(whale_wave_ir), test))]
    fn parse_reference(width: u16, text: &str) -> Result<Self, String> {
        let digits = match width {
            16 => 4,
            32 => 8,
            64 => 16,
            _ => return Err("float width must be 16, 32 or 64".into()),
        };
        let hex = text.strip_prefix("0x").ok_or("float value must start with 0x")?;
        if hex.len() != digits || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("f{width} requires exactly {digits} hexadecimal digits"));
        }
        let bits = u64::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
        Ok(match width {
            16 => Self::F16(bits as u16),
            32 => Self::F32(bits as u32),
            _ => Self::F64(bits),
        })
    }

    /// Numeric convenience conversion. Use the variants to preserve source bits.
    #[cfg(any(not(whale_wave_ir), test))]
    fn from_f64_reference(width: u16, value: f64) -> Result<Self, String> {
        Ok(match width {
            16 => Self::F16(f64_to_f16(value)),
            32 => Self::F32((value as f32).to_bits()),
            64 => Self::F64(value.to_bits()),
            _ => return Err("float width must be 16, 32 or 64".into()),
        })
    }

    #[cfg(any(not(whale_wave_ir), test))]
    fn to_f64_reference(self) -> f64 {
        match self {
            Self::F64(bits) => f64::from_bits(bits),
            Self::F32(bits) => f32::from_bits(bits) as f64,
            Self::F16(bits) => {
                let sign = u64::from(bits >> 15) << 63;
                let exponent = (bits >> 10) & 31;
                let fraction = u64::from(bits & 1023);
                if exponent == 31 {
                    return f64::from_bits(sign | (0x7ff << 52) | (fraction << 42));
                }
                let magnitude = if exponent == 0 {
                    (fraction as f64) * 2f64.powi(-24)
                } else {
                    (1024 + fraction) as f64 * 2f64.powi(i32::from(exponent) - 25)
                };
                if sign != 0 {
                    -magnitude
                } else {
                    magnitude
                }
            },
        }
    }
}

impl std::fmt::Display for FloatBits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:0width$x}", self.bits(), width = usize::from(self.width() / 4))
    }
}

// Direct binary64 -> binary16 rounding avoids the intermediate f32 double round.
#[cfg(any(not(whale_wave_ir), test))]
fn f64_to_f16(value: f64) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 48) & 0x8000) as u16;
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    if exp == 0x7ff {
        return sign | if fraction == 0 { 0x7c00 } else { 0x7e00 };
    }
    if exp == 0 {
        return sign;
    }
    let unbiased = exp - 1023;
    if unbiased > 15 {
        return sign | 0x7c00;
    }
    if unbiased < -25 {
        return sign;
    }
    let mantissa = fraction | (1u64 << 52);
    let shift = if unbiased < -14 { (28 - unbiased) as u32 } else { 42 };
    let base = mantissa >> shift;
    let remainder = mantissa & ((1u64 << shift) - 1);
    let halfway = 1u64 << (shift - 1);
    let rounded = base + u64::from(remainder > halfway || (remainder == halfway && base & 1 != 0));
    if unbiased < -14 {
        sign | rounded as u16
    } else {
        sign | ((((unbiased + 14) as u16) << 10) + rounded as u16)
    }
}

#[cfg(all(test, whale_wave_ir))]
mod wave_tests {
    use super::*;

    #[test]
    fn wave_storage_conveniences_preserve_reference_payload_and_spelling_rules() {
        for (width, raw) in [
            (16, 0u64),
            (16, 1),
            (16, 0x7c01),
            (16, 0xfcff),
            (32, 0x7f800001),
            (32, 0xffc12345),
            (32, 1),
            (64, 0x7ff0000000000001),
            (64, 0xfff800000000cafe),
            (64, 1),
        ] {
            let text = format!("0x{raw:0digits$x}", digits = usize::from(width) / 4);
            let value = FloatBits::parse(width, &text).unwrap();
            assert_eq!(value, FloatBits::parse_reference(width, &text).unwrap());
            assert_eq!(value.to_f64().to_bits(), value.to_f64_reference().to_bits(), "{value:?}");
            for pred in [
                crate::FCmpPred::Oeq,
                crate::FCmpPred::One,
                crate::FCmpPred::Olt,
                crate::FCmpPred::Ole,
                crate::FCmpPred::Ogt,
                crate::FCmpPred::Oge,
                crate::FCmpPred::Ord,
                crate::FCmpPred::Uno,
                crate::FCmpPred::Ueq,
                crate::FCmpPred::Une,
                crate::FCmpPred::Ult,
                crate::FCmpPred::Ule,
                crate::FCmpPred::Ugt,
                crate::FCmpPred::Uge,
            ] {
                assert_eq!(
                    value.try_compare(value, &pred).unwrap(),
                    value.compare_reference(value, &pred)
                );
            }
        }
        for bits in [
            0,
            1,
            0x8000000000000000,
            0x3ff0020000000001,
            0x7ff0000000000000,
            0x7ff0000000000001,
            0xfff800000000cafe,
            u64::MAX,
        ] {
            for width in [16, 32, 64] {
                let input = f64::from_bits(bits);
                assert_eq!(
                    FloatBits::from_f64(width, input).unwrap(),
                    FloatBits::from_f64_reference(width, input).unwrap(),
                    "{bits:x} {width}"
                );
            }
        }
        for width in [16, 32, 64, 80] {
            for text in
                ["", "0", "0X0", "0x0000", "0xGGGG", "0x01🦀", "0x12345678", "0x0123456789abcdef"]
            {
                assert_eq!(FloatBits::parse(width, text), FloatBits::parse_reference(width, text));
            }
        }
    }
}
