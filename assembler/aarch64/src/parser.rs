// SPDX-License-Identifier: MPL-2.0
//! A64 register classes, shifted operands and base-plus-offset address syntax.
use crate::tables;

use assembler::{
    fixed::{self, error, unsigned},
    AsmError,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Reg {
    pub(crate) index: u32,
    pub(crate) width: u32,
    pub(crate) sp: bool,
    pub(crate) zero: bool,
}

pub(crate) fn reg(text: &str) -> Result<Reg, AsmError> {
    let text = text.trim().to_ascii_lowercase();
    let (index, width, sp, zero) = if let Some(alias) = tables::register_alias(&text) {
        alias
    } else {
        let mut chars = text.chars();
        let width = match chars.next() {
            Some('x') => 64,
            Some('w') => 32,
            _ => return Err(error(format!("invalid A64 register {text:?}"))),
        };
        let index = chars
            .as_str()
            .parse::<u32>()
            .ok()
            .filter(|n| *n <= 30)
            .ok_or_else(|| error(format!("invalid A64 register {text:?}")))?;
        (index, width, false, false)
    };
    Ok(Reg { index, width, sp, zero })
}

pub(crate) fn same(a: Reg, b: Reg) -> Result<(), AsmError> {
    if a.width != b.width {
        Err(error("register widths must match"))
    } else {
        Ok(())
    }
}

pub(crate) fn no_sp(r: Reg) -> Result<Reg, AsmError> {
    if r.sp {
        Err(error("SP is not valid in this operand"))
    } else {
        Ok(r)
    }
}

pub(crate) fn shift(text: &str, width: u32) -> Result<(u32, u32), AsmError> {
    let a: Vec<_> = text.split_whitespace().collect();
    if a.len() != 2 {
        return Err(error("shift requires kind and amount"));
    }
    let kind = match a[0].to_ascii_lowercase().as_str() {
        "lsl" => 0,
        "lsr" => 1,
        "asr" => 2,
        "ror" => 3,
        _ => return Err(error("unsupported shift kind")),
    };
    let amount = unsigned(a[1], if width == 64 { 6 } else { 5 })?;
    Ok((kind, amount))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Memory {
    pub(crate) base: Reg,
    pub(crate) offset: i128,
}

pub(crate) fn memory(text: &str) -> Result<Memory, AsmError> {
    let inside = text.strip_prefix('[').and_then(|s| s.strip_suffix(']')).ok_or_else(|| {
        error("memory operand requires [base, #offset]; writeback is unsupported")
    })?;
    let parts = fixed::operands(inside)?;
    if !(1..=2).contains(&parts.len()) {
        return Err(error("memory operand accepts a base and immediate offset"));
    }
    let base = reg(&parts[0])?;
    if base.width != 64 || base.zero {
        return Err(error("memory base requires X0..X30 or SP"));
    }
    let offset = if parts.len() == 2 { fixed::integer(&parts[1])? } else { 0 };
    Ok(Memory { base, offset })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_31_keeps_sp_and_zero_distinct() {
        let sp = reg("SP").unwrap();
        let zero = reg("xzr").unwrap();
        assert_eq!(sp.index, zero.index);
        assert!(sp.sp && !sp.zero);
        assert!(zero.zero && !zero.sp);
        assert!(no_sp(sp).is_err());
        assert!(no_sp(zero).is_ok());
        assert!(same(reg("w0").unwrap(), reg("x0").unwrap()).is_err());
        for bad in ["x31", "w31", "x32", "r0", "x", ""] {
            assert!(reg(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn memory_requires_64_bit_base_without_writeback() {
        let address = memory("[sp, #-256]").unwrap();
        assert_eq!(address.base, reg("sp").unwrap());
        assert_eq!(address.offset, -256);
        assert_eq!(memory("[x29]").unwrap().offset, 0);
        for bad in ["[w0]", "[xzr]", "[x0]!", "[x0], #8", "[x0, x1]", "[x0, #4, #8]"] {
            assert!(memory(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn shift_amount_obeys_operand_width() {
        assert!(shift("lsl #31", 32).is_ok());
        assert!(shift("lsl #32", 32).is_err());
        assert!(shift("asr #63", 64).is_ok());
        for bad in ["lsl #64", "lsl #-1", "lsl", "rotate #1"] {
            assert!(shift(bad, 64).is_err(), "{bad}");
        }
    }
}
