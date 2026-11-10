// SPDX-License-Identifier: MPL-2.0
//! Allocation and typed AST/output transport. All encoding decisions run in Wave.
use crate::ast_model::{DirectiveRecord, Item, Name, OperandRecord, Packet, MISSING};

use crate::encoding::{DispKind, EncodedAddress};

use assembler::ast::{ASTNode, DirectiveValue, ExprValue, MemoryOperand, Operand, AST};

use assembler::{
    AsmError, AsmSection, AsmSectionKind, AsmSymbol, AssemblerOutput, AssemblyLimits, RelocKind,
    Relocation, SymbolKind, SymbolVisibility,
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Section {
    name: u64,
    kind: u64,
    align: u64,
    first: u64,
    size: u64,
    zero_fill: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Symbol {
    prefix: u64,
    name: u64,
    flags: u64,
    label_site: u64,
    extern_site: u64,
    global_site: u64,
    constant_site: u64,
    metadata_site: u64,
    section: u64,
    offset: u64,
    previous_section: u64,
    previous_offset: u64,
    low: u64,
    high: u64,
    dependency: u64,
    add_low: u64,
    add_high: u64,
    symbol_kind: u64,
    size: u64,
    visibility: u64,
    order: u64,
    users: u64,
    next: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Node {
    scope: u64,
    symbol: u64,
    section: u64,
    offset: u64,
    length: u64,
    previous_offset: u64,
    previous_length: u64,
    force_near: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RelocationRecord {
    section: u64,
    offset: u64,
    symbol: u64,
    kind: u64,
    addend: u64,
    node: u64,
}

#[repr(C)]
struct Input {
    names: *const Name,
    name_count: u64,
    items: *const Item,
    item_count: u64,
    operands: *const OperandRecord,
    operand_count: u64,
    values: *const DirectiveRecord,
    value_count: u64,
    sections: *mut Section,
    section_capacity: u64,
    symbols: *mut Symbol,
    symbol_capacity: u64,
    table: *mut u64,
    table_capacity: u64,
    nodes: *mut Node,
    queue: *mut u64,
    relocations: *mut RelocationRecord,
    relocation_capacity: u64,
    bytes: *mut u8,
    byte_capacity: u64,
    max_output: u64,
    max_iterations: u64,
    max_work: u64,
}

#[repr(C)]
#[derive(Default)]
struct Output {
    status: u64,
    error: u64,
    node: u64,
    previous: u64,
    argument: u64,
    section_count: u64,
    symbol_count: u64,
    output_symbols: u64,
    relocation_count: u64,
    bytes: u64,
    iterations: u64,
    work: u64,
}

#[repr(C)]
#[derive(Default)]
struct Address {
    mode: u64,
    rm: u64,
    sib: u64,
    displacement: u64,
    width: u64,
    rex_b: u64,
    rex_x: u64,
}

unsafe extern "C" {
    fn whale_amd64_encode(input: *const Input, emission: u64, output: *mut Output) -> i32;

    fn whale_amd64_bits(op: u64, a: u64, b: u64, c: u64, d: u64) -> u64;

    fn whale_amd64_address(
        names: *const Name,
        count: u64,
        operand: *const OperandRecord,
        mode: u64,
        address: *mut Address,
        output: *mut Output,
    ) -> i32;
}

fn backend() -> AsmError {
    AsmError::Backend("AMD64 encoding transport")
}

fn allocate<T: Clone + Default>(count: usize) -> Result<Vec<T>, AsmError> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| allocation())?;
    values.resize(count, T::default());
    Ok(values)
}

fn expr_names(expression: &ExprValue) -> usize {
    usize::from(matches!(expression, ExprValue::Symbol { .. }))
}

fn admission(ast: &AST) -> Result<(usize, usize, usize, usize), AsmError> {
    let mut names = 0usize;
    let mut operands = 0usize;
    let mut values = 0usize;
    for node in &ast.items {
        names = names.checked_add(1).ok_or_else(backend)?;
        match node {
            ASTNode::Instruction(instruction) => {
                operands = operands.checked_add(instruction.operands.len()).ok_or_else(backend)?;
                for operand in &instruction.operands {
                    let count = match operand {
                        Operand::Register(_) | Operand::Label(_) | Operand::SymbolExpr { .. } => 1,
                        Operand::Memory(m) => {
                            usize::from(m.base.is_some())
                                + usize::from(m.index.is_some())
                                + usize::from(m.symbol.is_some())
                        },
                        Operand::Immediate(_) => 0,
                    };
                    names = names.checked_add(count).ok_or_else(backend)?;
                }
            },
            ASTNode::Directive(directive) => {
                values = values.checked_add(directive.values.len()).ok_or_else(backend)?;
                for value in &directive.values {
                    if let DirectiveValue::Expr(expression) = value {
                        names = names.checked_add(expr_names(expression)).ok_or_else(backend)?;
                    }
                }
            },
            ASTNode::Const { expr, .. } | ASTNode::SymbolMetadata { size: expr, .. } => {
                names = names.checked_add(expr_names(expr)).ok_or_else(backend)?;
            },
            _ => {},
        }
    }
    let symbols = names
        .checked_add(ast.items.len())
        .and_then(|n| n.checked_add(operands))
        .and_then(|n| n.checked_add(values))
        .and_then(|n| n.checked_add(1))
        .ok_or_else(backend)?;
    let table =
        symbols.checked_mul(2).and_then(usize::checked_next_power_of_two).ok_or_else(backend)?;
    let sections = ast.items.len().checked_add(1).ok_or_else(backend)?;
    let relocs = ast.items.len().checked_add(values).ok_or_else(backend)?;
    let bytes = ast
        .items
        .len()
        .checked_mul(160 + 64)
        .and_then(|n| n.checked_add(names.checked_mul(32)?))
        .and_then(|n| n.checked_add(operands.checked_mul(176)?))
        .and_then(|n| n.checked_add(values.checked_mul(112)?))
        .and_then(|n| n.checked_add(symbols.checked_mul(184 + 8)?))
        .and_then(|n| n.checked_add(table.checked_mul(8)?))
        .and_then(|n| n.checked_add(sections.checked_mul(48)?))
        .and_then(|n| n.checked_add(relocs.checked_mul(48)?))
        .ok_or_else(backend)?;
    if bytes > 128 * 1024 * 1024 {
        return Err(AsmError::EncodeError("Assembly workspace limit exceeded".into()));
    }
    Ok((symbols, table, relocs, bytes))
}

fn name<'a>(packet: &'a Packet<'_>, index: u64) -> Result<&'a str, AsmError> {
    let descriptor =
        packet.names.get(usize::try_from(index).map_err(|_| backend())?).ok_or_else(backend)?;
    let bytes = unsafe { std::slice::from_raw_parts(descriptor.bytes, descriptor.count as usize) };
    std::str::from_utf8(bytes).map_err(|_| backend())
}

fn symbol_name(packet: &Packet<'_>, symbols: &[Symbol], index: u64) -> Result<String, AsmError> {
    let symbol = symbols.get(usize::try_from(index).map_err(|_| backend())?).ok_or_else(backend)?;
    let suffix = name(packet, symbol.name)?;
    let prefix = if symbol.prefix == MISSING { "" } else { name(packet, symbol.prefix)? };
    let mut value = String::new();
    let length = prefix
        .len()
        .checked_add(suffix.len())
        .filter(|length| *length <= 128 * 1024 * 1024)
        .ok_or_else(workspace_error)?;
    value.try_reserve_exact(length).map_err(|_| allocation())?;
    value.push_str(prefix);
    value.push_str(suffix);
    Ok(value)
}

fn error(
    output: &Output,
    ast: &AST,
    packet: &Packet<'_>,
    symbols: &[Symbol],
    limits: &AssemblyLimits,
) -> AsmError {
    if ast.source_name.len() > 128 * 1024 * 1024 {
        return workspace_error();
    }
    if let Some(ASTNode::Instruction(instruction)) = ast.items.get(output.node as usize) {
        if instruction
            .mnemonic
            .len()
            .checked_add(ast.source_name.len())
            .is_none_or(|length| length > 128 * 1024 * 1024 - 256)
        {
            return workspace_error();
        }
    }
    let limit = limits.max_output_bytes;
    let message = match output.error {
        1 => "Assembly work limit exceeded".into(),
        2 => format!("Assembly output limit exceeded (limit {limit} bytes)"),
        3 => return backend(),
        4 => "AH/CH/DH/BH cannot be encoded with a REX prefix".into(),
        5 => "Invalid register or expected register operand".into(),
        6 => "Register width mismatch".into(),
        7 => "Relative relocation addend overflow".into(),
        8 => "Memory width conflicts with register-derived width".into(),
        9 => "symbol memory should be encoded via rip-relative path".into(),
        10 => "symbol + register memory form is not implemented yet".into(),
        11 => "rel/abs requires a base-less long-mode address".into(),
        12 => "rel/abs displacement is out of signed 32-bit range".into(),
        13 => "Invalid base register".into(),
        14 => "Invalid index register".into(),
        15 => "Addressing without base/index not implemented".into(),
        16 => "Scale must be 1/2/4/8".into(),
        17 => "rsp/esp cannot be used as index register".into(),
        18 => "memory displacement is out of signed 32-bit range".into(),
        19 => "Ambiguous memory immediate; specify byte/word/dword/qword".into(),
        20 => {
            "Memory immediate out of range for selected width (qword requires signed imm32)".into()
        },
        21 => format!("imm{} out of range (r64 arithmetic requires signed imm32)", output.argument),
        22 => "Symbol move is supported only for r32/r64".into(),
        23 => "Symbol addend overflow".into(),
        24 => "Unsupported mov form".into(),
        25 => "symbolic immediate is not supported; use equ or mov reg, symbol".into(),
        26 => "Unsupported instruction form".into(),
        27 => "Register width does not match instruction".into(),
        28 => "Expected register or memory operand".into(),
        29 => "Extension memory source requires byte/word/dword width".into(),
        30 => "Extension source must be register or memory".into(),
        31 => "Invalid extension source/destination width pair".into(),
        32 => "imul form supported: imul reg, reg/mem (not 8-bit operands)".into(),
        33 => "imul requires matching r16/r32/r64 operands".into(),
        34 => "imul third operand must be immediate".into(),
        35 => "imul immediate out of range (imm16/imm32; r64 sign extends imm32)".into(),
        36 => "Shift count must be CL or unsigned imm8".into(),
        37 => "Scalar SSE arithmetic destination must be XMM".into(),
        38 => "Expected XMM0-XMM15 register".into(),
        39 => "Scalar SSE source must be XMM or matching scalar memory".into(),
        40 => "control transfer only supports labels for now".into(),
        41 => "control transfer does not support label addends".into(),
        42 => "branch target is out of 32-bit relative range".into(),
        43 => "Memory displacement overflow".into(),
        44 => format!(
            "Unknown mnemonic {}",
            ast.items
                .get(output.node as usize)
                .and_then(|n| match n {
                    ASTNode::Instruction(i) => Some(i.mnemonic.as_str()),
                    _ => None,
                })
                .unwrap_or("?")
        ),
        45 => {
            let mnemonic = match ast.items.get(output.node as usize) {
                Some(ASTNode::Instruction(i)) => &i.mnemonic,
                _ => return backend(),
            };
            format!(
                "{mnemonic} expects {} operand{}",
                output.argument,
                if output.argument == 1 { "" } else { "s" }
            )
        },
        46 => "push immediate requires signed imm8 or signed imm32 in AMD64 mode".into(),
        47 => "push/pop currently support only r16/r64 in amd64 mode".into(),
        48 => "lea destination must be r32 or r64 and source must be memory address".into(),
        49 => "cmovcc requires matching r16/r32/r64 registers".into(),
        code => {
            let needs_name = matches!(code, 100..=105 | 110..=120 | 123 | 124 | 154 | 163);
            let raw_name = matches!(code, 100 | 119 | 120 | 163);
            let symbol = if needs_name {
                let length = if raw_name {
                    name(packet, output.argument).map(str::len)
                } else {
                    symbol_length(packet, symbols, output.argument)
                };
                let length = match length {
                    Ok(value) => value,
                    Err(_) => return backend(),
                };
                if length
                    .checked_mul(6)
                    .and_then(|n| n.checked_add(ast.source_name.len()))
                    .is_none_or(|n| n > 128 * 1024 * 1024 - 256)
                {
                    return workspace_error();
                }
                let value = if raw_name {
                    name(packet, output.argument).and_then(copy_name)
                } else {
                    symbol_name(packet, symbols, output.argument)
                };
                match value {
                    Ok(value) => value,
                    Err(error) => return error,
                }
            } else {
                String::new()
            };
            let limit = if code == 122 { limits.max_relax_iterations } else { limit };
            return ast.error_at(
                super::namespace_errors::message(code, &symbol, limit),
                output.node as usize,
                usize::try_from(output.previous).ok().filter(|_| output.previous != MISSING),
            );
        },
    };
    ast.error_at(
        AsmError::EncodeError(message),
        output.node as usize,
        usize::try_from(output.previous).ok().filter(|_| output.previous != MISSING),
    )
}

fn allocation() -> AsmError {
    AsmError::EncodeError("Assembly allocation failed".into())
}

fn workspace_error() -> AsmError {
    AsmError::EncodeError("Assembly workspace limit exceeded".into())
}

fn charge_bytes(used: &mut usize, count: usize, width: usize) -> Result<(), AsmError> {
    *used = used
        .checked_add(count.checked_mul(width).ok_or_else(workspace_error)?)
        .filter(|total| *total <= 128 * 1024 * 1024)
        .ok_or_else(workspace_error)?;
    Ok(())
}

fn symbol_length(packet: &Packet<'_>, symbols: &[Symbol], index: u64) -> Result<usize, AsmError> {
    let symbol = symbols.get(usize::try_from(index).map_err(|_| backend())?).ok_or_else(backend)?;
    let suffix = name(packet, symbol.name)?.len();
    let prefix = if symbol.prefix == MISSING { 0 } else { name(packet, symbol.prefix)?.len() };
    prefix.checked_add(suffix).ok_or_else(workspace_error)
}

fn output_admission(
    packet: &Packet<'_>,
    sections: &[Section],
    symbols: &[Symbol],
    relocations: &[RelocationRecord],
    output: &Output,
    workspace: usize,
) -> Result<(), AsmError> {
    let mut used = workspace;
    // The native staging buffer is workspace. The final section payloads have
    // their own caller-selected max_output_bytes ceiling; thus both copies are
    // admitted without silently lowering that public output limit.
    charge_bytes(&mut used, output.bytes as usize, 1)?;
    charge_bytes(&mut used, output.section_count as usize, std::mem::size_of::<AsmSection>())?;
    charge_bytes(
        &mut used,
        output.output_symbols as usize,
        std::mem::size_of::<Option<AsmSymbol>>() + std::mem::size_of::<AsmSymbol>(),
    )?;
    charge_bytes(
        &mut used,
        output.relocation_count as usize,
        2 * std::mem::size_of::<Relocation>(),
    )?;
    let mut first = 0u64;
    for section in sections.iter().take(output.section_count as usize) {
        if section.first != first || !section.align.is_power_of_two() || section.kind > 3 {
            return Err(backend());
        }
        first = first.checked_add(section.size).ok_or_else(backend)?;
        let length = if section.name == MISSING { 5 } else { name(packet, section.name)?.len() };
        charge_bytes(&mut used, length, 1)?;
    }
    if first != output.bytes {
        return Err(backend());
    }
    for (index, symbol) in symbols.iter().take(output.symbol_count as usize).enumerate() {
        if symbol.flags & !447 != 0 {
            return Err(backend());
        }
        if symbol.flags & 6 != 0 {
            charge_bytes(&mut used, symbol_length(packet, symbols, index as u64)?, 1)?;
        }
    }
    for relocation in relocations.iter().take(output.relocation_count as usize) {
        charge_bytes(&mut used, symbol_length(packet, symbols, relocation.symbol)?, 1)?;
    }
    Ok(())
}

fn copy_name(value: &str) -> Result<String, AsmError> {
    let mut output = String::new();
    output.try_reserve_exact(value.len()).map_err(|_| allocation())?;
    output.push_str(value);
    Ok(output)
}

fn valid(output: &Output, input: &Input) -> bool {
    output.status == 0
        && output.section_count <= input.section_capacity
        && output.symbol_count <= input.symbol_capacity
        && output.output_symbols <= output.symbol_count
        && output.relocation_count <= input.relocation_capacity
        && output.bytes <= input.max_output
        && output.work <= input.max_work
        && output.iterations <= input.max_iterations
}

pub(super) fn encode(ast: &AST, limits: &AssemblyLimits) -> Result<AssemblerOutput, AsmError> {
    let (symbol_capacity, table_capacity, relocation_capacity, workspace) = admission(ast)?;
    let packet = Packet::from_ast(ast)?;
    let section_capacity = ast.items.len().checked_add(1).ok_or_else(backend)?;
    let mut sections = allocate::<Section>(section_capacity)?;
    let mut symbols = allocate::<Symbol>(symbol_capacity)?;
    let mut table = allocate::<u64>(table_capacity)?;
    let mut nodes = allocate::<Node>(ast.items.len())?;
    let mut queue = allocate::<u64>(symbol_capacity)?;
    let mut relocations = allocate::<RelocationRecord>(relocation_capacity)?;
    let mut input = Input {
        names: packet.names.as_ptr(),
        name_count: packet.names.len() as u64,
        items: packet.items.as_ptr(),
        item_count: packet.items.len() as u64,
        operands: packet.operands.as_ptr(),
        operand_count: packet.operands.len() as u64,
        values: packet.values.as_ptr(),
        value_count: packet.values.len() as u64,
        sections: sections.as_mut_ptr(),
        section_capacity: section_capacity as u64,
        symbols: symbols.as_mut_ptr(),
        symbol_capacity: symbol_capacity as u64,
        table: table.as_mut_ptr(),
        table_capacity: table_capacity as u64,
        nodes: nodes.as_mut_ptr(),
        queue: queue.as_mut_ptr(),
        relocations: relocations.as_mut_ptr(),
        relocation_capacity: relocation_capacity as u64,
        bytes: std::ptr::null_mut(),
        byte_capacity: 0,
        max_output: limits.max_output_bytes as u64,
        max_iterations: limits.max_relax_iterations as u64,
        max_work: 256 * 1024 * 1024,
    };
    let mut output = Output::default();
    let status = unsafe { whale_amd64_encode(&input, 0, &mut output) };
    if status != 0 {
        if output.status != status as u64 {
            return Err(backend());
        }
        return Err(error(&output, ast, &packet, &symbols, limits));
    }
    if !valid(&output, &input) {
        return Err(backend());
    }
    output_admission(&packet, &sections, &symbols, &relocations, &output, workspace)?;
    let size = usize::try_from(output.bytes).map_err(|_| backend())?;
    let mut bytes = allocate::<u8>(size)?;
    input.bytes = bytes.as_mut_ptr();
    input.byte_capacity = bytes.len() as u64;
    let status = unsafe { whale_amd64_encode(&input, 1, &mut output) };
    if status != 0 {
        if output.status != status as u64 {
            return Err(backend());
        }
        return Err(error(&output, ast, &packet, &symbols, limits));
    }
    if !valid(&output, &input) || output.bytes != bytes.len() as u64 {
        return Err(backend());
    }
    output_admission(&packet, &sections, &symbols, &relocations, &output, workspace)?;
    let mut result = AssemblerOutput { sections: Vec::new(), symbols: Vec::new() };
    result.sections.try_reserve_exact(output.section_count as usize).map_err(|_| allocation())?;
    for section in sections.iter().take(output.section_count as usize) {
        let first = usize::try_from(section.first).map_err(|_| backend())?;
        let count = usize::try_from(section.size).map_err(|_| backend())?;
        let data =
            bytes.get(first..first.checked_add(count).ok_or_else(backend)?).ok_or_else(backend)?;
        let mut owned = Vec::new();
        owned.try_reserve_exact(count).map_err(|_| allocation())?;
        owned.extend_from_slice(data);
        result.sections.push(AsmSection {
            kind: match section.kind {
                0 => AsmSectionKind::Text,
                1 => AsmSectionKind::Data,
                2 => AsmSectionKind::ReadOnlyData,
                3 => AsmSectionKind::Bss,
                _ => return Err(backend()),
            },
            align: section.align,
            name: if section.name == MISSING {
                copy_name(".text")?
            } else {
                copy_name(name(&packet, section.name)?)?
            },
            data: owned,
            zero_fill: section.zero_fill,
            relocs: Vec::new(),
        });
    }
    let mut ordered = {
        let mut ordered = Vec::new();
        ordered.try_reserve_exact(output.output_symbols as usize).map_err(|_| allocation())?;
        ordered.resize_with(output.output_symbols as usize, || None);
        ordered
    };
    for (index, symbol) in symbols.iter().take(output.symbol_count as usize).enumerate() {
        if symbol.flags & 6 == 0 {
            continue;
        }
        let slot = ordered
            .get_mut(usize::try_from(symbol.order).map_err(|_| backend())?)
            .ok_or_else(backend)?;
        if slot.is_some() {
            return Err(backend());
        }
        *slot = Some(AsmSymbol {
            kind: match symbol.symbol_kind {
                0 => SymbolKind::NoType,
                1 => SymbolKind::Function,
                2 => SymbolKind::Object,
                _ => return Err(backend()),
            },
            size: symbol.size,
            visibility: match symbol.visibility {
                0 => SymbolVisibility::Default,
                1 => SymbolVisibility::Hidden,
                _ => return Err(backend()),
            },
            name: symbol_name(&packet, &symbols, index as u64)?,
            section_index: if symbol.section == MISSING {
                None
            } else {
                let section = usize::try_from(symbol.section).map_err(|_| backend())?;
                if section >= result.sections.len() {
                    return Err(backend());
                }
                Some(section)
            },
            offset: symbol.offset,
            is_global: symbol.flags & 1 != 0,
        });
    }
    result.symbols.try_reserve_exact(ordered.len()).map_err(|_| allocation())?;
    for symbol in ordered {
        result.symbols.push(symbol.ok_or_else(backend)?);
    }
    for relocation in relocations.iter().take(output.relocation_count as usize) {
        let section = result
            .sections
            .get_mut(usize::try_from(relocation.section).map_err(|_| backend())?)
            .ok_or_else(backend)?;
        section.relocs.try_reserve(1).map_err(|_| allocation())?;
        section.relocs.push(Relocation {
            offset: usize::try_from(relocation.offset).map_err(|_| backend())?,
            symbol: symbol_name(&packet, &symbols, relocation.symbol)?,
            kind: match relocation.kind {
                0 => RelocKind::Absolute64,
                1 => RelocKind::Absolute32,
                2 => RelocKind::SignedAbsolute32,
                3 => RelocKind::Relative32,
                4 => RelocKind::Branch32,
                5 => RelocKind::Relative8,
                _ => return Err(backend()),
            },
            addend: relocation.addend as i64,
        });
    }
    Ok(result)
}

pub(crate) fn bits(op: u64, a: u64, b: u64, c: u64, d: u64) -> u8 {
    u8::try_from(unsafe { whale_amd64_bits(op, a, b, c, d) }).expect("Wave AMD64 bit transport")
}

pub(crate) fn address(memory: &MemoryOperand, mode: u8) -> Result<EncodedAddress, AsmError> {
    let ast = AST::new(Vec::new());
    let mut packet = Packet::new();
    let mut raw = OperandRecord {
        kind: 4,
        name: MISSING,
        width: memory.width.map_or(MISSING, u64::from),
        base: MISSING,
        index: MISSING,
        symbol: MISSING,
        scale: memory.scale.into(),
        disp: memory.disp as u64,
        mode: match memory.address_mode {
            assembler::ast::MemoryAddressMode::Default => 0,
            assembler::ast::MemoryAddressMode::Relative => 1,
            assembler::ast::MemoryAddressMode::Absolute => 2,
        },
        ..OperandRecord::default()
    };
    for (source, target) in [
        (&memory.base, &mut raw.base),
        (&memory.index, &mut raw.index),
        (&memory.symbol, &mut raw.symbol),
    ] {
        if let Some(value) = source {
            *target = crate::ast_model::push(
                &mut packet.names,
                Name { bytes: value.as_ptr(), count: value.len() as u64 },
            )?;
        }
    }
    let mut output = Output::default();
    let mut address = Address::default();
    let status = unsafe {
        whale_amd64_address(
            packet.names.as_ptr(),
            packet.names.len() as u64,
            &raw,
            mode.into(),
            &mut address,
            &mut output,
        )
    };
    if status != 0 {
        return Err(error(&output, &ast, &packet, &[], &AssemblyLimits::default()));
    }
    if address.mode > 3
        || address.rm > 7
        || address.rex_b > 1
        || address.rex_x > 1
        || address.sib != MISSING && address.sib > 255
    {
        return Err(backend());
    }
    Ok(EncodedAddress {
        mod_bits: address.mode as u8,
        rm_bits: address.rm as u8,
        sib: (address.sib != MISSING).then_some((
            (address.sib >> 6) as u8,
            ((address.sib >> 3) & 7) as u8,
            (address.sib & 7) as u8,
        )),
        disp: match address.width {
            0 => None,
            1 => Some(DispKind::Disp8(address.displacement as i8)),
            4 => Some(DispKind::Disp32(address.displacement as i32)),
            _ => return Err(backend()),
        },
        rex_b: address.rex_b != 0,
        rex_x: address.rex_x != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_output_names_are_admitted_cumulatively_before_copying() {
        // A small borrowed AST can describe over 128 MiB of canonical output
        // names: the parent is stored once, but each local label adds it again.
        let mut items = vec![ASTNode::Label("p".repeat(1024 * 1024))];
        items.extend((0..128).map(|index| ASTNode::Label(format!(".local{index}"))));
        let ast = AST::new(items);
        let packet = Packet::from_ast(&ast).unwrap();
        let mut symbols: Vec<Symbol> = (0..129)
            .map(|index| Symbol {
                prefix: if index == 0 { MISSING } else { 0 },
                name: index,
                flags: 4,
                order: index,
                ..Symbol::default()
            })
            .collect();
        let sections = [Section { name: MISSING, align: 16, ..Section::default() }];
        let output = Output {
            section_count: 1,
            symbol_count: 129,
            output_symbols: 129,
            ..Output::default()
        };
        let (_, _, _, workspace) = admission(&ast).unwrap();
        let error =
            output_admission(&packet, &sections, &symbols, &[], &output, workspace).unwrap_err();
        assert!(
            matches!(error, AsmError::EncodeError(message) if message.contains("workspace limit"))
        );
        // Corrupt native indexes stay backend failures rather than becoming a
        // data error, even when the rest of this fixture also exceeds a budget.
        symbols[0].name = packet.names.len() as u64;
        assert!(matches!(
            output_admission(&packet, &sections, &symbols, &[], &output, workspace),
            Err(AsmError::Backend(_))
        ));
    }

    #[test]
    fn malformed_native_section_ranges_are_rejected_before_payload_allocation() {
        let ast = AST::new(Vec::new());
        let packet = Packet::from_ast(&ast).unwrap();
        let output = Output { section_count: 1, bytes: 4, ..Output::default() };
        for section in [
            Section { name: MISSING, align: 16, first: 1, size: 4, ..Section::default() },
            Section { name: MISSING, align: 3, first: 0, size: 4, ..Section::default() },
            Section { name: MISSING, align: 16, first: 0, size: 5, ..Section::default() },
        ] {
            assert!(matches!(
                output_admission(&packet, &[section], &[], &[], &output, 0),
                Err(AsmError::Backend(_))
            ));
        }
    }
}
