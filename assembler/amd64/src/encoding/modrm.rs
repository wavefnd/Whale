#[derive(Debug, Clone)]
pub struct ModRM {
    pub mod_bits: u8, // 0~3
    pub reg: u8,      // 0~7
    pub rm: u8,       // 0~7
}

impl ModRM {
    pub fn new(mod_bits: u8, reg: u8, rm: u8) -> Self {
        Self { mod_bits, reg, rm }
    }

    pub fn encode(&self) -> u8 {
        #[cfg(whale_wave_amd64)]
        return crate::encoder::wave::bits(
            0,
            u64::from(self.mod_bits),
            u64::from(self.reg),
            u64::from(self.rm),
            0,
        );
        #[cfg(not(whale_wave_amd64))]
        {
            (self.mod_bits << 6) | ((self.reg & 7) << 3) | (self.rm & 7)
        }
    }
}
