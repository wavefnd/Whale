// SPDX-License-Identifier: MPL-2.0
//! LoongArch64 numeric/ABI register syntax and optional dollar prefixes.
use crate::tables::REGISTER_ALIASES;

use assembler::{fixed::error, AsmError};

pub(crate) fn reg(text: &str) -> Result<u32, AsmError> {
    let text = text.trim();
    let text = text.strip_prefix('$').unwrap_or(text).to_ascii_lowercase();
    if let Some(i) = REGISTER_ALIASES.iter().position(|&a| a == text) {
        return Ok(i as u32);
    }
    text.strip_prefix('r')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n < 32)
        .ok_or_else(|| error(format!("invalid LoongArch64 register {text:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_aliases_and_numeric_registers_share_identity() {
        for (number, alias) in REGISTER_ALIASES.iter().enumerate() {
            assert_eq!(reg(alias).unwrap(), number as u32);
            assert_eq!(reg(&format!("${alias}")).unwrap(), number as u32);
            assert_eq!(reg(&format!("$r{number}")).unwrap(), number as u32);
        }
        assert_eq!(reg(" $FP ").unwrap(), 22);
    }

    #[test]
    fn rejects_invalid_registers_and_repeated_prefixes() {
        for bad in ["$r32", "r-1", "$$r1", "$$sp", "$r", "x0", "$a8", ""] {
            assert!(reg(bad).is_err(), "{bad}");
        }
    }
}
