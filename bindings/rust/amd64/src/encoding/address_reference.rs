use super::{DispKind, EncodedAddress};

use crate::tables::{REGISTERS_32, REGISTERS_64};

use assembler::ast::{MemoryAddressMode, MemoryOperand};

use assembler::error::AsmError;

fn reg_code(name: &str, mode: u8) -> Option<u8> {
    let regs = match mode {
        64 => REGISTERS_64,
        32 => REGISTERS_32,
        _ => return None,
    };

    regs.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

fn scale_bits(scale: u8) -> Option<u8> {
    match scale {
        1 => Some(0),
        2 => Some(1),
        4 => Some(2),
        8 => Some(3),
        _ => None,
    }
}

pub fn encode_address(mem: &MemoryOperand, mode: u8) -> Result<EncodedAddress, AsmError> {
    if mem.symbol.is_some() {
        return Err(AsmError::EncodeError(
            "symbol memory should be encoded via rip-relative path".into(),
        ));
    }

    if mem.address_mode != MemoryAddressMode::Default {
        if mode != 64 || mem.base.is_some() || mem.index.is_some() || mem.scale != 1 {
            return Err(AsmError::EncodeError(
                "rel/abs requires a base-less long-mode address".into(),
            ));
        }
        let displacement = i32::try_from(mem.disp).map_err(|_| {
            AsmError::EncodeError("rel/abs displacement is out of signed 32-bit range".into())
        })?;
        return Ok(EncodedAddress {
            mod_bits: 0,
            rm_bits: if mem.address_mode == MemoryAddressMode::Relative { 5 } else { 4 },
            sib: if mem.address_mode == MemoryAddressMode::Absolute {
                Some((0, 4, 5))
            } else {
                None
            },
            disp: Some(DispKind::Disp32(displacement)),
            rex_b: false,
            rex_x: false,
        });
    }

    let base_code = mem
        .base
        .as_deref()
        .map(|b| {
            reg_code(b, mode).ok_or_else(|| AsmError::EncodeError("Invalid base register".into()))
        })
        .transpose()?;
    let index_code = mem
        .index
        .as_deref()
        .map(|i| {
            reg_code(i, mode).ok_or_else(|| AsmError::EncodeError("Invalid index register".into()))
        })
        .transpose()?;

    if base_code.is_none() && index_code.is_none() {
        return Err(AsmError::EncodeError("Addressing without base/index not implemented".into()));
    }

    let scale = scale_bits(mem.scale)
        .ok_or_else(|| AsmError::EncodeError("Scale must be 1/2/4/8".into()))?;

    if let Some(idx) = index_code {
        if idx == 4 {
            return Err(AsmError::EncodeError("rsp/esp cannot be used as index register".into()));
        }
    }

    let disp = mem.disp;
    let disp32 = i32::try_from(disp).map_err(|_| {
        AsmError::EncodeError("memory displacement is out of signed 32-bit range".into())
    })?;
    let disp_kind = if disp == 0 {
        None
    } else if (-128..=127).contains(&disp) {
        Some(DispKind::Disp8(disp as i8))
    } else {
        Some(DispKind::Disp32(disp32))
    };

    let need_sib = index_code.is_some()
        || base_code.is_none()
        || base_code.map(|b| (b & 7) == 4).unwrap_or(false);

    let (mod_bits, final_disp, rm_bits, sib, rex_b, rex_x) = match (base_code, index_code, need_sib)
    {
        (Some(base), idx, true) => {
            let base_low = base & 7;
            let mut mod_bits = 0;
            let mut disp = disp_kind;

            if base_low == 5 && disp.is_none() {
                mod_bits = 1;
                disp = Some(DispKind::Disp8(0));
            } else if matches!(disp, Some(DispKind::Disp8(_))) {
                mod_bits = 1;
            } else if matches!(disp, Some(DispKind::Disp32(_))) {
                mod_bits = 2;
            }

            let index_low = idx.map(|i| i & 7).unwrap_or(4);
            let base_low = base & 7;
            (
                mod_bits,
                disp,
                4,
                Some((scale, index_low, base_low)),
                base >= 8,
                idx.map(|i| i >= 8).unwrap_or(false),
            )
        },
        (Some(base), _, false) => {
            let base_low = base & 7;
            let mut mod_bits = 0;
            let mut disp = disp_kind;

            if base_low == 5 && disp.is_none() {
                mod_bits = 1;
                disp = Some(DispKind::Disp8(0));
            } else if matches!(disp, Some(DispKind::Disp8(_))) {
                mod_bits = 1;
            } else if matches!(disp, Some(DispKind::Disp32(_))) {
                mod_bits = 2;
            }

            (mod_bits, disp, base_low, None, base >= 8, false)
        },
        (None, Some(idx), _) => {
            let index_low = idx & 7;
            (0, Some(DispKind::Disp32(disp32)), 4, Some((scale, index_low, 5)), false, idx >= 8)
        },
        _ => return Err(AsmError::EncodeError("Addressing mode is not supported yet".into())),
    };

    Ok(EncodedAddress { mod_bits, rm_bits, sib, disp: final_disp, rex_b, rex_x })
}
