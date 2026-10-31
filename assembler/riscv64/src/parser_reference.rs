// SPDX-License-Identifier: MPL-2.0
//! RV64 register aliases, offset(base) operands and FENCE flag syntax.
use crate::tables::REGISTER_ALIASES;

use assembler::{
    fixed::reference::{error, signed},
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
        .filter(|n| n.bytes().all(|b| b.is_ascii_digit()) && (n.len() == 1 || !n.starts_with('0')))
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
    if s == "0" {
        return Ok(0);
    }
    let mut value = 0;
    for c in s.chars() {
        let bit = match c {
            'i' => 8,
            'o' => 4,
            'r' => 2,
            'w' => 1,
            _ => return Err(error("fence sets use i,o,r,w or literal 0")),
        };
        if value & bit != 0 {
            return Err(error("duplicate fence flag"));
        }
        value |= bit;
    }
    Ok(value)
}
