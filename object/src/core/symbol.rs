#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolBinding {
    Local,
    Global,
    Weak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolVisibility {
    Default,
    Hidden,
}

pub struct ObjectSymbol {
    pub name: String,
    pub section_index: Option<usize>,
    pub value: u64,
    pub size: u64,
    pub binding: SymbolBinding,
    pub visibility: SymbolVisibility,
}
