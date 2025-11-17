use crate::ast::MemoryOperand;
use crate::error::AsmError;
use crate::isa::amd64::encoding::modrm::ModRM;
use crate::isa::amd64::encoding::sib::SIB;
use crate::isa::amd64::tables::{REGISTERS_32, REGISTERS_64};

#[derive(Debug, Clone)]
pub struct EncodedAddress {
    pub modrm: ModRM,
    pub sib: Option<SIB>,
    pub disp: Option<Vec<u8>>,  // disp8 or disp32
}

fn reg_code(name: &str, mode: u8) -> Option<u8> {
    let regs = match mode {
        64 => REGISTERS_64,
        32 => REGISTERS_32,
        _ => return None,
    };

    regs.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, code)| *code)
}

pub fn encode_address(mem: &MemoryOperand, reg_for_reg_field: u8) -> Result<EncodedAddress, AsmError> {
    let base = mem.base.as_deref();
    let index = mem.index.as_deref();
    let scale = mem.scale;
    let disp = mem.disp;

    // CASE 1: [reg] — no displacement
    if let Some(b) = base {
        let base_code = reg_code(b, 64).ok_or_else(|| AsmError::EncodeError("Invalid base register".into()))?;

        // special case: [rbp] must use disp8=0
        if base == Some("rbp") && disp == 0 {
            return Ok(EncodedAddress {
                modrm: ModRM::new(1, reg_for_reg_field, base_code),
                sib: None,
                disp: Some(vec![0x00]),
            });
        }

        // normal [reg] no displacement
        if disp == 0 {
            return Ok(EncodedAddress {
                modrm: ModRM::new(0, reg_for_reg_field, base_code),
                sib: None,
                disp: None,
            });
        }

        // disp8
        if disp >= -128 && disp <= 127 {
            return Ok(EncodedAddress {
                modrm: ModRM::new(1, reg_for_reg_field, base_code),
                sib: None,
                disp: Some(vec![disp as u8]),
            });
        }

        // disp32
        return Ok(EncodedAddress {
            modrm: ModRM::new(2, reg_for_reg_field, base_code),
            sib: None,
            disp: Some((disp as i32).to_le_bytes().to_vec()),
        });
    }

    Err(AsmError::EncodeError("Complex memory addressing not yet implemented".into()))
}