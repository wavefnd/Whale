use crate::error::{AsmError, SourceSpan};

#[derive(Debug, Clone)]
pub struct AST {
    pub items: Vec<ASTNode>,
    /// Source ranges remain parallel to items through encoding/constant passes.
    pub spans: Vec<SourceSpan>,
    pub source_name: String,
}

impl AST {
    /// Manually constructed ASTs may omit source metadata.
    pub fn new(items: Vec<ASTNode>) -> Self {
        Self { items, spans: Vec::new(), source_name: "<input>".into() }
    }

    pub fn error_at(&self, error: AsmError, item: usize, previous: Option<usize>) -> AsmError {
        let Some(span) = self.spans.get(item).copied() else {
            return error;
        };
        error.at(&self.source_name, span, previous.and_then(|item| self.spans.get(item).copied()))
    }
}

#[derive(Debug, Clone)]
pub enum ASTNode {
    Instruction(Instruction),
    SymbolMetadata {
        name: String,
        kind: crate::SymbolKind,
        size: ExprValue,
        visibility: crate::SymbolVisibility,
    },
    Directive(Directive),
    Label(String),
    Section(String),
    SectionAttributes {
        name: String,
        kind: AsmSectionKind,
    },
    Global(String),
    Extern(String),
    Const {
        name: String,
        expr: ExprValue,
    },
}

#[derive(Debug, Clone)]
pub struct Instruction {
    pub mnemonic: String,
    pub operands: Vec<Operand>,
}

#[derive(Debug, Clone)]
pub struct Directive {
    pub name: String,
    pub values: Vec<DirectiveValue>,
}

#[derive(Debug, Clone)]
pub enum DirectiveValue {
    Expr(ExprValue),
    StringLiteral(Vec<u8>),
}

#[derive(Debug, Clone)]
pub enum ExprValue {
    Number(i128),
    Symbol { name: String, addend: i128 },
}

#[derive(Debug, Clone)]
pub enum Operand {
    Register(String),
    Immediate(i128),
    Label(String),
    SymbolExpr { name: String, addend: i128 },
    Memory(MemoryOperand),
}

/// Explicit intent for base-less AMD64 memory addresses. Default preserves the
/// original symbol-only RIP-relative and register-based address rules.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAddressMode {
    #[default]
    Default,
    Relative,
    Absolute,
}

#[derive(Debug, Clone)]
pub struct MemoryOperand {
    pub address_mode: MemoryAddressMode,
    /// Explicit data width in bits; absence permits register inference.
    pub width: Option<u8>,
    pub base: Option<String>,
    pub index: Option<String>,
    pub scale: u8,
    pub disp: i64,
    pub symbol: Option<String>,
}

/// Supported initial section identities, independent of the section spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsmSectionKind {
    Text,
    Data,
    ReadOnlyData,
    Bss,
}

impl AsmSectionKind {
    pub fn default_for_name(name: &str) -> Self {
        match name {
            ".text" => Self::Text,
            ".rodata" => Self::ReadOnlyData,
            ".bss" => Self::Bss,
            _ => Self::Data,
        }
    }

    pub fn default_alignment(self) -> u64 {
        if self == Self::Text {
            16
        } else {
            1
        }
    }
}
