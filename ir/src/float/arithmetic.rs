// SPDX-License-Identifier: MPL-2.0
//! Integer IEEE arithmetic. Finite values are sign * significand * 2^exponent.
//! A u128 holds exact products (at most 106 bits). Addition/division retain
//! guard, round and sticky bits; only pack performs nearest-even rounding.
use crate::{BinOp, FloatBits};

#[path = "conversions.rs"]
mod conversions;

pub use conversions::convert;

#[derive(Clone, Copy)]
struct Format {
    width: u32,
    fraction: u32,
    bias: i32,
}

impl Format {
    fn of(value: FloatBits) -> Self {
        match value {
            FloatBits::F16(_) => Self { width: 16, fraction: 10, bias: 15 },
            FloatBits::F32(_) => Self { width: 32, fraction: 23, bias: 127 },
            FloatBits::F64(_) => Self { width: 64, fraction: 52, bias: 1023 },
        }
    }

    fn sign(self) -> u64 {
        1u64 << (self.width - 1)
    }

    fn infinity(self) -> u64 {
        ((self.bias as u64) * 2 + 1) << self.fraction
    }

    fn nan(self) -> FloatBits {
        self.bits(self.infinity() | (1u64 << (self.fraction - 1)))
    }

    fn bits(self, raw: u64) -> FloatBits {
        match self.width {
            16 => FloatBits::F16(raw as u16),
            32 => FloatBits::F32(raw as u32),
            _ => FloatBits::F64(raw),
        }
    }

    fn special(self, negative: bool, infinity: bool) -> FloatBits {
        self.bits(
            if negative { self.sign() } else { 0 } | if infinity { self.infinity() } else { 0 },
        )
    }

    fn unpack(self, value: FloatBits) -> Parts {
        let bits = value.bits();
        let exp = ((bits & !self.sign()) >> self.fraction) as i32;
        let fraction = bits & ((1u64 << self.fraction) - 1);
        let special = exp == 2 * self.bias + 1;
        Parts {
            negative: bits & self.sign() != 0,
            sig: if exp == 0 || special {
                u128::from(fraction)
            } else {
                u128::from(fraction) | (1u128 << self.fraction)
            },
            exp: exp.max(1) - self.bias - self.fraction as i32,
            nan: special && fraction != 0,
            infinity: special && fraction == 0,
        }
    }

    fn pack(self, negative: bool, sig: u128, exp: i32) -> FloatBits {
        if sig == 0 {
            return self.special(negative, false);
        }
        let highest = (127 - sig.leading_zeros()) as i32;
        let target =
            (exp + highest - self.fraction as i32).max(1 - self.bias - self.fraction as i32);
        let shift = target - exp;
        let mut rounded = if shift > 0 { round(sig, shift as u32) } else { sig << (-shift as u32) };
        let mut target = target;
        if rounded >= (1u128 << (self.fraction + 1)) {
            rounded >>= 1;
            target += 1;
        }
        if rounded == 0 {
            return self.special(negative, false);
        }
        let exponent = if rounded < (1u128 << self.fraction) {
            0
        } else {
            target + self.fraction as i32 + self.bias
        };
        if exponent > 2 * self.bias {
            return self.special(negative, true);
        }
        self.bits(
            (if negative { self.sign() } else { 0 })
                | ((exponent as u64) << self.fraction)
                | ((rounded as u64) & ((1u64 << self.fraction) - 1)),
        )
    }
}

#[derive(Clone, Copy)]
struct Parts {
    negative: bool,
    sig: u128,
    exp: i32,
    nan: bool,
    infinity: bool,
}

impl Parts {
    fn normalize(&mut self, f: Format) {
        if self.sig != 0 {
            let shift = self.sig.leading_zeros() - (127 - f.fraction);
            self.sig <<= shift;
            self.exp -= shift as i32;
        }
    }
}

fn round(value: u128, shift: u32) -> u128 {
    if shift > 128 {
        return 0;
    }
    if shift == 128 {
        return u128::from(value > 1u128 << 127);
    }
    let q = value >> shift;
    let remainder = value & ((1u128 << shift) - 1);
    let halfway = 1u128 << (shift - 1);
    q + u128::from(remainder > halfway || (remainder == halfway && q & 1 != 0))
}

fn jam(value: u128, shift: u32) -> u128 {
    if shift == 0 {
        value
    } else if shift >= 128 {
        u128::from(value != 0)
    } else {
        (value >> shift) | u128::from(value & ((1u128 << shift) - 1) != 0)
    }
}

/// IEEE sign-bit negation preserves zeros and NaN payloads without rounding.
pub fn negate(value: FloatBits) -> FloatBits {
    let f = Format::of(value);
    f.bits(value.bits() ^ f.sign())
}

pub fn arithmetic(op: &BinOp, lhs: FloatBits, rhs: FloatBits) -> Option<FloatBits> {
    if !matches!(op, BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv | BinOp::FRem) {
        return None;
    }
    if lhs.width() != rhs.width() {
        return None;
    }
    let f = Format::of(lhs);
    let mut a = f.unpack(lhs);
    let mut b = f.unpack(rhs);
    if a.nan || b.nan {
        return Some(f.nan());
    }
    if *op == BinOp::FSub {
        b.negative = !b.negative;
    }
    let negative = a.negative ^ b.negative;
    a.normalize(f);
    b.normalize(f);
    Some(match op {
        BinOp::FAdd | BinOp::FSub => {
            if a.infinity || b.infinity {
                if a.infinity && b.infinity && negative {
                    f.nan()
                } else {
                    f.special(if a.infinity { a.negative } else { b.negative }, true)
                }
            } else if a.sig == 0 && b.sig == 0 {
                f.special(a.negative && b.negative, false)
            } else if a.sig == 0 {
                f.pack(b.negative, b.sig, b.exp)
            } else if b.sig == 0 {
                f.pack(a.negative, a.sig, a.exp)
            } else {
                if (a.exp, a.sig) < (b.exp, b.sig) {
                    std::mem::swap(&mut a, &mut b);
                }
                // Three low bits retain ties and inexact aligned tails. Unlike
                // a host float intermediary, cancellation happens before rounding.
                let av = a.sig << 3;
                let bv = jam(b.sig << 3, (a.exp - b.exp) as u32);
                if a.negative == b.negative {
                    f.pack(a.negative, av + bv, a.exp - 3)
                } else {
                    f.pack(a.negative && av != bv, av - bv, a.exp - 3)
                }
            }
        },
        BinOp::FMul => {
            if (a.infinity && b.sig == 0 && !b.infinity)
                || (b.infinity && a.sig == 0 && !a.infinity)
            {
                f.nan()
            } else if a.infinity || b.infinity {
                f.special(negative, true)
            } else {
                f.pack(negative, a.sig * b.sig, a.exp + b.exp)
            }
        },
        BinOp::FDiv => {
            if (a.infinity && b.infinity)
                || (!a.infinity && !b.infinity && a.sig == 0 && b.sig == 0)
            {
                f.nan()
            } else if a.infinity || b.sig == 0 && !b.infinity {
                f.special(negative, true)
            } else if b.infinity || a.sig == 0 {
                f.special(negative, false)
            } else {
                let shift = f.fraction + 4;
                let numerator = a.sig << shift;
                let q = (numerator / b.sig) | u128::from(numerator % b.sig != 0);
                f.pack(negative, q, a.exp - b.exp - shift as i32)
            }
        },
        BinOp::FRem => {
            if a.infinity || b.sig == 0 && !b.infinity {
                f.nan()
            } else if b.infinity || a.sig == 0 || a.exp < b.exp {
                lhs
            } else {
                // Exact fmod remainder with logarithmic work even for the full
                // binary64 exponent span. Products are bounded by 106 bits.
                let mut r = a.sig % b.sig;
                let mut power = 2u128 % b.sig;
                let mut shift = (a.exp - b.exp) as u32;
                while shift != 0 {
                    if shift & 1 != 0 {
                        r = (r * power) % b.sig;
                    }
                    power = (power * power) % b.sig;
                    shift >>= 1;
                }
                f.pack(a.negative, r, b.exp)
            }
        },
        _ => return None,
    })
}
