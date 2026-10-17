// SPDX-License-Identifier: MPL-2.0
//! RV64 register aliases, offset(base) operands and FENCE flag syntax.
use crate::tables::REGISTER_ALIASES;

use assembler::{
    fixed::{error, signed},
    AsmError,
};

pub(crate) fn reg(text: &str) -> Result<u32, AsmError> {
    let text = text.trim().to_ascii_lowercase();
    if text == "fp" {
        return Ok(8);
    }
    if let Some(i) = REGISTER_ALIASES.iter().position(|&name| name == text) {
        return Ok(i as u32);
    }
    text.strip_prefix('x')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n < 32)
        .ok_or_else(|| error(format!("invalid RV64 register {text:?}")))
}

pub(crate) fn memory(text: &str) -> Result<(u32, u32), AsmError> {
    let (offset, rest) =
        text.split_once('(').ok_or_else(|| error("memory operand requires offset(base)"))?;
    let base = rest
        .strip_suffix(')')
        .filter(|s| !s.contains(['(', ')']))
        .ok_or_else(|| error("invalid memory operand"))?;
    Ok((reg(base)?, signed(if offset.trim().is_empty() { "0" } else { offset.trim() }, 12)?))
}

pub(crate) fn fence_mask(s: &str) -> Result<u32, AsmError> {
    let mut value = 0;
    for c in s.chars() {
        let bit = match c {
            'i' => 8,
            'o' => 4,
            'r' => 2,
            'w' => 1,
            _ => return Err(error("fence sets use i,o,r,w")),
        };
        if value & bit != 0 {
            return Err(error("duplicate fence flag"));
        }
        value |= bit;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_aliases_preserve_register_identity() {
        for (number, alias) in REGISTER_ALIASES.iter().enumerate() {
            assert_eq!(reg(alias).unwrap(), number as u32);
            assert_eq!(reg(&format!("x{number}")).unwrap(), number as u32);
        }
        assert_eq!(reg("fp").unwrap(), reg("s0").unwrap());
        for bad in ["x32", "x-1", "x", "a8", "$ra", ""] {
            assert!(reg(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn memory_checks_signed_12_bit_offset_and_delimiters() {
        assert_eq!(memory("-2048(sp)").unwrap(), (2, 0x800));
        assert_eq!(memory("2047(x31)").unwrap(), (31, 0x7ff));
        assert_eq!(memory("(sp)").unwrap(), (2, 0));
        for bad in ["2048(sp)", "-2049(sp)", "4((sp))", "4(sp))", "4(sp", "4(x32)"] {
            assert!(memory(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn fence_requires_unique_known_flags() {
        assert_eq!(fence_mask("iorw").unwrap(), 15);
        assert_eq!(fence_mask("rw").unwrap(), 3);
        for bad in ["rr", "ii", "rwz", "RW"] {
            assert!(fence_mask(bad).is_err(), "{bad}");
        }
    }
}
