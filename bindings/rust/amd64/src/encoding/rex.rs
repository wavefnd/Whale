#[derive(Debug, Clone, Default)]
pub struct REX {
    pub w: bool,
    pub r: bool,
    pub x: bool,
    pub b: bool,
}

impl REX {
    pub fn new() -> Self {
        Self { w: false, r: false, x: false, b: false }
    }

    pub fn encode(&self) -> u8 {
        #[cfg(whale_wave_amd64)]
        return crate::encoder::wave::bits(
            1,
            u64::from(self.w),
            u64::from(self.r),
            u64::from(self.x),
            u64::from(self.b),
        );
        #[cfg(not(whale_wave_amd64))]
        {
            0x40 | ((self.w as u8) << 3)
                | ((self.r as u8) << 2)
                | ((self.x as u8) << 1)
                | (self.b as u8)
        }
    }
}
