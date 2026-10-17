// SPDX-License-Identifier: MPL-2.0
//! Two-pass source/section handling for fixed-width little-endian ISAs.
//! Instruction syntax and bit encodings belong to the implementation crate.
use crate::{
    AsmError, AsmSection, AsmSectionKind, AsmSymbol, AssemblerOutput, AssemblyLimits, RelocKind,
    Relocation, SourceSpan, SymbolKind, SymbolVisibility,
};

use std::collections::BTreeMap;

pub struct Instruction {
    pub mnemonic: String,
    pub operands: Vec<String>,
}

pub trait FixedIsa {
    fn id(&self) -> &'static str;

    fn hash_comments(&self) -> bool {
        true
    }

    fn encode(&self, instruction: &Instruction, context: &mut Context<'_>)
        -> Result<u32, AsmError>;
}

struct Symbol {
    section: Option<usize>,
    offset: u64,
    span: SourceSpan,
}

pub struct Context<'a> {
    namespace: &'static str,
    section: usize,
    offset: u64,
    symbols: &'a BTreeMap<String, Symbol>,
    relocations: &'a mut Vec<Relocation>,
}

impl Context<'_> {
    /// Numeric operands are byte displacements. Local labels use this instruction's
    /// PC; cross-section and external symbols retain an ELF relocation.
    pub fn pc_relative(&mut self, operand: &str, code: u32) -> Result<i64, AsmError> {
        match expression(operand)? {
            Expression::Number(value) => {
                i64::try_from(value).map_err(|_| error("branch displacement exceeds i64"))
            },
            Expression::Symbol(name, addend) => {
                let symbol = self.symbols.get(&name).ok_or_else(|| {
                    error(format!("undefined symbol {name:?}; use .extern for external references"))
                })?;
                if symbol.section == Some(self.section) {
                    i64::try_from(
                        i128::from(symbol.offset) + i128::from(addend) - i128::from(self.offset),
                    )
                    .map_err(|_| error("branch displacement exceeds i64"))
                } else {
                    self.relocations.push(Relocation {
                        offset: self.offset as usize,
                        symbol: name,
                        addend,
                        kind: RelocKind::TargetSpecific { namespace: self.namespace.into(), code },
                    });
                    Ok(0)
                }
            },
        }
    }
}

pub fn error(message: impl Into<String>) -> AsmError {
    AsmError::EncodeError(message.into())
}

pub fn count(instruction: &Instruction, expected: usize) -> Result<&[String], AsmError> {
    if instruction.operands.len() != expected {
        return Err(error(format!("{} requires {expected} operands", instruction.mnemonic)));
    }
    Ok(&instruction.operands)
}

pub fn integer(text: &str) -> Result<i128, AsmError> {
    let text = text.trim().strip_prefix('#').unwrap_or(text.trim()).trim();
    let (negative, text) = if let Some(s) = text.strip_prefix('-') {
        (true, s)
    } else {
        (false, text.strip_prefix('+').unwrap_or(text))
    };
    let text = text.trim();
    if !text.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(error(format!("invalid integer {text:?}")));
    }
    let clean = text.replace('_', "");
    let (digits, radix) =
        if let Some(s) = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")) {
            (s, 16)
        } else if let Some(s) = clean.strip_prefix("0b").or_else(|| clean.strip_prefix("0B")) {
            (s, 2)
        } else {
            (clean.as_str(), 10)
        };
    let value = i128::from_str_radix(digits, radix)
        .map_err(|_| error(format!("invalid integer {text:?}")))?;
    if negative {
        value.checked_neg().ok_or_else(|| error("integer overflow"))
    } else {
        Ok(value)
    }
}

pub fn signed(text: &str, bits: u32) -> Result<u32, AsmError> {
    if !(1..=32).contains(&bits) {
        return Err(error("invalid immediate field width"));
    }
    let value = integer(text)?;
    if value < -(1i128 << (bits - 1)) || value >= (1i128 << (bits - 1)) {
        return Err(error(format!("immediate must fit signed {bits} bits")));
    }
    Ok((value as u32) & ((1u64 << bits) - 1) as u32)
}

pub fn unsigned(text: &str, bits: u32) -> Result<u32, AsmError> {
    if !(1..=32).contains(&bits) {
        return Err(error("invalid immediate field width"));
    }
    let value = integer(text)?;
    if value < 0 || value >= (1i128 << bits) {
        return Err(error(format!("immediate must fit unsigned {bits} bits")));
    }
    Ok(value as u32)
}

pub fn displacement(value: i64, bits: u32, alignment: i64) -> Result<u32, AsmError> {
    if !(1..=32).contains(&bits) || alignment <= 0 {
        return Err(error("invalid displacement field"));
    }
    if value % alignment != 0 {
        return Err(error(format!("branch displacement must be aligned to {alignment} bytes")));
    }
    let scaled = value / alignment;
    if scaled < -(1i64 << (bits - 1)) || scaled >= (1i64 << (bits - 1)) {
        return Err(error(format!("branch displacement exceeds signed {bits}-bit scaled range")));
    }
    Ok((scaled as u32) & ((1u64 << bits) - 1) as u32)
}

enum Expression {
    Number(i128),
    Symbol(String, i64),
}

fn expression(text: &str) -> Result<Expression, AsmError> {
    if let Ok(value) = integer(text) {
        return Ok(Expression::Number(value));
    }
    let text = text.trim();
    let split = text.char_indices().skip(1).find(|(_, c)| matches!(c, '+' | '-')).map(|(i, _)| i);
    let (name, addend) = if let Some(i) = split {
        (
            text[..i].trim(),
            i64::try_from(integer(&text[i..])?).map_err(|_| error("symbol addend exceeds i64"))?,
        )
    } else {
        (text, 0)
    };
    identifier(name)?;
    Ok(Expression::Symbol(name.into(), addend))
}

fn identifier(name: &str) -> Result<(), AsmError> {
    let mut chars = name.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '_' | '.' | '$'))
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'))
    {
        return Err(error(format!("invalid symbol name {name:?}")));
    }
    Ok(())
}

pub fn operands(text: &str) -> Result<Vec<String>, AsmError> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut stack = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '[' | '(' => stack.push(c),
            ']' | ')' => {
                let expected = if c == ']' { '[' } else { '(' };
                if stack.pop() != Some(expected) {
                    return Err(error("unbalanced memory operand"));
                }
            },
            ',' if stack.is_empty() => {
                if result.len() >= 65536 {
                    return Err(error("operand count exceeds 65536"));
                }
                result.push(text[start..i].trim().into());
                start = i + 1;
            },
            _ => {},
        }
    }
    if !stack.is_empty() {
        return Err(error("unclosed memory operand"));
    }
    result.push(text[start..].trim().into());
    if result.len() > 65536 {
        return Err(error("operand count exceeds 65536"));
    }
    if result.iter().any(String::is_empty) {
        return Err(error("empty operand"));
    }
    Ok(result)
}

enum Action {
    Instruction(Instruction),
    Data(usize, Vec<String>),
    Zero(usize),
    Align(u64),
}

struct Record {
    section: usize,
    offset: u64,
    span: SourceSpan,
    action: Action,
}

/// No expansion, relaxation or external process: every instruction is four bytes.
pub fn assemble(
    isa: &impl FixedIsa,
    name: &str,
    source: &str,
    limits: &AssemblyLimits,
) -> Result<AssemblerOutput, AsmError> {
    if source.len() > 8 * 1024 * 1024 {
        return Err(error("assembly input exceeds 8388608 bytes"));
    }
    if limits.max_relax_iterations == 0 {
        return Err(error("assembly pass budget must be nonzero"));
    }
    let mut sections = vec![AsmSection {
        name: ".text".into(),
        kind: AsmSectionKind::Text,
        align: 4,
        data: vec![],
        zero_fill: 0,
        relocs: vec![],
    }];
    let mut section_names = BTreeMap::from([(".text".to_owned(), 0usize)]);
    let mut sizes = vec![0u64];
    let mut selected = 0;
    let mut symbols: BTreeMap<String, Symbol> = BTreeMap::new();
    let mut globals = BTreeMap::new();
    let mut records = Vec::new();
    let mut materialized = 0usize;
    let mut byte_start = 0;
    for (line_index, original) in source.split_inclusive('\n').enumerate() {
        let span = SourceSpan {
            start: byte_start,
            end: byte_start + original.trim_end_matches(['\r', '\n']).len(),
            line: line_index + 1,
            column: 1,
        };
        byte_start += original.len();
        let process = (|| -> Result<(), AsmError> {
            let end = original
                .find("//")
                .into_iter()
                .chain(original.find(';'))
                .chain(if isa.hash_comments() { original.find('#') } else { None })
                .min()
                .unwrap_or(original.len());
            let mut text = original[..end].trim();
            while let Some(colon) = text.find(':') {
                let label = text[..colon].trim();
                identifier(label)?;
                if symbols.len() >= 65536 {
                    return Err(error("symbol count exceeds 65536"));
                }
                if let Some(previous) = symbols.get(label) {
                    return Err(error(format!("duplicate symbol {label:?}")).at(
                        name,
                        span,
                        Some(previous.span),
                    ));
                }
                symbols.insert(
                    label.into(),
                    Symbol { section: Some(selected), offset: sizes[selected], span },
                );
                text = text[colon + 1..].trim();
            }
            if text.is_empty() {
                return Ok(());
            }
            let word_end = text.find(char::is_whitespace).unwrap_or(text.len());
            let mnemonic = text[..word_end].to_ascii_lowercase();
            let tail = text[word_end..].trim();
            if matches!(mnemonic.as_str(), ".text" | ".data" | ".rodata" | ".bss" | ".section") {
                let section_name = if mnemonic == ".section" {
                    tail
                } else {
                    if !tail.is_empty() {
                        return Err(error("section directive takes no operands"));
                    }
                    &mnemonic
                };
                identifier(section_name)?;
                selected = if let Some(&index) = section_names.get(section_name) {
                    index
                } else {
                    let index = sections.len();
                    if index >= 4096 {
                        return Err(error("section count exceeds 4096"));
                    }
                    let kind = AsmSectionKind::default_for_name(section_name);
                    sections.push(AsmSection {
                        name: section_name.into(),
                        kind,
                        align: if kind == AsmSectionKind::Text { 4 } else { 1 },
                        data: vec![],
                        zero_fill: 0,
                        relocs: vec![],
                    });
                    sizes.push(0);
                    section_names.insert(section_name.into(), index);
                    index
                };
                return Ok(());
            }
            if matches!(mnemonic.as_str(), ".global" | ".globl" | ".extern") {
                let names = operands(tail)?;
                if names.is_empty() {
                    return Err(error("symbol directive requires a name"));
                }
                for symbol in names {
                    identifier(&symbol)?;
                    if globals.len() >= 65536 {
                        return Err(error("global count exceeds 65536"));
                    }
                    globals.insert(symbol.clone(), span);
                    if mnemonic == ".extern" {
                        if let Some(previous) = symbols.get(&symbol) {
                            if previous.section.is_some() {
                                return Err(error("external symbol is already defined"));
                            }
                        } else {
                            if symbols.len() >= 65536 {
                                return Err(error("symbol count exceeds 65536"));
                            }
                            symbols.insert(symbol, Symbol { section: None, offset: 0, span });
                        }
                    }
                }
                return Ok(());
            }
            let action = match mnemonic.as_str() {
                ".byte" | ".hword" | ".short" | ".word" | ".long" | ".quad" => {
                    let width = match mnemonic.as_str() {
                        ".byte" => 1,
                        ".hword" | ".short" => 2,
                        ".quad" => 8,
                        _ => 4,
                    };
                    let values = operands(tail)?;
                    if values.is_empty() {
                        return Err(error("data directive requires values"));
                    }
                    Action::Data(width, values)
                },
                ".zero" | ".space" => Action::Zero(
                    usize::try_from(integer(tail)?)
                        .map_err(|_| error("invalid reservation size"))?,
                ),
                ".balign" | ".p2align" | ".align" => {
                    let value =
                        u64::try_from(integer(tail)?).map_err(|_| error("invalid alignment"))?;
                    let alignment = if mnemonic == ".balign" {
                        value
                    } else {
                        1u64.checked_shl(u32::try_from(value).unwrap_or(u32::MAX))
                            .ok_or_else(|| error("alignment overflow"))?
                    };
                    if !alignment.is_power_of_two() {
                        return Err(error("alignment must be a nonzero power of two"));
                    }
                    sections[selected].align = sections[selected].align.max(alignment);
                    Action::Align(alignment)
                },
                _ if mnemonic.starts_with('.') => {
                    return Err(error(format!("unsupported directive {mnemonic}")))
                },
                _ => Action::Instruction(Instruction { mnemonic, operands: operands(tail)? }),
            };
            if sections[selected].kind == AsmSectionKind::Bss
                && !matches!(action, Action::Zero(_) | Action::Align(_))
            {
                return Err(error("BSS accepts only labels, alignment and zero reservations"));
            }
            if matches!(action, Action::Instruction(_))
                && (sections[selected].kind != AsmSectionKind::Text || sizes[selected] % 4 != 0)
            {
                return Err(error("instructions require four-byte alignment in a text section"));
            }
            let size = match &action {
                Action::Instruction(_) => 4,
                Action::Data(width, values) => {
                    width.checked_mul(values.len()).ok_or_else(|| error("data size overflow"))?
                        as u64
                },
                Action::Zero(size) => *size as u64,
                Action::Align(align) => (align - sizes[selected] % align) % align,
            };
            let offset = sizes[selected];
            sizes[selected] =
                offset.checked_add(size).ok_or_else(|| error("section size overflow"))?;
            if sections[selected].kind != AsmSectionKind::Bss {
                materialized = materialized
                    .checked_add(
                        usize::try_from(size)
                            .map_err(|_| error("output size exceeds host capacity"))?,
                    )
                    .filter(|n| *n <= limits.max_output_bytes.min(256 * 1024 * 1024))
                    .ok_or_else(|| error("Assembly output limit exceeded"))?;
            }
            if records.len() >= 262144 {
                return Err(error("record count exceeds 262144"));
            }
            records.push(Record { section: selected, offset, span, action });
            Ok(())
        })();
        process.map_err(|e| e.at(name, span, None))?;
    }
    for (global, span) in &globals {
        if !symbols.contains_key(global) {
            return Err(error(format!(
                "global symbol {global:?} has no definition; use .extern for external declarations"
            ))
            .at(name, *span, None));
        }
    }
    for (index, section) in sections.iter_mut().enumerate() {
        if section.kind == AsmSectionKind::Bss {
            section.zero_fill = sizes[index];
        } else {
            section
                .data
                .try_reserve_exact(sizes[index] as usize)
                .map_err(|_| error("unable to reserve assembly output"))?;
        }
    }
    for record in records {
        let section = &mut sections[record.section];
        if section.kind == AsmSectionKind::Bss {
            continue;
        }
        let result = (|| -> Result<(), AsmError> {
            match record.action {
                Action::Instruction(instruction) => {
                    let mut context = Context {
                        namespace: isa.id(),
                        section: record.section,
                        offset: record.offset,
                        symbols: &symbols,
                        relocations: &mut section.relocs,
                    };
                    section
                        .data
                        .extend_from_slice(&isa.encode(&instruction, &mut context)?.to_le_bytes());
                },
                Action::Data(width, values) => {
                    for value in values {
                        let bits = match expression(&value)? {
                            Expression::Number(n) => {
                                if n < -(1i128 << (width * 8 - 1)) || n >= (1i128 << (width * 8)) {
                                    return Err(error("data value does not fit its width"));
                                }
                                n as u64
                            },
                            Expression::Symbol(symbol, addend) => {
                                if !symbols.contains_key(&symbol) {
                                    return Err(error(format!("undefined symbol {symbol:?}")));
                                }
                                let kind = match width {
                                    4 => RelocKind::Absolute32,
                                    8 => RelocKind::Absolute64,
                                    _ => return Err(error("symbol data requires .word or .quad")),
                                };
                                section.relocs.push(Relocation {
                                    offset: section.data.len(),
                                    symbol,
                                    kind,
                                    addend,
                                });
                                0
                            },
                        };
                        section.data.extend_from_slice(&bits.to_le_bytes()[..width]);
                    }
                },
                Action::Zero(size) => section.data.resize(section.data.len() + size, 0),
                Action::Align(align) => {
                    let padding = (align - record.offset % align) % align;
                    section.data.resize(section.data.len() + padding as usize, 0);
                },
            }
            Ok(())
        })();
        result.map_err(|e| e.at(name, record.span, None))?;
    }
    Ok(AssemblerOutput {
        sections,
        symbols: symbols
            .into_iter()
            .map(|(name, symbol)| AsmSymbol {
                is_global: globals.contains_key(&name),
                name,
                section_index: symbol.section,
                offset: symbol.offset,
                kind: SymbolKind::NoType,
                size: 0,
                visibility: SymbolVisibility::Default,
            })
            .collect(),
    })
}
