use assembler::ast::MemoryOperand;

use assembler::error::AsmError;

#[derive(Debug, Clone)]
pub enum DispKind {
    Disp8(i8),
    Disp32(i32),
}

#[derive(Debug, Clone)]
pub struct EncodedAddress {
    pub mod_bits: u8,
    pub rm_bits: u8,
    pub sib: Option<(u8, u8, u8)>, // (scale, index, base)
    pub disp: Option<DispKind>,

    pub rex_b: bool,
    pub rex_x: bool,
}

#[cfg(any(test, not(whale_wave_amd64)))]
#[path = "address_reference.rs"]
pub(crate) mod reference;

pub fn encode_address(mem: &MemoryOperand, mode: u8) -> Result<EncodedAddress, AsmError> {
    #[cfg(whale_wave_amd64)]
    return crate::encoder::wave::address(mem, mode);
    #[cfg(not(whale_wave_amd64))]
    reference::encode_address(mem, mode)
}
