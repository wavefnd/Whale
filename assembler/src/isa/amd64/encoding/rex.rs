#[derive(Debug, Clone)]
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
        0x40
        | ((self.w as u8) << 3)
        | ((self.r as u8) << 2)
        | ((self.x as u8) << 1)
        | (self.b as u8)
    }
}