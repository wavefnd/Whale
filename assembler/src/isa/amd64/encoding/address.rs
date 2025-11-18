use crate::ast::MemoryOperand;
use crate::error::AsmError;
use crate::isa::amd64::tables::{REGISTERS_32, REGISTERS_64};

/// displacement 크기
#[derive(Debug, Clone)]
pub enum DispKind {
    Disp8(i8),
    Disp32(i32),
}

/// Memory addressing 인코딩 결과
#[derive(Debug, Clone)]
pub struct EncodedAddress {
    pub mod_bits: u8,
    pub rm_bits: u8,
    pub sib: Option<(u8, u8, u8)>, // (scale, index, base)
    pub disp: Option<DispKind>,

    /// REX 확장 정보
    pub rex_b: bool,
    pub rex_x: bool,
}

/// base/index → 레지스터 번호 찾기
fn reg_code(name: &str, mode: u8) -> Option<u8> {
    let regs = match mode {
        64 => REGISTERS_64,
        32 => REGISTERS_32,
        _ => return None,
    };

    regs.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// Encodes x86-64 memory operand
///
/// Supports:
/// - [base]
/// - [base + disp8]
/// - [base + disp32]
/// - special case [rbp], [rbp + 0]
/// - automatic REX.B
///
/// Does NOT (yet) support:
/// - index register
/// - scale factor
/// - sib addressing
pub fn encode_address(mem: &MemoryOperand, mode: u8) -> Result<EncodedAddress, AsmError> {
    let base = mem.base.as_deref();
    let index = mem.index.as_deref();
    let scale = mem.scale;
    let disp = mem.disp;

    if index.is_some() {
        return Err(AsmError::EncodeError(
            "index-based addressing not implemented yet (A-Step)".into(),
        ));
    }

    if let Some(base_reg) = base {
        let base_code =
            reg_code(base_reg, mode).ok_or_else(|| AsmError::EncodeError("Invalid base register".into()))?;

        let rex_b = base_code > 7; // REX.B = high registers (r8–r15)
        let rm = base_code & 7;

        if base_reg == "rbp" && disp == 0 {
            return Ok(EncodedAddress {
                mod_bits: 1,      // 01b = disp8
                rm_bits: rm,
                sib: None,
                disp: Some(DispKind::Disp8(0)),
                rex_b,
                rex_x: false,
            });
        }

        if disp == 0 {
            return Ok(EncodedAddress {
                mod_bits: 0,      // 00b
                rm_bits: rm,
                sib: None,
                disp: None,
                rex_b,
                rex_x: false,
            });
        }

        if (-128..=127).contains(&disp) {
            return Ok(EncodedAddress {
                mod_bits: 1,  // 01b
                rm_bits: rm,
                sib: None,
                disp: Some(DispKind::Disp8(disp as i8)),
                rex_b,
                rex_x: false,
            });
        }

        return Ok(EncodedAddress {
            mod_bits: 2, // 10b
            rm_bits: rm,
            sib: None,
            disp: Some(DispKind::Disp32(disp as i32)),
            rex_b,
            rex_x: false,
        });
    }

    Err(AsmError::EncodeError(
        "Addressing without base register not implemented yet".into(),
    ))
}
