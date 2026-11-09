#[derive(Debug, Clone)]
pub struct SIB {
    pub scale: u8, // 0~3
    pub index: u8, // 0~3
    pub base: u8,  // 0~3
}

impl SIB {
    pub fn new(scale: u8, index: u8, base: u8) -> Self {
        Self { scale, index, base }
    }

    pub fn encode(&self) -> u8 {
        #[cfg(whale_wave_amd64)]
        return crate::encoder::wave::bits(
            2,
            u64::from(self.scale),
            u64::from(self.index),
            u64::from(self.base),
            0,
        );
        #[cfg(not(whale_wave_amd64))]
        {
            ((self.scale & 3) << 6) | ((self.index & 7) << 3) | (self.base & 7)
        }
    }
}
