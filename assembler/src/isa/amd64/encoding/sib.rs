#[derive(Debug, Clone)]
pub struct SIB {
    pub scale: u8,          // 0~3
    pub index: u8,          // 0~3
    pub base: u8,           // 0~3
}

impl SIB {
    pub fn new(scale: u8, index: u8, base: u8) -> Self {
        Self { scale, index, base }
    }
    
    pub fn encode(&self) -> u8 {
        ((self.scale & 3) << 6) | ((self.index & 7) << 3) | (self.base & 7)
    }
}