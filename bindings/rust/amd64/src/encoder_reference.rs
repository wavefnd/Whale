use std::collections::{HashMap, HashSet, VecDeque};

use assembler::assembler::{AsmSection, AsmSymbol, AssemblerOutput, RelocKind, Relocation};

use assembler::ast::*;

use assembler::error::AsmError;

use crate::encoding::address::reference::encode_address;

use crate::encoding::{DispKind, EncodedAddress};

use reference_modrm::ModRM;

use reference_rex::Rex;

use crate::tables::*;

#[cfg(whale_wave_amd64)]
use crate::tables::{
    condition_code_reference as condition_code, xmm_register_code_reference as xmm_register_code,
};

type LabelLocations = HashMap<String, (usize, u64)>;

#[derive(PartialEq, Eq)]
struct EncodingLayout {
    labels: LabelLocations,
    instruction_lengths: Vec<usize>,
    instruction_offsets: Vec<usize>,
}

struct EncodingPass {
    output: AssemblerOutput,
    layout: EncodingLayout,
    relocation_sites: HashMap<(usize, usize), usize>,
}

// Resolve the one-symbol constant dependency graph before encoding any use.
// Worklist traversal avoids recursion and repeated whole-source rescans.
fn resolve_constants(ast: &AST) -> Result<HashMap<String, i128>, AsmError> {
    let mut definitions = Vec::new();
    let mut positions = HashMap::new();
    let mut reserved = HashSet::new();
    let mut scope = None;
    let mut constant_sites = HashMap::<String, usize>::new();
    let mut reserved_sites = HashMap::<String, usize>::new();
    for (node_index, node) in ast.items.iter().enumerate() {
        match node {
            ASTNode::Label(name) => {
                let resolved = resolve_symbol_name(name, &scope)
                    .map_err(|error| ast.error_at(error, node_index, None))?;
                reserved_sites.entry(resolved.clone()).or_insert(node_index);
                reserved.insert(resolved.clone());
                if !name.starts_with('.') {
                    scope = Some(resolved);
                }
            },
            ASTNode::Extern(name) => {
                let resolved = resolve_symbol_name(name, &scope)
                    .map_err(|error| ast.error_at(error, node_index, None))?;
                reserved_sites.entry(resolved.clone()).or_insert(node_index);
                reserved.insert(resolved);
            },
            ASTNode::Const { name, expr } => {
                let name = resolve_symbol_name(name, &scope)
                    .map_err(|error| ast.error_at(error, node_index, None))?;
                if positions.insert(name.clone(), definitions.len()).is_some() {
                    return Err(ast.error_at(
                        AsmError::SymbolError(format!("Duplicate constant definition '{name}'")),
                        node_index,
                        constant_sites.get(&name).copied(),
                    ));
                }
                constant_sites.insert(name.clone(), node_index);
                definitions.push((
                    name,
                    resolve_expr_symbols(expr, &scope)
                        .map_err(|error| ast.error_at(error, node_index, None))?,
                ));
            },
            _ => {},
        }
    }
    let mut dependents = vec![Vec::new(); definitions.len()];
    let mut ready = VecDeque::new();
    for (position, (name, expr)) in definitions.iter().enumerate() {
        if reserved.contains(name) {
            let declaration = constant_sites[name];
            let previous = reserved_sites[name];
            return Err(ast.error_at(
                AsmError::SymbolError(format!(
                    "Constant '{name}' conflicts with a label or extern symbol"
                )),
                declaration.max(previous),
                Some(declaration.min(previous)),
            ));
        }
        match expr {
            ExprValue::Number(_) => ready.push_back(position),
            ExprValue::Symbol { name: dependency, .. } => {
                let dependency = positions.get(dependency).ok_or_else(|| {
                    ast.error_at(
                        AsmError::SymbolError(format!(
                            "equ requires a constant expression; unresolved symbol '{dependency}'"
                        )),
                        constant_sites[name],
                        None,
                    )
                })?;
                dependents[*dependency].push(position);
            },
        }
    }
    let mut values = HashMap::new();
    while let Some(position) = ready.pop_front() {
        let (name, expr) = &definitions[position];
        let EvaluatedExpr::Number(value) = eval_expr(expr, &values)
            .map_err(|error| ast.error_at(error, constant_sites[name], None))?
        else {
            return Err(ast.error_at(
                AsmError::SymbolError(format!("Unresolved constant dependency for '{name}'")),
                constant_sites[name],
                None,
            ));
        };
        values.insert(name.clone(), value);
        ready.extend(dependents[position].iter().copied());
    }
    if let Some((name, _)) = definitions.iter().find(|(name, _)| !values.contains_key(name)) {
        return Err(ast.error_at(
            AsmError::SymbolError(format!("Constant dependency cycle prevents resolving '{name}'")),
            constant_sites[name],
            None,
        ));
    }
    Ok(values)
}

pub fn encode_with_limits(
    ast: &AST,
    limits: &assembler::AssemblyLimits,
) -> Result<AssemblerOutput, AsmError> {
    let consts = resolve_constants(ast)?;
    let mut prev_label_locs = None;
    let mut forced_near = HashSet::new();
    for _ in 0..limits.max_relax_iterations {
        let mut pass =
            encode_once(ast, prev_label_locs.as_ref(), &consts, limits, &mut forced_near)?;
        if prev_label_locs.as_ref() == Some(&pass.layout) {
            resolve_loops(ast, &mut pass)?;
            return Ok(pass.output);
        }
        prev_label_locs = Some(pass.layout);
    }
    Err(AsmError::EncodeError(format!(
        "Branch relaxation did not converge within {} passes",
        limits.max_relax_iterations
    )))
}

fn encode_once(
    ast: &AST,
    jump_hint_layout: Option<&EncodingLayout>,
    consts: &HashMap<String, i128>,
    limits: &assembler::AssemblyLimits,
    forced_near: &mut HashSet<usize>,
) -> Result<EncodingPass, AsmError> {
    let mut materialized = 0usize;
    let mut sections = Vec::new();
    let mut symbols = Vec::new();

    let mut defined_labels = HashSet::<String>::new();
    let mut label_locs = HashMap::<String, (usize, u64)>::new();
    let mut jump_known_locs =
        jump_hint_layout.map(|layout| layout.labels.clone()).unwrap_or_default();
    let mut instruction_lengths = vec![0; ast.items.len()];
    let mut instruction_offsets = vec![0; ast.items.len()];
    let mut extern_symbols = HashSet::<String>::new();
    let mut global_symbols = HashSet::<String>::new();
    let mut label_sites = HashMap::<String, usize>::new();
    let mut extern_sites = HashMap::<String, usize>::new();
    let mut global_sites = HashMap::<String, usize>::new();
    let mut relocation_sites = HashMap::new();
    let mut symbol_metadata = HashMap::new();
    let mut metadata_sites = HashMap::<String, usize>::new();

    sections.push(AsmSection {
        kind: AsmSectionKind::Text,
        align: 16,
        name: ".text".to_string(),
        data: Vec::new(),
        zero_fill: 0,
        relocs: Vec::new(),
    });

    let mut current_section_idx = 0usize;
    let mut current_nonlocal_label: Option<String> = None;

    for (node_index, node) in ast.items.iter().enumerate() {
        let original_section = current_section_idx;
        let relocation_start = sections[original_section].relocs.len();
        (|| -> Result<(), AsmError> {
            match node {
                ASTNode::Section(name) | ASTNode::SectionAttributes { name, .. } => {
                    let explicit = match node {
                        ASTNode::SectionAttributes { kind, .. } => Some(*kind),
                        _ => None,
                    };
                    if let Some(idx) = sections.iter().position(|s| s.name == *name) {
                        if explicit.is_some_and(|kind| kind != sections[idx].kind) {
                            return Err(AsmError::EncodeError(format!(
                                "Incompatible section redeclaration '{name}'"
                            )));
                        }
                        current_section_idx = idx;
                    } else {
                        let default = AsmSectionKind::default_for_name(name);
                        let kind = explicit.unwrap_or(default);
                        if matches!(name.as_str(), ".text" | ".data" | ".rodata" | ".bss")
                            && kind != default
                        {
                            return Err(AsmError::EncodeError(format!(
                                "Standard section '{name}' has incompatible kind"
                            )));
                        }
                        sections.push(AsmSection {
                            name: name.clone(),
                            kind,
                            align: kind.default_alignment(),
                            data: Vec::new(),
                            zero_fill: 0,
                            relocs: Vec::new(),
                        });
                        current_section_idx = sections.len() - 1;
                    }
                },
                ASTNode::Global(name) => {
                    let resolved = resolve_symbol_name(name, &current_nonlocal_label)?;
                    global_sites.entry(resolved.clone()).or_insert(node_index);
                    global_symbols.insert(resolved);
                },
                ASTNode::Extern(name) => {
                    let resolved = resolve_symbol_name(name, &current_nonlocal_label)?;
                    if defined_labels.contains(&resolved) {
                        return Err(ast.error_at(
                            AsmError::SymbolError(format!(
                                "Extern symbol '{}' was already defined in this file",
                                resolved
                            )),
                            node_index,
                            label_sites.get(&resolved).copied(),
                        ));
                    }
                    extern_sites.entry(resolved.clone()).or_insert(node_index);
                    if extern_symbols.insert(resolved.clone()) {
                        symbols.push(AsmSymbol {
                            kind: assembler::SymbolKind::NoType,
                            size: 0,
                            visibility: assembler::SymbolVisibility::Default,
                            name: resolved,
                            section_index: None,
                            offset: 0,
                            is_global: true,
                        });
                    }
                },
                ASTNode::SymbolMetadata { name, kind, size, visibility } => {
                    let name = resolve_symbol_name(name, &current_nonlocal_label)?;
                    let size = resolve_expr_symbols(size, &current_nonlocal_label)?;
                    let EvaluatedExpr::Number(size) = eval_expr(&size, consts)? else {
                        return Err(AsmError::SymbolError(format!(
                            "symbol '{name}' size must be a constant"
                        )));
                    };
                    let size = u64::try_from(size).map_err(|_| {
                        AsmError::SymbolError(format!(
                            "symbol '{name}' size must be a nonnegative u64"
                        ))
                    })?;
                    if symbol_metadata.insert(name.clone(), (*kind, size, *visibility)).is_some() {
                        return Err(ast.error_at(
                            AsmError::SymbolError(format!(
                                "Duplicate symbol metadata for '{name}'"
                            )),
                            node_index,
                            metadata_sites.get(&name).copied(),
                        ));
                    }
                    metadata_sites.insert(name, node_index);
                },
                ASTNode::Const { .. } => {},
                ASTNode::Label(name) => {
                    let resolved = resolve_symbol_name(name, &current_nonlocal_label)?;
                    if !name.starts_with('.') {
                        current_nonlocal_label = Some(resolved.clone());
                    }
                    if extern_symbols.contains(&resolved) {
                        return Err(ast.error_at(
                            AsmError::SymbolError(format!(
                                "Label '{}' conflicts with extern declaration",
                                resolved
                            )),
                            node_index,
                            extern_sites.get(&resolved).copied(),
                        ));
                    }
                    if consts.contains_key(&resolved) {
                        return Err(AsmError::SymbolError(format!(
                            "Label '{}' conflicts with constant definition",
                            resolved
                        )));
                    }
                    if !defined_labels.insert(resolved.clone()) {
                        return Err(ast.error_at(
                            AsmError::SymbolError(format!(
                                "Duplicate label definition '{}'",
                                resolved
                            )),
                            node_index,
                            label_sites.get(&resolved).copied(),
                        ));
                    }
                    label_sites.insert(resolved.clone(), node_index);
                    let section = &sections[current_section_idx];
                    let offset = (section.data.len() as u64)
                        .checked_add(section.zero_fill)
                        .ok_or_else(|| AsmError::EncodeError("section size overflow".into()))?;
                    label_locs.insert(resolved.clone(), (current_section_idx, offset));
                    jump_known_locs.insert(resolved.clone(), (current_section_idx, offset));
                    symbols.push(AsmSymbol {
                        kind: assembler::SymbolKind::NoType,
                        size: 0,
                        visibility: assembler::SymbolVisibility::Default,
                        name: resolved.clone(),
                        section_index: Some(current_section_idx),
                        offset,
                        is_global: global_symbols.contains(&resolved),
                    });
                },
                ASTNode::Instruction(ins) => {
                    let scoped = resolve_instruction_symbols(ins, &current_nonlocal_label)?;
                    let inst = resolve_instruction_consts(&scoped, consts)?;
                    let sec = &mut sections[current_section_idx];
                    if sec.kind == AsmSectionKind::Bss {
                        return Err(AsmError::EncodeError(
                            "instructions are not valid in BSS".into(),
                        ));
                    }
                    let cur_off = sec.data.len();
                    let mut instruction_bytes = Vec::new();
                    let mut instruction_relocs = Vec::new();
                    encode_instruction(
                        &inst,
                        &mut instruction_bytes,
                        &mut instruction_relocs,
                        current_section_idx,
                        jump_hint_layout
                            .map(|layout| layout.instruction_offsets[node_index])
                            .unwrap_or(cur_off),
                        jump_hint_layout.map(|layout| &layout.labels).unwrap_or(&jump_known_locs),
                        BranchRelaxation {
                            previous_len: jump_hint_layout
                                .map(|layout| layout.instruction_lengths[node_index])
                                .unwrap_or(0),
                            force_near: forced_near.contains(&node_index),
                        },
                    )?;
                    // Alignment padding can offset an earlier shrink. Once a short
                    // branch must grow, keep it near: each branch changes size at most
                    // twice, so relaxation cannot oscillate around alignment edges.
                    if jump_hint_layout
                        .is_some_and(|layout| layout.instruction_lengths[node_index] == 2)
                        && instruction_bytes.len() > 2
                        && (inst.mnemonic == "jmp" || condition_code(&inst.mnemonic).is_some())
                    {
                        forced_near.insert(node_index);
                    }
                    instruction_lengths[node_index] = instruction_bytes.len();
                    instruction_offsets[node_index] = cur_off;
                    reserve_output(
                        &mut sec.data,
                        instruction_bytes.len(),
                        &mut materialized,
                        limits,
                    )?;
                    sec.data.extend_from_slice(&instruction_bytes);
                    for reloc in &mut instruction_relocs {
                        reloc.offset += cur_off;
                    }
                    sec.relocs.extend(instruction_relocs);
                },
                ASTNode::Directive(dir) => {
                    let scoped = resolve_directive_symbols(dir, &current_nonlocal_label)?;
                    let sec = &mut sections[current_section_idx];
                    if scoped.name == "align" {
                        let alignment = alignment_value(&scoped, consts)?;
                        sec.align = sec.align.max(alignment);
                        let offset = (sec.data.len() as u64)
                            .checked_add(sec.zero_fill)
                            .ok_or_else(|| AsmError::EncodeError("Section size overflow".into()))?;
                        let padding = offset.wrapping_neg() & (alignment - 1);
                        offset.checked_add(padding).ok_or_else(|| {
                            AsmError::EncodeError("Alignment size overflow".into())
                        })?;
                        if sec.kind == AsmSectionKind::Bss {
                            sec.zero_fill =
                                sec.zero_fill.checked_add(padding).ok_or_else(|| {
                                    AsmError::EncodeError("BSS alignment overflow".into())
                                })?;
                        } else {
                            let extra = usize::try_from(padding).map_err(|_| {
                                AsmError::EncodeError("Alignment exceeds host capacity".into())
                            })?;
                            reserve_output(&mut sec.data, extra, &mut materialized, limits)?;
                            let fill = if sec.kind == AsmSectionKind::Text { 0x90 } else { 0 };
                            sec.data.resize(sec.data.len() + extra, fill);
                        }
                    } else if sec.kind == AsmSectionKind::Bss {
                        encode_bss_directive(&scoped, sec, consts)?;
                    } else {
                        let extra = directive_size(&scoped, consts)?;
                        reserve_output(&mut sec.data, extra, &mut materialized, limits)?;
                        encode_directive(&scoped, &mut sec.data, &mut sec.relocs, consts)?;
                    }
                },
            }
            Ok(())
        })()
        .map_err(|error| ast.error_at(error, node_index, None))?;
        for relocation in relocation_start..sections[original_section].relocs.len() {
            relocation_sites.insert((original_section, relocation), node_index);
        }
    }

    // `global` plus `extern` intentionally exports an undefined external;
    // an ordinary global must have a label definition in this file.
    if let Some(name) = global_symbols
        .iter()
        .filter(|name| !defined_labels.contains(*name) && !extern_symbols.contains(*name))
        .min()
    {
        return Err(ast.error_at(
            AsmError::SymbolError(format!(
                "Global symbol '{name}' has no definition or extern declaration"
            )),
            global_sites[name],
            None,
        ));
    }

    let symbol_names: HashSet<_> = symbols.iter().map(|s| &s.name).collect();
    for name in symbol_metadata.keys() {
        if !symbol_names.contains(name) {
            return Err(ast.error_at(
                AsmError::SymbolError(format!(
                    "Metadata symbol '{name}' has no label or extern declaration"
                )),
                metadata_sites[name],
                None,
            ));
        }
    }
    for sym in &mut symbols {
        if let Some((kind, size, visibility)) = symbol_metadata.get(&sym.name) {
            sym.kind = *kind;
            sym.size = *size;
            sym.visibility = *visibility;
            if let Some(index) = sym.section_index {
                let section = &sections[index];
                let extent = (section.data.len() as u64)
                    .checked_add(section.zero_fill)
                    .ok_or_else(|| AsmError::SymbolError("symbol section size overflow".into()))?;
                if sym.offset.checked_add(*size).is_none_or(|end| end > extent) {
                    return Err(ast.error_at(
                        AsmError::SymbolError(format!(
                            "Symbol '{}' size exceeds its section",
                            sym.name
                        )),
                        metadata_sites[&sym.name],
                        None,
                    ));
                }
            }
        }
        if global_symbols.contains(&sym.name) {
            sym.is_global = true;
        }
    }

    for (section_index, sec) in sections.iter().enumerate() {
        for (relocation_index, reloc) in sec.relocs.iter().enumerate() {
            if !defined_labels.contains(&reloc.symbol) && !extern_symbols.contains(&reloc.symbol) {
                return Err(ast.error_at(AsmError::SymbolError(format!("Undefined symbol '{}' (define it, declare with extern, or define with equ)",reloc.symbol)),relocation_sites[&(section_index,relocation_index)],None));
            }
        }
    }

    Ok(EncodingPass {
        output: AssemblerOutput { sections, symbols },
        layout: EncodingLayout { labels: label_locs, instruction_lengths, instruction_offsets },
        relocation_sites,
    })
}

// LOOP has a fixed length, so its displacement cannot affect relaxation. Resolve
// local PC8 fields only after the whole layout stabilizes; an intermediate near
// JMP/Jcc or alignment change must not trigger a premature range diagnostic.
fn resolve_loops(ast: &AST, pass: &mut EncodingPass) -> Result<(), AsmError> {
    for (section_index, section) in pass.output.sections.iter_mut().enumerate() {
        let mut retained = 0;
        for index in 0..section.relocs.len() {
            let relocation = &section.relocs[index];
            let target = pass.layout.labels.get(&relocation.symbol);
            if let Some((_, target_offset)) = target.filter(|(target_section, _)| {
                matches!(relocation.kind, RelocKind::Relative8) && *target_section == section_index
            }) {
                let displacement = i128::from(*target_offset) - relocation.offset as i128
                    + i128::from(relocation.addend);
                let displacement = i8::try_from(displacement).map_err(|_| {
                    ast.error_at(
                        AsmError::EncodeError(format!(
                            "loop target '{}' is out of 8-bit range",
                            relocation.symbol
                        )),
                        pass.relocation_sites[&(section_index, index)],
                        None,
                    )
                })?;
                section.data[relocation.offset] = displacement as u8;
            } else {
                section.relocs.swap(retained, index);
                retained += 1;
            }
        }
        section.relocs.truncate(retained);
    }
    Ok(())
}

#[derive(Clone)]
struct RegInfo {
    code: u8,
    width: u8,
    high8: bool,
    rex8: bool,
}

#[derive(Clone)]
enum EvaluatedExpr {
    Number(i128),
    Symbol { name: String, addend: i64 },
}

fn resolve_symbol_name(
    raw: &str,
    current_nonlocal_label: &Option<String>,
) -> Result<String, AsmError> {
    if raw.starts_with('.') {
        if let Some(base) = current_nonlocal_label {
            return Ok(format!("{}{}", base, raw));
        }
        return Err(AsmError::SymbolError(format!(
            "Local label '{}' has no parent non-local label in scope",
            raw
        )));
    }
    Ok(raw.to_string())
}

fn resolve_expr_symbols(
    expr: &ExprValue,
    current_nonlocal_label: &Option<String>,
) -> Result<ExprValue, AsmError> {
    match expr {
        ExprValue::Number(n) => Ok(ExprValue::Number(*n)),
        ExprValue::Symbol { name, addend } => Ok(ExprValue::Symbol {
            name: resolve_symbol_name(name, current_nonlocal_label)?,
            addend: *addend,
        }),
    }
}

fn eval_expr(expr: &ExprValue, consts: &HashMap<String, i128>) -> Result<EvaluatedExpr, AsmError> {
    Ok(match expr {
        ExprValue::Number(n) => EvaluatedExpr::Number(*n),
        ExprValue::Symbol { name, addend } => {
            if let Some(c) = consts.get(name) {
                EvaluatedExpr::Number(checked_expression_add(*c, *addend, name)?)
            } else {
                EvaluatedExpr::Symbol {
                    name: name.clone(),
                    addend: i64::try_from(*addend)
                        .map_err(|_| AsmError::EncodeError("Symbol addend overflow".into()))?,
                }
            }
        },
    })
}

fn checked_expression_add(a: i128, b: i128, symbol: &str) -> Result<i128, AsmError> {
    a.checked_add(b).filter(|n| (i64::MIN as i128..=u64::MAX as i128).contains(n)).ok_or_else(
        || AsmError::EncodeError(format!("Signed expression overflow while resolving {symbol:?}")),
    )
}

fn relative_addend(displacement: i64) -> Result<i64, AsmError> {
    displacement
        .checked_sub(4)
        .ok_or_else(|| AsmError::EncodeError("Relative relocation addend overflow".into()))
}

fn resolve_instruction_symbols(
    ins: &Instruction,
    current_nonlocal_label: &Option<String>,
) -> Result<Instruction, AsmError> {
    let mut resolved_ops = Vec::with_capacity(ins.operands.len());
    for op in &ins.operands {
        match op {
            Operand::Label(sym) => {
                resolved_ops.push(Operand::Label(resolve_symbol_name(sym, current_nonlocal_label)?))
            },
            Operand::SymbolExpr { name, addend } => {
                resolved_ops.push(Operand::SymbolExpr {
                    name: resolve_symbol_name(name, current_nonlocal_label)?,
                    addend: *addend,
                });
            },
            Operand::Memory(mem) => {
                let mut mem = mem.clone();
                if let Some(sym) = &mem.symbol {
                    mem.symbol = Some(resolve_symbol_name(sym, current_nonlocal_label)?);
                }
                resolved_ops.push(Operand::Memory(mem));
            },
            _ => resolved_ops.push(op.clone()),
        }
    }
    Ok(Instruction { mnemonic: ins.mnemonic.clone(), operands: resolved_ops })
}

fn resolve_instruction_consts(
    ins: &Instruction,
    consts: &HashMap<String, i128>,
) -> Result<Instruction, AsmError> {
    let mut resolved_ops = Vec::with_capacity(ins.operands.len());
    for op in &ins.operands {
        match op {
            Operand::Label(name) => {
                if let Some(v) = consts.get(name) {
                    resolved_ops.push(Operand::Immediate(*v));
                } else {
                    resolved_ops.push(op.clone());
                }
            },
            Operand::SymbolExpr { name, addend } => {
                if let Some(v) = consts.get(name) {
                    resolved_ops
                        .push(Operand::Immediate(checked_expression_add(*v, *addend, name)?));
                } else {
                    resolved_ops.push(op.clone());
                }
            },
            Operand::Memory(mem) => {
                let mut mem = mem.clone();
                if let Some(sym) = &mem.symbol {
                    if let Some(v) = consts.get(sym) {
                        mem.disp =
                            i64::try_from(checked_expression_add(*v, mem.disp as i128, sym)?)
                                .map_err(|_| {
                                    AsmError::EncodeError("Memory displacement overflow".into())
                                })?;
                        mem.symbol = None;
                    }
                }
                resolved_ops.push(Operand::Memory(mem));
            },
            _ => resolved_ops.push(op.clone()),
        }
    }
    Ok(Instruction { mnemonic: ins.mnemonic.clone(), operands: resolved_ops })
}

fn resolve_directive_symbols(
    dir: &Directive,
    current_nonlocal_label: &Option<String>,
) -> Result<Directive, AsmError> {
    let mut out_values = Vec::with_capacity(dir.values.len());
    for v in &dir.values {
        match v {
            DirectiveValue::StringLiteral(s) => {
                out_values.push(DirectiveValue::StringLiteral(s.clone()))
            },
            DirectiveValue::Expr(expr) => out_values
                .push(DirectiveValue::Expr(resolve_expr_symbols(expr, current_nonlocal_label)?)),
        }
    }
    Ok(Directive { name: dir.name.clone(), values: out_values })
}

fn lookup_reg(name: &str) -> Option<RegInfo> {
    if let Some((_, code)) = REGISTERS_64.iter().find(|(n, _)| *n == name) {
        return Some(RegInfo { code: *code, width: 64, high8: false, rex8: false });
    }
    if let Some((_, code)) = REGISTERS_32.iter().find(|(n, _)| *n == name) {
        return Some(RegInfo { code: *code, width: 32, high8: false, rex8: false });
    }
    if let Some((_, code)) = REGISTERS_16.iter().find(|(n, _)| *n == name) {
        return Some(RegInfo { code: *code, width: 16, high8: false, rex8: false });
    }
    if let Some((reg, code)) = REGISTERS_8.iter().find(|(n, _)| *n == name) {
        return Some(RegInfo {
            code: *code,
            width: 8,
            high8: matches!(*reg, "ah" | "bh" | "ch" | "dh"),
            rex8: matches!(*reg, "spl" | "bpl" | "sil" | "dil"),
        });
    }
    None
}

fn emit_operand_size_prefix(bytes: &mut Vec<u8>, width: u8) {
    if width == 16 {
        bytes.push(0x66);
    }
}

fn emit_rex(
    bytes: &mut Vec<u8>,
    width: u8,
    rex_r: bool,
    rex_x: bool,
    rex_b: bool,
    any_high8: bool,
    force: bool,
) -> Result<(), AsmError> {
    let mut rex = Rex::new();
    rex.w = width == 64;
    rex.r = rex_r;
    rex.x = rex_x;
    rex.b = rex_b;

    let needed = force || rex.w || rex.r || rex.x || rex.b;
    if any_high8 && needed {
        return Err(AsmError::EncodeError(
            "AH/CH/DH/BH cannot be encoded with a REX prefix".into(),
        ));
    }
    if needed {
        bytes.push(rex.encode());
    }
    Ok(())
}

fn emit_disp(bytes: &mut Vec<u8>, disp: Option<DispKind>) {
    if let Some(d) = disp {
        match d {
            DispKind::Disp8(v) => bytes.push(v as u8),
            DispKind::Disp32(v) => bytes.extend_from_slice(&v.to_le_bytes()),
        }
    }
}

fn emit_addr_tail(bytes: &mut Vec<u8>, addr: &EncodedAddress) {
    if let Some((scale, index, base)) = addr.sib {
        bytes.push((scale << 6) | (index << 3) | base);
    }
    emit_disp(bytes, addr.disp.clone());
}

fn emit_reg_reg(
    bytes: &mut Vec<u8>,
    opcode: u8,
    dst: &RegInfo,
    src: &RegInfo,
) -> Result<(), AsmError> {
    emit_operand_size_prefix(bytes, dst.width);
    emit_rex(
        bytes,
        dst.width,
        src.code >= 8,
        false,
        dst.code >= 8,
        dst.high8 || src.high8,
        dst.rex8 || src.rex8,
    )?;
    bytes.push(opcode);
    bytes.push(ModRM::new(0b11, src.code, dst.code).encode());
    Ok(())
}

fn validate_memory_width(mem: &MemoryOperand, width: u8) -> Result<(), AsmError> {
    if mem.width.is_some_and(|explicit| explicit != width) {
        return Err(AsmError::EncodeError(
            "Memory width conflicts with register-derived width".into(),
        ));
    }
    Ok(())
}

fn memory_address(mem: &MemoryOperand) -> Result<EncodedAddress, AsmError> {
    if mem.symbol.is_none() {
        return encode_address(mem, 64);
    }
    if mem.base.is_some() || mem.index.is_some() {
        return Err(AsmError::EncodeError(
            "symbol + register memory form is not implemented yet".into(),
        ));
    }
    let mut address = mem.clone();
    address.symbol = None;
    address.disp = 0; // The linker applies the checked symbol value plus addend.
    if address.address_mode == MemoryAddressMode::Default {
        address.address_mode = MemoryAddressMode::Relative;
    }
    encode_address(&address, 64)
}

// Prefix selection belongs to each instruction; addressing and relocation
// offsets share one path, including instructions with a trailing immediate.
fn emit_memory_operation(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
    opcodes: &[u8],
    reg: &RegInfo,
    mem: &MemoryOperand,
    rex_width: u8,
    trailing: u8,
) -> Result<(), AsmError> {
    let addr = memory_address(mem)?;
    emit_rex(bytes, rex_width, reg.code >= 8, addr.rex_x, addr.rex_b, reg.high8, reg.rex8)?;
    bytes.extend_from_slice(opcodes);
    bytes.push(ModRM::new(addr.mod_bits, reg.code, addr.rm_bits).encode());
    if let Some(symbol) = &mem.symbol {
        let absolute = mem.address_mode == MemoryAddressMode::Absolute;
        let addend = if absolute {
            mem.disp
        } else {
            relative_addend(mem.disp)?.checked_sub(i64::from(trailing)).ok_or_else(|| {
                AsmError::EncodeError("Relative relocation addend overflow".into())
            })?
        };
        relocs.push(Relocation {
            offset: bytes.len() + usize::from(addr.sib.is_some()),
            symbol: symbol.clone(),
            kind: if absolute { RelocKind::SignedAbsolute32 } else { RelocKind::Relative32 },
            addend,
        });
    }
    emit_addr_tail(bytes, &addr);
    Ok(())
}

fn emit_reg_mem(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
    opcode: u8,
    reg: &RegInfo,
    mem: &MemoryOperand,
) -> Result<(), AsmError> {
    validate_memory_width(mem, reg.width)?;
    emit_operand_size_prefix(bytes, reg.width);
    emit_memory_operation(bytes, relocs, &[opcode], reg, mem, reg.width, 0)
}

fn emit_mem_reg(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
    opcode: u8,
    mem: &MemoryOperand,
    reg: &RegInfo,
) -> Result<(), AsmError> {
    emit_reg_mem(bytes, relocs, opcode, reg, mem)
}

#[derive(Clone, Copy)]
struct BranchRelaxation {
    previous_len: usize,
    force_near: bool,
}

fn encode_instruction(
    ins: &Instruction,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
    current_section: usize,
    current_offset: usize,
    label_locs: &HashMap<String, (usize, u64)>,
    relaxation: BranchRelaxation,
) -> Result<(), AsmError> {
    match ins.mnemonic.as_str() {
        "ret" | "nop" | "syscall" | "int3" | "cwd" | "cdq" | "cqo" if !ins.operands.is_empty() => {
            Err(AsmError::EncodeError(format!("{} expects 0 operands", ins.mnemonic)))
        },
        "movss" | "movsd" | "addss" | "addsd" | "subss" | "subsd" | "mulss" | "mulsd" | "divss"
        | "divsd" => encode_scalar_sse(ins, bytes, relocs),
        "mov" => encode_mov(ins, bytes, relocs),
        "movzx" | "movsx" | "movsxd" => encode_extension(ins, bytes, relocs),
        "adc" => encode_carry(ins, 0x10, 0x11, bytes),
        "sbb" => encode_carry(ins, 0x18, 0x19, bytes),
        mnemonic if prefixed_condition(mnemonic, "set").is_some() => {
            encode_setcc(ins, prefixed_condition(mnemonic, "set").unwrap(), bytes, relocs)
        },
        mnemonic if prefixed_condition(mnemonic, "cmov").is_some() => {
            encode_cmovcc(ins, prefixed_condition(mnemonic, "cmov").unwrap(), bytes)
        },
        "add" => encode_binop(ins, 0x00, 0x01, 0x02, 0x03, 0, bytes, relocs),
        "sub" => encode_binop(ins, 0x28, 0x29, 0x2A, 0x2B, 5, bytes, relocs),
        "and" => encode_binop(ins, 0x20, 0x21, 0x22, 0x23, 4, bytes, relocs),
        "or" => encode_binop(ins, 0x08, 0x09, 0x0A, 0x0B, 1, bytes, relocs),
        "xor" => encode_binop(ins, 0x30, 0x31, 0x32, 0x33, 6, bytes, relocs),
        "cmp" => encode_binop(ins, 0x38, 0x39, 0x3A, 0x3B, 7, bytes, relocs),
        "imul" => encode_imul(ins, bytes, relocs),
        "lea" => encode_lea(ins, bytes, relocs),
        "test" => encode_test(ins, bytes),
        "not" => encode_unary_register(ins, 2, bytes),
        "neg" => encode_unary_register(ins, 3, bytes),
        "div" => encode_unary_register(ins, 6, bytes),
        "idiv" => encode_unary_register(ins, 7, bytes),
        "shl" | "sal" => encode_shift(ins, 4, bytes),
        "shr" => encode_shift(ins, 5, bytes),
        "sar" => encode_shift(ins, 7, bytes),
        "cwd" => {
            bytes.extend_from_slice(&[0x66, 0x99]);
            Ok(())
        },
        "cdq" => {
            bytes.push(0x99);
            Ok(())
        },
        "cqo" => {
            bytes.extend_from_slice(&[0x48, 0x99]);
            Ok(())
        },
        "push" if matches!(ins.operands.first(), Some(Operand::Immediate(_))) => {
            encode_push_immediate(ins, bytes)
        },
        "push" => encode_push_pop(ins, 0x50, bytes),
        "pop" => encode_push_pop(ins, 0x58, bytes),
        "jmp" | "call"
            if matches!(ins.operands.first(), Some(Operand::Register(_) | Operand::Memory(_))) =>
        {
            encode_indirect_branch(ins, bytes, relocs)
        },
        "jmp" => encode_jump(
            ins,
            JumpSpec {
                previous_len: relaxation.previous_len,
                force_near: relaxation.force_near,
                near_opcode: JumpOpcode::One(0xE9),
                short_opcode: Some(0xEB),
                reloc_kind: RelocKind::Branch32,
                reloc_addend: -4,
                near_len: 5,
            },
            bytes,
            relocs,
            current_section,
            current_offset,
            label_locs,
        ),
        "call" => encode_jump(
            ins,
            JumpSpec {
                previous_len: relaxation.previous_len,
                force_near: relaxation.force_near,
                near_opcode: JumpOpcode::One(0xE8),
                short_opcode: None,
                reloc_kind: RelocKind::Branch32,
                reloc_addend: -4,
                near_len: 5,
            },
            bytes,
            relocs,
            current_section,
            current_offset,
            label_locs,
        ),
        condition if condition_code(condition).is_some() => encode_jump(
            ins,
            JumpSpec {
                previous_len: relaxation.previous_len,
                force_near: relaxation.force_near,
                near_opcode: JumpOpcode::Two(0x0F, 0x80 + condition_code(condition).unwrap()),
                short_opcode: Some(0x70 + condition_code(condition).unwrap()),
                reloc_kind: RelocKind::Branch32,
                reloc_addend: -4,
                near_len: 6,
            },
            bytes,
            relocs,
            current_section,
            current_offset,
            label_locs,
        ),
        "loop" => encode_loop(ins, bytes, relocs),
        "ret" => {
            bytes.push(0xC3);
            Ok(())
        },
        "nop" => {
            bytes.push(0x90);
            Ok(())
        },
        "syscall" => {
            bytes.push(0x0F);
            bytes.push(0x05);
            Ok(())
        },
        "int3" => {
            bytes.push(0xCC);
            Ok(())
        },
        _ => Err(AsmError::EncodeError(format!("Unknown mnemonic {}", ins.mnemonic))),
    }
}

fn symbol_operand(op: &Operand) -> Option<(String, i128)> {
    match op {
        Operand::Label(name) => Some((name.clone(), 0)),
        Operand::SymbolExpr { name, addend } => Some((name.clone(), *addend)),
        _ => None,
    }
}

fn xmm_operand(operand: &Operand) -> Result<u8, AsmError> {
    if let Operand::Register(name) = operand {
        if let Some(code) = xmm_register_code(name) {
            return Ok(code);
        }
    }
    Err(AsmError::EncodeError("Expected XMM0-XMM15 register".into()))
}

fn encode_scalar_sse(
    ins: &Instruction,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 2 {
        return Err(AsmError::EncodeError(format!("{} expects 2 operands", ins.mnemonic)));
    }
    let single = ins.mnemonic.ends_with("ss");
    let (prefix, width) = if single { (0xf3, 32) } else { (0xf2, 64) };
    let move_instruction = matches!(ins.mnemonic.as_str(), "movss" | "movsd");
    let opcode = match ins.mnemonic.as_str() {
        "movss" | "movsd" => 0x10,
        "addss" | "addsd" => 0x58,
        "subss" | "subsd" => 0x5c,
        "mulss" | "mulsd" => 0x59,
        "divss" | "divsd" => 0x5e,
        _ => unreachable!(),
    };
    // XMM registers are 128-bit registers. The scalar access width chooses the
    // mandatory prefix, never REX.W or the integer operand-size prefix 66.
    if let Operand::Memory(mem) = &ins.operands[0] {
        if !move_instruction {
            return Err(AsmError::EncodeError(
                "Scalar SSE arithmetic destination must be XMM".into(),
            ));
        }
        let code = xmm_operand(&ins.operands[1])?;
        validate_memory_width(mem, width)?;
        bytes.push(prefix);
        let reg = RegInfo { code, width, high8: false, rex8: false };
        return emit_memory_operation(bytes, relocs, &[0x0f, 0x11], &reg, mem, 0, 0);
    }
    let code = xmm_operand(&ins.operands[0])?;
    match &ins.operands[1] {
        Operand::Register(_) => {
            let source = xmm_operand(&ins.operands[1])?;
            bytes.push(prefix);
            emit_rex(bytes, 0, code >= 8, false, source >= 8, false, false)?;
            bytes.extend_from_slice(&[0x0f, opcode]);
            bytes.push(ModRM::new(3, code, source).encode());
            Ok(())
        },
        Operand::Memory(mem) => {
            validate_memory_width(mem, width)?;
            bytes.push(prefix);
            let reg = RegInfo { code, width, high8: false, rex8: false };
            emit_memory_operation(bytes, relocs, &[0x0f, opcode], &reg, mem, 0, 0)
        },
        _ => Err(AsmError::EncodeError(
            "Scalar SSE source must be XMM or matching scalar memory".into(),
        )),
    }
}

fn encode_mov(
    ins: &Instruction,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 2 {
        return Err(AsmError::EncodeError("mov expects 2 operands".into()));
    }
    let dst = &ins.operands[0];
    let src = &ins.operands[1];

    match (dst, src) {
        (Operand::Register(dst_name), Operand::Immediate(imm)) => {
            let reg =
                lookup_reg(dst_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            match reg.width {
                8 => {
                    if !(-128..=255).contains(imm) {
                        return Err(AsmError::EncodeError("imm8 out of range".into()));
                    }
                    emit_rex(bytes, reg.width, false, false, reg.code >= 8, reg.high8, reg.rex8)?;
                    bytes.push(0xB0 + (reg.code & 7));
                    bytes.push(*imm as u8);
                },
                16 => {
                    if !(-32768..=65535).contains(imm) {
                        return Err(AsmError::EncodeError("imm16 out of range".into()));
                    }
                    emit_operand_size_prefix(bytes, reg.width);
                    emit_rex(bytes, reg.width, false, false, reg.code >= 8, false, false)?;
                    bytes.push(0xB8 + (reg.code & 7));
                    bytes.extend_from_slice(&(*imm as i16).to_le_bytes());
                },
                32 => {
                    if !(*imm >= i32::MIN as i128 && *imm <= u32::MAX as i128) {
                        return Err(AsmError::EncodeError("imm32 out of range".into()));
                    }
                    emit_rex(bytes, reg.width, false, false, reg.code >= 8, false, false)?;
                    bytes.push(0xB8 + (reg.code & 7));
                    bytes.extend_from_slice(&(*imm as u32).to_le_bytes());
                },
                64 => {
                    emit_rex(bytes, reg.width, false, false, reg.code >= 8, false, false)?;
                    bytes.push(0xB8 + (reg.code & 7));
                    if !(i64::MIN as i128..=u64::MAX as i128).contains(imm) {
                        return Err(AsmError::EncodeError("imm64 out of range".into()));
                    }
                    bytes.extend_from_slice(&(*imm as u64).to_le_bytes());
                },
                _ => return Err(AsmError::EncodeError("Unsupported register width".into())),
            }
            Ok(())
        },
        (Operand::Register(dst_name), _) if symbol_operand(src).is_some() => {
            let (label, addend) = symbol_operand(src).unwrap();
            let reg =
                lookup_reg(dst_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            match reg.width {
                64 => {
                    emit_rex(bytes, reg.width, false, false, reg.code >= 8, false, false)?;
                    bytes.push(0xB8 + (reg.code & 7));
                    relocs.push(Relocation {
                        offset: bytes.len(),
                        symbol: label,
                        kind: RelocKind::Absolute64,
                        addend: i64::try_from(addend)
                            .map_err(|_| AsmError::EncodeError("Symbol addend overflow".into()))?,
                    });
                    bytes.extend_from_slice(&0i64.to_le_bytes());
                },
                32 => {
                    emit_rex(bytes, reg.width, false, false, reg.code >= 8, false, false)?;
                    bytes.push(0xB8 + (reg.code & 7));
                    relocs.push(Relocation {
                        offset: bytes.len(),
                        symbol: label,
                        kind: RelocKind::Absolute32,
                        addend: i64::try_from(addend)
                            .map_err(|_| AsmError::EncodeError("Symbol addend overflow".into()))?,
                    });
                    bytes.extend_from_slice(&0u32.to_le_bytes());
                },
                _ => {
                    return Err(AsmError::EncodeError(
                        "Symbol move is supported only for r32/r64".into(),
                    ))
                },
            }
            Ok(())
        },
        (Operand::Register(dst_name), Operand::Register(src_name)) => {
            let dst_reg =
                lookup_reg(dst_name).ok_or(AsmError::EncodeError("Invalid dst register".into()))?;
            let src_reg =
                lookup_reg(src_name).ok_or(AsmError::EncodeError("Invalid src register".into()))?;
            if dst_reg.width != src_reg.width {
                return Err(AsmError::EncodeError("Register width mismatch".into()));
            }
            let opcode = if dst_reg.width == 8 { 0x88 } else { 0x89 };
            emit_reg_reg(bytes, opcode, &dst_reg, &src_reg)
        },
        (Operand::Register(dst_name), Operand::Memory(mem)) => {
            let reg =
                lookup_reg(dst_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            let opcode = if reg.width == 8 { 0x8A } else { 0x8B };
            emit_reg_mem(bytes, relocs, opcode, &reg, mem)
        },
        (Operand::Memory(mem), Operand::Register(src_name)) => {
            let reg =
                lookup_reg(src_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            let opcode = if reg.width == 8 { 0x88 } else { 0x89 };
            emit_mem_reg(bytes, relocs, opcode, mem, &reg)
        },
        (Operand::Memory(mem), Operand::Immediate(imm)) => {
            encode_memory_immediate(mem, *imm, None, bytes, relocs)
        },
        _ => Err(AsmError::EncodeError("Unsupported mov form".into())),
    }
}

// The five opcode fields describe the existing instruction table explicitly.
#[allow(clippy::too_many_arguments)]
fn encode_binop(
    ins: &Instruction,
    opcode_rm_r_8: u8,
    opcode_rm_r: u8,
    opcode_r_rm_8: u8,
    opcode_r_rm: u8,
    imm_op_ext: u8,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 2 {
        return Err(AsmError::EncodeError(format!("{} expects 2 operands", ins.mnemonic)));
    }
    let dst = &ins.operands[0];
    let src = &ins.operands[1];

    match (dst, src) {
        (Operand::Register(dst_name), Operand::Register(src_name)) => {
            let dst_reg =
                lookup_reg(dst_name).ok_or(AsmError::EncodeError("Invalid dst register".into()))?;
            let src_reg =
                lookup_reg(src_name).ok_or(AsmError::EncodeError("Invalid src register".into()))?;
            if dst_reg.width != src_reg.width {
                return Err(AsmError::EncodeError("Register width mismatch".into()));
            }
            let opcode = if dst_reg.width == 8 { opcode_rm_r_8 } else { opcode_rm_r };
            emit_reg_reg(bytes, opcode, &dst_reg, &src_reg)
        },
        (Operand::Register(dst_name), Operand::Immediate(imm)) => {
            let dst_reg =
                lookup_reg(dst_name).ok_or(AsmError::EncodeError("Invalid dst register".into()))?;
            emit_operand_size_prefix(bytes, dst_reg.width);
            emit_rex(
                bytes,
                dst_reg.width,
                false,
                false,
                dst_reg.code >= 8,
                dst_reg.high8,
                dst_reg.rex8,
            )?;

            if dst_reg.width == 8 {
                if !(-128..=255).contains(imm) {
                    return Err(AsmError::EncodeError("imm8 out of range".into()));
                }
                bytes.push(0x80);
                bytes.push(ModRM::new(0b11, imm_op_ext, dst_reg.code).encode());
                bytes.push(*imm as u8);
                return Ok(());
            }

            if (-128..=127).contains(imm) {
                bytes.push(0x83);
                bytes.push(ModRM::new(0b11, imm_op_ext, dst_reg.code).encode());
                bytes.push(*imm as u8);
                return Ok(());
            }

            bytes.push(0x81);
            bytes.push(ModRM::new(0b11, imm_op_ext, dst_reg.code).encode());
            match dst_reg.width {
                16 => {
                    if !(-32768..=65535).contains(imm) {
                        return Err(AsmError::EncodeError("imm16 out of range".into()));
                    }
                    bytes.extend_from_slice(&(*imm as i16).to_le_bytes());
                },
                32 => {
                    if !(*imm >= i32::MIN as i128 && *imm <= u32::MAX as i128) {
                        return Err(AsmError::EncodeError("imm32 out of range".into()));
                    }
                    bytes.extend_from_slice(&(*imm as u32).to_le_bytes());
                },
                64 => {
                    if !(*imm >= i32::MIN as i128 && *imm <= i32::MAX as i128) {
                        return Err(AsmError::EncodeError(
                            "imm32-sign-extended out of range for r64".into(),
                        ));
                    }
                    bytes.extend_from_slice(&(*imm as i32).to_le_bytes());
                },
                _ => return Err(AsmError::EncodeError("Unsupported register width".into())),
            }
            Ok(())
        },
        (Operand::Register(_), _) if symbol_operand(src).is_some() => {
            Err(AsmError::EncodeError(format!(
                "{} with symbolic immediate is not supported; use equ or mov reg, symbol",
                ins.mnemonic
            )))
        },
        (Operand::Register(dst_name), Operand::Memory(mem)) => {
            let reg =
                lookup_reg(dst_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            let opcode = if reg.width == 8 { opcode_r_rm_8 } else { opcode_r_rm };
            emit_reg_mem(bytes, relocs, opcode, &reg, mem)
        },
        (Operand::Memory(mem), Operand::Register(src_name)) => {
            let reg =
                lookup_reg(src_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            let opcode = if reg.width == 8 { opcode_rm_r_8 } else { opcode_rm_r };
            emit_mem_reg(bytes, relocs, opcode, mem, &reg)
        },
        (Operand::Memory(mem), Operand::Immediate(imm)) => {
            encode_memory_immediate(mem, *imm, Some(imm_op_ext), bytes, relocs)
        },
        _ => Err(AsmError::EncodeError(format!("Unsupported {} form", ins.mnemonic))),
    }
}

fn encode_memory_immediate(
    mem: &MemoryOperand,
    imm: i128,
    operation: Option<u8>,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    let width = mem.width.ok_or_else(|| {
        AsmError::EncodeError("Ambiguous memory immediate; specify byte/word/dword/qword".into())
    })?;
    let encoded_width = if width == 64 { 32 } else { width };
    let valid = match width {
        8 => (-128..=255).contains(&imm),
        16 => (-32768..=65535).contains(&imm),
        32 => (i32::MIN as i128..=u32::MAX as i128).contains(&imm),
        64 => (i32::MIN as i128..=i32::MAX as i128).contains(&imm),
        _ => false,
    };
    if !valid {
        return Err(AsmError::EncodeError(
            "Memory immediate out of range for selected width (qword requires signed imm32)".into(),
        ));
    }
    let short = operation.is_some() && width != 8 && (-128..=127).contains(&imm);
    let tail = if short { 1 } else { encoded_width / 8 };
    let opcode = match operation {
        None => {
            if width == 8 {
                0xc6
            } else {
                0xc7
            }
        },
        Some(_) => {
            if width == 8 {
                0x80
            } else if short {
                0x83
            } else {
                0x81
            }
        },
    };
    emit_operand_size_prefix(bytes, width);
    let group = RegInfo { code: operation.unwrap_or(0), width, high8: false, rex8: false };
    emit_memory_operation(bytes, relocs, &[opcode], &group, mem, width, tail)?;
    bytes.extend_from_slice(&(imm as u64).to_le_bytes()[..tail as usize]);
    Ok(())
}

fn encode_imul(
    ins: &Instruction,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    if ins.operands.len() == 3 {
        return encode_imul_immediate(ins, bytes);
    }
    if ins.operands.len() != 2 {
        return Err(AsmError::EncodeError("imul expects 2 or 3 operands".into()));
    }
    let dst = &ins.operands[0];
    let src = &ins.operands[1];

    let Operand::Register(dst_name) = dst else {
        return Err(AsmError::EncodeError("imul form supported: imul reg, reg/mem".into()));
    };
    let dst_reg =
        lookup_reg(dst_name).ok_or(AsmError::EncodeError("Invalid dst register".into()))?;

    if dst_reg.width == 8 {
        return Err(AsmError::EncodeError(
            "imul reg, reg/mem is not supported for 8-bit operands".into(),
        ));
    }

    match src {
        Operand::Register(src_name) => {
            let src_reg =
                lookup_reg(src_name).ok_or(AsmError::EncodeError("Invalid src register".into()))?;
            if src_reg.width != dst_reg.width {
                return Err(AsmError::EncodeError("Register width mismatch".into()));
            }
            emit_operand_size_prefix(bytes, dst_reg.width);
            emit_rex(
                bytes,
                dst_reg.width,
                dst_reg.code >= 8,
                false,
                src_reg.code >= 8,
                false,
                false,
            )?;
            bytes.push(0x0F);
            bytes.push(0xAF);
            bytes.push(ModRM::new(0b11, dst_reg.code, src_reg.code).encode());
            Ok(())
        },
        Operand::Memory(mem) => {
            validate_memory_width(mem, dst_reg.width)?;
            emit_operand_size_prefix(bytes, dst_reg.width);
            emit_memory_operation(bytes, relocs, &[0x0f, 0xaf], &dst_reg, mem, dst_reg.width, 0)?;
            Ok(())
        },
        _ => Err(AsmError::EncodeError("imul form supported: imul reg, reg/mem".into())),
    }
}

fn register_operand(op: &Operand) -> Result<RegInfo, AsmError> {
    match op {
        Operand::Register(name) => {
            lookup_reg(name).ok_or_else(|| AsmError::EncodeError("Invalid register".into()))
        },
        _ => Err(AsmError::EncodeError("Expected register operand".into())),
    }
}

fn encode_lea(
    ins: &Instruction,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 2 {
        return Err(AsmError::EncodeError("lea expects 2 operands".into()));
    }
    let reg = register_operand(&ins.operands[0])?;
    if !matches!(reg.width, 32 | 64) {
        return Err(AsmError::EncodeError("lea destination must be r32 or r64".into()));
    }
    let Operand::Memory(mem) = &ins.operands[1] else {
        return Err(AsmError::EncodeError("lea source must be memory address".into()));
    };
    emit_reg_mem(bytes, relocs, 0x8d, &reg, mem)
}

fn encode_test(ins: &Instruction, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    if ins.operands.len() != 2 {
        return Err(AsmError::EncodeError("test expects 2 operands".into()));
    }
    let dst = register_operand(&ins.operands[0])?;
    let src = register_operand(&ins.operands[1])?;
    if dst.width != src.width {
        return Err(AsmError::EncodeError("Register width mismatch".into()));
    }
    emit_reg_reg(bytes, if dst.width == 8 { 0x84 } else { 0x85 }, &dst, &src)
}

fn encode_unary_register(
    ins: &Instruction,
    extension: u8,
    bytes: &mut Vec<u8>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 1 {
        return Err(AsmError::EncodeError(format!("{} expects 1 operand", ins.mnemonic)));
    }
    let reg = register_operand(&ins.operands[0])?;
    emit_operand_size_prefix(bytes, reg.width);
    emit_rex(bytes, reg.width, false, false, reg.code >= 8, reg.high8, reg.rex8)?;
    bytes.push(if reg.width == 8 { 0xf6 } else { 0xf7 });
    bytes.push(ModRM::new(3, extension, reg.code).encode());
    Ok(())
}

fn encode_shift(ins: &Instruction, extension: u8, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    if ins.operands.len() != 2 {
        return Err(AsmError::EncodeError(format!("{} expects 2 operands", ins.mnemonic)));
    }
    let reg = register_operand(&ins.operands[0])?;
    let (opcode, immediate) = match &ins.operands[1] {
        Operand::Immediate(1) => (if reg.width == 8 { 0xd0 } else { 0xd1 }, None),
        Operand::Immediate(count) if (0..=255).contains(count) => {
            (if reg.width == 8 { 0xc0 } else { 0xc1 }, Some(*count as u8))
        },
        Operand::Register(count) if count == "cl" => {
            (if reg.width == 8 { 0xd2 } else { 0xd3 }, None)
        },
        _ => return Err(AsmError::EncodeError("Shift count must be CL or unsigned imm8".into())),
    };
    emit_operand_size_prefix(bytes, reg.width);
    emit_rex(bytes, reg.width, false, false, reg.code >= 8, reg.high8, reg.rex8)?;
    bytes.push(opcode);
    bytes.push(ModRM::new(3, extension, reg.code).encode());
    if let Some(count) = immediate {
        bytes.push(count);
    }
    Ok(())
}

fn encode_imul_immediate(ins: &Instruction, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    let dst = register_operand(&ins.operands[0])?;
    let src = register_operand(&ins.operands[1])?;
    let Operand::Immediate(imm) = ins.operands[2] else {
        return Err(AsmError::EncodeError("imul third operand must be immediate".into()));
    };
    if dst.width != src.width || !matches!(dst.width, 16 | 32 | 64) {
        return Err(AsmError::EncodeError("imul requires matching r16/r32/r64 operands".into()));
    }
    let short = (-128..=127).contains(&imm);
    let valid = match dst.width {
        16 => (-32768..=65535).contains(&imm),
        32 => (i32::MIN as i128..=u32::MAX as i128).contains(&imm),
        64 => (i32::MIN as i128..=i32::MAX as i128).contains(&imm),
        _ => false,
    };
    if !valid {
        return Err(AsmError::EncodeError(
            "imul immediate out of range (imm16/imm32; r64 sign extends imm32)".into(),
        ));
    }
    emit_operand_size_prefix(bytes, dst.width);
    emit_rex(bytes, dst.width, dst.code >= 8, false, src.code >= 8, false, false)?;
    bytes.push(if short { 0x6b } else { 0x69 });
    bytes.push(ModRM::new(3, dst.code, src.code).encode());
    if short {
        bytes.push(imm as u8);
    } else if dst.width == 16 {
        bytes.extend_from_slice(&(imm as u16).to_le_bytes());
    } else {
        bytes.extend_from_slice(&(imm as u32).to_le_bytes());
    }
    Ok(())
}

fn prefixed_condition(mnemonic: &str, prefix: &str) -> Option<u8> {
    let suffix = mnemonic.strip_prefix(prefix)?;
    CONDITION_CODES
        .iter()
        .find(|(name, _)| name.strip_prefix('j') == Some(suffix))
        .map(|(_, code)| *code)
}

fn encode_carry(
    ins: &Instruction,
    byte_opcode: u8,
    wide_opcode: u8,
    bytes: &mut Vec<u8>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 2 {
        return Err(AsmError::EncodeError(format!("{} expects 2 register operands", ins.mnemonic)));
    }
    let dst = register_operand(&ins.operands[0])?;
    let src = register_operand(&ins.operands[1])?;
    if dst.width != src.width {
        return Err(AsmError::EncodeError("Register width mismatch".into()));
    }
    emit_reg_reg(bytes, if dst.width == 8 { byte_opcode } else { wide_opcode }, &dst, &src)
}

fn encode_setcc(
    ins: &Instruction,
    condition: u8,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 1 {
        return Err(AsmError::EncodeError("setcc expects 1 byte operand".into()));
    }
    emit_group_operand(&ins.operands[0], 8, 8, &[0x0f, 0x90 + condition], 0, bytes, relocs)
}

fn encode_cmovcc(ins: &Instruction, condition: u8, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    if ins.operands.len() != 2 {
        return Err(AsmError::EncodeError("cmovcc expects 2 register operands".into()));
    }
    let dst = register_operand(&ins.operands[0])?;
    let src = register_operand(&ins.operands[1])?;
    if dst.width != src.width || !matches!(dst.width, 16 | 32 | 64) {
        return Err(AsmError::EncodeError("cmovcc requires matching r16/r32/r64 registers".into()));
    }
    emit_operand_size_prefix(bytes, dst.width);
    emit_rex(bytes, dst.width, dst.code >= 8, false, src.code >= 8, false, false)?;
    bytes.extend_from_slice(&[0x0f, 0x40 + condition]);
    bytes.push(ModRM::new(3, dst.code, src.code).encode());
    Ok(())
}

fn encode_indirect_branch(
    ins: &Instruction,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 1 {
        return Err(AsmError::EncodeError(format!("{} expects 1 operand", ins.mnemonic)));
    }
    // Near indirect branches default to 64-bit in long mode; REX.W is unnecessary.
    emit_group_operand(
        &ins.operands[0],
        64,
        0,
        &[0xff],
        if ins.mnemonic == "call" { 2 } else { 4 },
        bytes,
        relocs,
    )
}

fn emit_group_operand(
    operand: &Operand,
    width: u8,
    rex_width: u8,
    opcodes: &[u8],
    extension: u8,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    emit_operand_size_prefix(bytes, width);
    match operand {
        Operand::Register(_) => {
            let reg = register_operand(operand)?;
            if reg.width != width {
                return Err(AsmError::EncodeError(
                    "Register width does not match instruction".into(),
                ));
            }
            emit_rex(bytes, rex_width, false, false, reg.code >= 8, reg.high8, reg.rex8)?;
            bytes.extend_from_slice(opcodes);
            bytes.push(ModRM::new(3, extension, reg.code).encode());
        },
        Operand::Memory(mem) => {
            validate_memory_width(mem, width)?;
            let group = RegInfo { code: extension, width, high8: false, rex8: false };
            emit_memory_operation(bytes, relocs, opcodes, &group, mem, rex_width, 0)?;
        },
        _ => return Err(AsmError::EncodeError("Expected register or memory operand".into())),
    }
    Ok(())
}

fn encode_extension(
    ins: &Instruction,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 2 {
        return Err(AsmError::EncodeError(format!("{} expects 2 operands", ins.mnemonic)));
    }
    let dst = register_operand(&ins.operands[0])?;
    let src_width = match &ins.operands[1] {
        Operand::Register(_) => register_operand(&ins.operands[1])?.width,
        Operand::Memory(mem) => mem.width.ok_or_else(|| {
            AsmError::EncodeError("Extension memory source requires byte/word/dword width".into())
        })?,
        _ => {
            return Err(AsmError::EncodeError("Extension source must be register or memory".into()))
        },
    };
    let valid = if ins.mnemonic == "movsxd" {
        dst.width == 64 && src_width == 32
    } else {
        (src_width == 8 && matches!(dst.width, 16 | 32 | 64))
            || (src_width == 16 && matches!(dst.width, 32 | 64))
    };
    if !valid {
        return Err(AsmError::EncodeError(
            "Invalid extension source/destination width pair".into(),
        ));
    }
    let opcode = if ins.mnemonic == "movsxd" {
        vec![0x63]
    } else {
        vec![
            0x0f,
            match (ins.mnemonic.as_str(), src_width) {
                ("movzx", 8) => 0xb6,
                ("movzx", 16) => 0xb7,
                ("movsx", 8) => 0xbe,
                _ => 0xbf,
            },
        ]
    };
    emit_operand_size_prefix(bytes, dst.width);
    match &ins.operands[1] {
        Operand::Register(_) => {
            let src = register_operand(&ins.operands[1])?;
            emit_rex(bytes, dst.width, dst.code >= 8, false, src.code >= 8, src.high8, src.rex8)?;
            bytes.extend_from_slice(&opcode);
            bytes.push(ModRM::new(3, dst.code, src.code).encode());
        },
        Operand::Memory(mem) => {
            emit_memory_operation(bytes, relocs, &opcode, &dst, mem, dst.width, 0)?;
        },
        _ => unreachable!(),
    }
    Ok(())
}

fn encode_push_immediate(ins: &Instruction, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    if ins.operands.len() != 1 {
        return Err(AsmError::EncodeError("push expects 1 operand".into()));
    }
    let Operand::Immediate(imm) = ins.operands[0] else { unreachable!() };
    if (-128..=127).contains(&imm) {
        bytes.extend_from_slice(&[0x6a, imm as u8]);
    } else if (i32::MIN as i128..=i32::MAX as i128).contains(&imm) {
        bytes.push(0x68);
        bytes.extend_from_slice(&(imm as i32).to_le_bytes());
    } else {
        return Err(AsmError::EncodeError(
            "push immediate requires signed imm8 or signed imm32 in AMD64 mode".into(),
        ));
    }
    Ok(())
}

fn encode_push_pop(
    ins: &Instruction,
    base_opcode: u8,
    bytes: &mut Vec<u8>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 1 {
        return Err(AsmError::EncodeError(format!("{} expects 1 operand", ins.mnemonic)));
    }
    if let Operand::Register(name) = &ins.operands[0] {
        let reg = lookup_reg(name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
        if reg.width == 16 {
            bytes.push(0x66);
        } else if reg.width != 64 {
            return Err(AsmError::EncodeError(
                "push/pop currently support only r16/r64 in amd64 mode".into(),
            ));
        }
        emit_rex(bytes, 0, false, false, reg.code >= 8, false, false)?;
        bytes.push(base_opcode + (reg.code & 7));
        Ok(())
    } else {
        Err(AsmError::EncodeError(format!("{} only supports registers for now", ins.mnemonic)))
    }
}

enum JumpOpcode {
    One(u8),
    Two(u8, u8),
}

struct JumpSpec {
    previous_len: usize,
    force_near: bool,
    near_opcode: JumpOpcode,
    short_opcode: Option<u8>,
    reloc_kind: RelocKind,
    reloc_addend: i64,
    near_len: usize,
}

fn emit_near_opcode(bytes: &mut Vec<u8>, opcode: &JumpOpcode) {
    match opcode {
        JumpOpcode::One(b) => bytes.push(*b),
        JumpOpcode::Two(a, b) => {
            bytes.push(*a);
            bytes.push(*b);
        },
    }
}

fn encode_jump(
    ins: &Instruction,
    spec: JumpSpec,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
    current_section: usize,
    current_offset: usize,
    label_locs: &HashMap<String, (usize, u64)>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 1 {
        return Err(AsmError::EncodeError(format!("{} expects 1 operand", ins.mnemonic)));
    }
    let Some((label, addend)) = symbol_operand(&ins.operands[0]) else {
        return Err(AsmError::EncodeError(format!(
            "{} only supports labels for now",
            ins.mnemonic
        )));
    };
    if addend != 0 {
        return Err(AsmError::EncodeError(format!(
            "{} does not support label addends",
            ins.mnemonic
        )));
    }

    if let Some((target_sec, target_off)) = label_locs.get(&label) {
        if *target_sec == current_section {
            if let Some(short_op) = spec.short_opcode.filter(|_| !spec.force_near) {
                let short_len = 2i64;
                // A forward target moves when this branch itself shrinks.
                // Its previous encoded length keeps the inclusive +127 edge
                // available without ever truncating an out-of-range count.
                let own_shrink = if *target_off > current_offset as u64 {
                    spec.previous_len.saturating_sub(short_len as usize) as i64
                } else {
                    0
                };
                let disp_short =
                    *target_off as i64 - own_shrink - (current_offset as i64 + short_len);
                if (-128..=127).contains(&disp_short) {
                    bytes.push(short_op);
                    bytes.push(disp_short as i8 as u8);
                    return Ok(());
                }
            }

            let disp_near = *target_off as i64 - (current_offset as i64 + spec.near_len as i64);
            if !(i32::MIN as i64..=i32::MAX as i64).contains(&disp_near) {
                return Err(AsmError::EncodeError(format!(
                    "{} target '{}' is out of 32-bit relative range",
                    ins.mnemonic, label
                )));
            }

            emit_near_opcode(bytes, &spec.near_opcode);
            bytes.extend_from_slice(&(disp_near as i32).to_le_bytes());
            return Ok(());
        }
    }

    emit_near_opcode(bytes, &spec.near_opcode);
    relocs.push(Relocation {
        offset: bytes.len(),
        symbol: label,
        kind: spec.reloc_kind,
        addend: spec.reloc_addend,
    });
    bytes.extend_from_slice(&0i32.to_le_bytes());
    Ok(())
}

fn encode_loop(
    ins: &Instruction,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    if ins.operands.len() != 1 {
        return Err(AsmError::EncodeError("loop expects 1 operand".into()));
    }
    let Some((label, addend)) = symbol_operand(&ins.operands[0]) else {
        return Err(AsmError::EncodeError("loop only supports labels for now".into()));
    };
    if addend != 0 {
        return Err(AsmError::EncodeError("loop does not support label addends".into()));
    }

    bytes.push(0xE2);

    relocs.push(Relocation {
        offset: bytes.len(),
        symbol: label,
        kind: RelocKind::Relative8,
        addend: -1,
    });
    bytes.push(0);
    Ok(())
}

fn eval_directive_expr(
    expr: &ExprValue,
    consts: &HashMap<String, i128>,
) -> Result<EvaluatedExpr, AsmError> {
    eval_expr(expr, consts)
}

fn encode_data_expr(
    eval: EvaluatedExpr,
    width: usize,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {
    match eval {
        EvaluatedExpr::Number(n) => {
            match width {
                1 => {
                    if !(-128..=255).contains(&n) {
                        return Err(AsmError::EncodeError("db value out of range".into()));
                    }
                    bytes.push(n as u8);
                },
                2 => {
                    if !(-32768..=65535).contains(&n) {
                        return Err(AsmError::EncodeError("dw value out of range".into()));
                    }
                    bytes.extend_from_slice(&(n as i16).to_le_bytes());
                },
                4 => {
                    if !(i32::MIN as i128..=u32::MAX as i128).contains(&n) {
                        return Err(AsmError::EncodeError("dd value out of range".into()));
                    }
                    bytes.extend_from_slice(&(n as u32).to_le_bytes());
                },
                8 => {
                    if !(i64::MIN as i128..=u64::MAX as i128).contains(&n) {
                        return Err(AsmError::EncodeError("dq value out of range".into()));
                    }
                    bytes.extend_from_slice(&(n as u64).to_le_bytes());
                },
                _ => return Err(AsmError::EncodeError("Unsupported data width".into())),
            }
            Ok(())
        },
        EvaluatedExpr::Symbol { name, addend } => match width {
            4 => {
                relocs.push(Relocation {
                    offset: bytes.len(),
                    symbol: name,
                    kind: RelocKind::Absolute32,
                    addend,
                });
                bytes.extend_from_slice(&0u32.to_le_bytes());
                Ok(())
            },
            8 => {
                relocs.push(Relocation {
                    offset: bytes.len(),
                    symbol: name,
                    kind: RelocKind::Absolute64,
                    addend,
                });
                bytes.extend_from_slice(&0u64.to_le_bytes());
                Ok(())
            },
            _ => Err(AsmError::EncodeError("Only dd/dq support symbolic expressions".into())),
        },
    }
}

fn alignment_value(dir: &Directive, consts: &HashMap<String, i128>) -> Result<u64, AsmError> {
    if dir.values.len() != 1 {
        return Err(AsmError::EncodeError("align expects one power-of-two operand".into()));
    }
    let DirectiveValue::Expr(expr) = &dir.values[0] else {
        return Err(AsmError::EncodeError("align requires numeric expression".into()));
    };
    let EvaluatedExpr::Number(number) = eval_expr(expr, consts)? else {
        return Err(AsmError::EncodeError("align requires a resolved constant".into()));
    };
    let value = u64::try_from(number)
        .ok()
        .filter(|value| value.is_power_of_two())
        .ok_or_else(|| AsmError::EncodeError("align requires a positive power of two".into()))?;
    Ok(value)
}

fn reservation_size(
    unit: u64,
    values: &[DirectiveValue],
    consts: &HashMap<String, i128>,
) -> Result<u64, AsmError> {
    if values.len() != 1 {
        return Err(AsmError::EncodeError("res* expects exactly one count operand".into()));
    }

    let count = match &values[0] {
        DirectiveValue::Expr(expr) => match eval_directive_expr(expr, consts)? {
            EvaluatedExpr::Number(v) => v,
            EvaluatedExpr::Symbol { name, .. } => {
                return Err(AsmError::EncodeError(format!(
                    "res* count must be constant; unresolved symbol '{}'",
                    name
                )))
            },
        },
        DirectiveValue::StringLiteral(_) => {
            return Err(AsmError::EncodeError("res* count must be numeric expression".into()))
        },
    };

    if count < 0 {
        return Err(AsmError::EncodeError("res* count must be non-negative".into()));
    }
    u64::try_from(count)
        .map_err(|_| AsmError::EncodeError("res* size overflow".into()))?
        .checked_mul(unit)
        .ok_or_else(|| AsmError::EncodeError("res* size overflow".into()))
}

fn reserve_bytes(
    bytes: &mut Vec<u8>,
    unit: u64,
    values: &[DirectiveValue],
    consts: &HashMap<String, i128>,
) -> Result<(), AsmError> {
    let extra = usize::try_from(reservation_size(unit, values, consts)?)
        .map_err(|_| AsmError::EncodeError("res* size exceeds host capacity".into()))?;
    let new_len = bytes
        .len()
        .checked_add(extra)
        .ok_or_else(|| AsmError::EncodeError("res* size overflow".into()))?;
    bytes.try_reserve(extra).map_err(|_| AsmError::EncodeError("res* allocation failed".into()))?;
    bytes.resize(new_len, 0);
    Ok(())
}

fn reserve_output(
    bytes: &mut Vec<u8>,
    extra: usize,
    materialized: &mut usize,
    limits: &assembler::AssemblyLimits,
) -> Result<(), AsmError> {
    let total = materialized
        .checked_add(extra)
        .ok_or_else(|| AsmError::EncodeError("Assembly output size overflow".into()))?;
    let section_size = bytes
        .len()
        .checked_add(extra)
        .ok_or_else(|| AsmError::EncodeError("Section output size overflow".into()))?;
    if total > limits.max_output_bytes || section_size > isize::MAX as usize {
        return Err(AsmError::EncodeError(format!(
            "Assembly output limit exceeded (limit {} bytes)",
            limits.max_output_bytes
        )));
    }
    bytes
        .try_reserve(extra)
        .map_err(|_| AsmError::EncodeError("Assembly output allocation failed".into()))?;
    *materialized = total;
    Ok(())
}

fn directive_size(dir: &Directive, consts: &HashMap<String, i128>) -> Result<usize, AsmError> {
    if let Some(unit) = match dir.name.as_str() {
        "resb" => Some(1),
        "resw" => Some(2),
        "resd" => Some(4),
        "resq" => Some(8),
        _ => None,
    } {
        return usize::try_from(reservation_size(unit, &dir.values, consts)?)
            .map_err(|_| AsmError::EncodeError("Reservation size exceeds host capacity".into()));
    }
    let width = match dir.name.as_str() {
        "db" => 1,
        "dw" => 2,
        "dd" => 4,
        "dq" => 8,
        _ => return Err(AsmError::EncodeError(format!("Unknown directive {}", dir.name))),
    };
    dir.values.iter().try_fold(0usize, |size, value| {
        let extra = match value {
            DirectiveValue::StringLiteral(s) if width == 1 => s.len(),
            DirectiveValue::StringLiteral(_) => {
                return Err(AsmError::EncodeError(format!(
                    "{} only supports numeric expressions",
                    dir.name
                )))
            },
            _ => width,
        };
        size.checked_add(extra)
            .ok_or_else(|| AsmError::EncodeError("Directive output size overflow".into()))
    })
}

fn encode_bss_directive(
    dir: &Directive,
    section: &mut AsmSection,
    consts: &HashMap<String, i128>,
) -> Result<(), AsmError> {
    let extra = match dir.name.as_str() {
        "resb" => reservation_size(1, &dir.values, consts)?,
        "resw" => reservation_size(2, &dir.values, consts)?,
        "resd" => reservation_size(4, &dir.values, consts)?,
        "resq" => reservation_size(8, &dir.values, consts)?,
        _ => {
            let size = directive_size(dir, consts)?;
            for value in &dir.values {
                let zero = match value {
                    DirectiveValue::StringLiteral(s) => s.as_slice().iter().all(|b| *b == 0),
                    DirectiveValue::Expr(expr) => {
                        matches!(eval_directive_expr(expr, consts)?, EvaluatedExpr::Number(0))
                    },
                };
                if !zero {
                    return Err(AsmError::EncodeError(
                        "BSS has a nonzero initializer or relocation".into(),
                    ));
                }
            }
            size as u64
        },
    };
    section.zero_fill = section
        .zero_fill
        .checked_add(extra)
        .ok_or_else(|| AsmError::EncodeError("BSS size overflow".into()))?;
    Ok(())
}

fn encode_directive(
    dir: &Directive,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
    consts: &HashMap<String, i128>,
) -> Result<(), AsmError> {
    match dir.name.as_str() {
        "db" => {
            for v in &dir.values {
                match v {
                    DirectiveValue::StringLiteral(s) => bytes.extend_from_slice(s.as_slice()),
                    DirectiveValue::Expr(expr) => {
                        let eval = eval_directive_expr(expr, consts)?;
                        encode_data_expr(eval, 1, bytes, relocs)?;
                    },
                }
            }
            Ok(())
        },
        "dw" => {
            for v in &dir.values {
                let DirectiveValue::Expr(expr) = v else {
                    return Err(AsmError::EncodeError(
                        "dw only supports numeric expressions".into(),
                    ));
                };
                let eval = eval_directive_expr(expr, consts)?;
                encode_data_expr(eval, 2, bytes, relocs)?;
            }
            Ok(())
        },
        "dd" => {
            for v in &dir.values {
                let DirectiveValue::Expr(expr) = v else {
                    return Err(AsmError::EncodeError(
                        "dd only supports numeric expressions".into(),
                    ));
                };
                let eval = eval_directive_expr(expr, consts)?;
                encode_data_expr(eval, 4, bytes, relocs)?;
            }
            Ok(())
        },
        "dq" => {
            for v in &dir.values {
                let DirectiveValue::Expr(expr) = v else {
                    return Err(AsmError::EncodeError(
                        "dq only supports numeric expressions".into(),
                    ));
                };
                let eval = eval_directive_expr(expr, consts)?;
                encode_data_expr(eval, 8, bytes, relocs)?;
            }
            Ok(())
        },
        "resb" => reserve_bytes(bytes, 1, &dir.values, consts),
        "resw" => reserve_bytes(bytes, 2, &dir.values, consts),
        "resd" => reserve_bytes(bytes, 4, &dir.values, consts),
        "resq" => reserve_bytes(bytes, 8, &dir.values, consts),
        _ => Err(AsmError::EncodeError(format!("Unknown directive {}", dir.name))),
    }
}

mod reference_modrm {
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
            (self.mod_bits << 6) | ((self.reg & 7) << 3) | (self.rm & 7)
        }
    }
}

mod reference_rex {
    #[derive(Debug, Clone, Default)]
    pub struct Rex {
        pub w: bool,
        pub r: bool,
        pub x: bool,
        pub b: bool,
    }

    impl Rex {
        pub fn new() -> Self {
            Self { w: false, r: false, x: false, b: false }
        }

        pub fn encode(&self) -> u8 {
            0x40 | ((self.w as u8) << 3)
                | ((self.r as u8) << 2)
                | ((self.x as u8) << 1)
                | (self.b as u8)
        }
    }
}
