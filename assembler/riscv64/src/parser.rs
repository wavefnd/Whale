// SPDX-License-Identifier: MPL-2.0
//! RV64 operand transport. All native classification and validation run in Wave.
#[cfg(test)]
use assembler::AsmError;

#[cfg(any(test, not(whale_wave_riscv64)))]
#[path = "parser_reference.rs"]
pub(crate) mod reference;

#[cfg(test)]
pub(crate) fn reg(text: &str) -> Result<u32, AsmError> {
    #[cfg(whale_wave_riscv64)]
    return crate::wave::reg(text);
    #[cfg(not(whale_wave_riscv64))]
    reference::reg(text)
}

#[cfg(test)]
pub(crate) fn memory(text: &str) -> Result<(u32, u32), AsmError> {
    #[cfg(whale_wave_riscv64)]
    return crate::wave::memory(text);
    #[cfg(not(whale_wave_riscv64))]
    reference::memory(text)
}

#[cfg(test)]
pub(crate) fn fence_mask(text: &str) -> Result<u32, AsmError> {
    #[cfg(whale_wave_riscv64)]
    return crate::wave::fence_mask(text);
    #[cfg(not(whale_wave_riscv64))]
    reference::fence_mask(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::tables::REGISTER_ALIASES;

    #[test]
    fn numeric_register_names_are_canonical() {
        for index in 0..32 {
            let name = format!("x{index}");
            assert_eq!(reg(&name).unwrap(), index);
            assert_eq!(reg(&name.to_ascii_uppercase()).unwrap(), index);
        }
        for suffix in ["+1", "-1", "-0", "+0", "00", "01", "031", "1x", "１"] {
            let name = format!("x{suffix}");
            assert!(reg(&name).is_err(), "accepted {name}");
        }
    }

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
        for bad in
            ["2048(sp)", "-2049(sp)", "4((sp))", "4(sp))", "4(sp", "4(x32)", "4(x+1)", "4(x01)"]
        {
            assert!(memory(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn fence_accepts_literal_zero_for_an_empty_set() {
        assert_eq!(fence_mask("0").unwrap(), 0);
    }

    #[test]
    fn fence_requires_unique_known_flags() {
        assert_eq!(fence_mask("iorw").unwrap(), 15);
        assert_eq!(fence_mask("rw").unwrap(), 3);
        for bad in ["rr", "ii", "rwz", "RW", "0r", "r0", "00", "+0", "-0", "0x0"] {
            assert!(fence_mask(bad).is_err(), "{bad}");
        }
    }
}
