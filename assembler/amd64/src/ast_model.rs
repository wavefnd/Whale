// SPDX-License-Identifier: MPL-2.0
//! Structural data packets; instruction and expression policy lives in Wave.
use assembler::ast::*;

use assembler::{AsmError, SymbolKind, SymbolVisibility};

use std::marker::PhantomData;

pub(crate) const MISSING: u64 = u64::MAX;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Name {
    pub(crate) bytes: *const u8,
    pub(crate) count: u64,
}

impl Default for Name {
    fn default() -> Self {
        Self { bytes: std::ptr::null(), count: 0 }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Expr {
    pub(crate) kind: u64,
    pub(crate) name: u64,
    pub(crate) low: u64,
    pub(crate) high: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct OperandRecord {
    pub(crate) kind: u64,
    pub(crate) name: u64,
    pub(crate) low: u64,
    pub(crate) high: u64,
    pub(crate) mode: u64,
    pub(crate) width: u64,
    pub(crate) base: u64,
    pub(crate) index: u64,
    pub(crate) scale: u64,
    pub(crate) disp: u64,
    pub(crate) symbol: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Item {
    pub(crate) kind: u64,
    pub(crate) name: u64,
    pub(crate) aux: u64,
    pub(crate) flags: u64,
    pub(crate) expression: Expr,
    pub(crate) first: u64,
    pub(crate) count: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct DirectiveRecord {
    pub(crate) kind: u64,
    pub(crate) expression: Expr,
    pub(crate) bytes: *const u8,
    pub(crate) count: u64,
}

impl Default for DirectiveRecord {
    fn default() -> Self {
        Self { kind: 0, expression: Expr::default(), bytes: std::ptr::null(), count: 0 }
    }
}

pub(crate) fn transport() -> AsmError {
    AsmError::Backend("AMD64 AST transport")
}

pub(crate) fn push<T>(values: &mut Vec<T>, value: T) -> Result<u64, AsmError> {
    values.try_reserve(1).map_err(|_| transport())?;
    let index = values.len() as u64;
    values.push(value);
    Ok(index)
}

pub(crate) struct Packet<'a> {
    pub(crate) names: Vec<Name>,
    pub(crate) items: Vec<Item>,
    pub(crate) operands: Vec<OperandRecord>,
    pub(crate) values: Vec<DirectiveRecord>,
    borrowed: PhantomData<&'a AST>,
}

impl<'a> Packet<'a> {
    pub(crate) fn new() -> Self {
        Self {
            names: Vec::new(),
            items: Vec::new(),
            operands: Vec::new(),
            values: Vec::new(),
            borrowed: PhantomData,
        }
    }

    fn name(&mut self, bytes: &'a [u8]) -> Result<u64, AsmError> {
        push(&mut self.names, Name { bytes: bytes.as_ptr(), count: bytes.len() as u64 })
    }

    fn expression(&mut self, expression: &'a ExprValue) -> Result<Expr, AsmError> {
        let (kind, name, bits) = match expression {
            ExprValue::Number(number) => (0, MISSING, *number as u128),
            ExprValue::Symbol { name, addend } => (1, self.name(name.as_bytes())?, *addend as u128),
        };
        Ok(Expr { kind, name, low: bits as u64, high: (bits >> 64) as u64 })
    }

    fn operand(&mut self, operand: &'a Operand) -> Result<OperandRecord, AsmError> {
        let mut raw = OperandRecord {
            name: MISSING,
            base: MISSING,
            index: MISSING,
            symbol: MISSING,
            width: MISSING,
            scale: 1,
            ..OperandRecord::default()
        };
        match operand {
            Operand::Register(name) => {
                raw.kind = 0;
                raw.name = self.name(name.as_bytes())?;
            },
            Operand::Immediate(value) => {
                raw.kind = 1;
                raw.low = *value as u128 as u64;
                raw.high = ((*value as u128) >> 64) as u64;
            },
            Operand::Label(name) => {
                raw.kind = 2;
                raw.name = self.name(name.as_bytes())?;
            },
            Operand::SymbolExpr { name, addend } => {
                raw.kind = 3;
                raw.name = self.name(name.as_bytes())?;
                raw.low = *addend as u128 as u64;
                raw.high = ((*addend as u128) >> 64) as u64;
            },
            Operand::Memory(memory) => {
                raw.kind = 4;
                raw.mode = match memory.address_mode {
                    MemoryAddressMode::Default => 0,
                    MemoryAddressMode::Relative => 1,
                    MemoryAddressMode::Absolute => 2,
                };
                raw.width = memory.width.map_or(MISSING, u64::from);
                raw.base = memory
                    .base
                    .as_ref()
                    .map(|name| self.name(name.as_bytes()))
                    .transpose()?
                    .unwrap_or(MISSING);
                raw.index = memory
                    .index
                    .as_ref()
                    .map(|name| self.name(name.as_bytes()))
                    .transpose()?
                    .unwrap_or(MISSING);
                raw.symbol = memory
                    .symbol
                    .as_ref()
                    .map(|name| self.name(name.as_bytes()))
                    .transpose()?
                    .unwrap_or(MISSING);
                raw.scale = u64::from(memory.scale);
                raw.disp = memory.disp as u64;
            },
        }
        Ok(raw)
    }

    pub(crate) fn from_ast(ast: &'a AST) -> Result<Self, AsmError> {
        let mut packet = Self::new();
        for item in &ast.items {
            let mut raw = Item { name: MISSING, ..Item::default() };
            match item {
                ASTNode::Instruction(instruction) => {
                    raw.name = packet.name(instruction.mnemonic.as_bytes())?;
                    raw.first = packet.operands.len() as u64;
                    raw.count = instruction.operands.len() as u64;
                    for operand in &instruction.operands {
                        let native = packet.operand(operand)?;
                        push(&mut packet.operands, native)?;
                    }
                },
                ASTNode::SymbolMetadata { name, kind, size, visibility } => {
                    raw.kind = 1;
                    raw.name = packet.name(name.as_bytes())?;
                    raw.aux = match kind {
                        SymbolKind::NoType => 0,
                        SymbolKind::Function => 1,
                        SymbolKind::Object => 2,
                    };
                    raw.flags = match visibility {
                        SymbolVisibility::Default => 0,
                        SymbolVisibility::Hidden => 1,
                    };
                    raw.expression = packet.expression(size)?;
                },
                ASTNode::Directive(directive) => {
                    raw.kind = 2;
                    raw.name = packet.name(directive.name.as_bytes())?;
                    raw.first = packet.values.len() as u64;
                    raw.count = directive.values.len() as u64;
                    for value in &directive.values {
                        let native = match value {
                            DirectiveValue::Expr(expression) => DirectiveRecord {
                                expression: packet.expression(expression)?,
                                ..DirectiveRecord::default()
                            },
                            DirectiveValue::StringLiteral(bytes) => DirectiveRecord {
                                kind: 1,
                                bytes: bytes.as_ptr(),
                                count: bytes.len() as u64,
                                ..DirectiveRecord::default()
                            },
                        };
                        push(&mut packet.values, native)?;
                    }
                },
                ASTNode::Label(name)
                | ASTNode::Section(name)
                | ASTNode::Global(name)
                | ASTNode::Extern(name) => {
                    raw.kind = match item {
                        ASTNode::Label(_) => 3,
                        ASTNode::Section(_) => 4,
                        ASTNode::Global(_) => 6,
                        _ => 7,
                    };
                    raw.name = packet.name(name.as_bytes())?;
                },
                ASTNode::SectionAttributes { name, kind } => {
                    raw.kind = 5;
                    raw.name = packet.name(name.as_bytes())?;
                    raw.aux = match kind {
                        AsmSectionKind::Text => 0,
                        AsmSectionKind::Data => 1,
                        AsmSectionKind::ReadOnlyData => 2,
                        AsmSectionKind::Bss => 3,
                    };
                },
                ASTNode::Const { name, expr } => {
                    raw.kind = 8;
                    raw.name = packet.name(name.as_bytes())?;
                    raw.expression = packet.expression(expr)?;
                },
            }
            push(&mut packet.items, raw)?;
        }
        Ok(packet)
    }
}
