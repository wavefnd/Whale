#[derive(Debug, Clone)]
pub struct ModRM {
    pub mod_bits: u8,       // 0~3
    pub reg: u8,            // 0~7
    pub rm: u8,             // 0~7
}

impl ModRM {
    pub fn new(mod_bits: u8, reg: u8, rm: u8) -> Self {
        Self { mod_bits, reg, rm }
    }
    
    pub fn encode(&self) -> u8 {
        (self.mod_bits << 6) | ((self.reg & 7) << 3) | (self.rm & 7)
    }
}