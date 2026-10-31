// SPDX-License-Identifier: MPL-2.0
//! Typed allocation/snapshot transport for the Wave fixed-width source modules.
use super::*;

use std::borrow::Cow;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FixedText {
    pub data: *const u8,
    pub count: u64,
}

impl Default for FixedText {
    fn default() -> Self {
        Self { data: std::ptr::null(), count: 0 }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FixedInstruction {
    pub mnemonic: FixedText,
    pub operands: *const FixedText,
    pub count: u64,
    pub fold: u64,
}

impl Default for FixedInstruction {
    fn default() -> Self {
        Self { mnemonic: FixedText::default(), operands: std::ptr::null(), count: 0, fold: 0 }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FixedSymbol {
    pub name: FixedText,
    pub section: u64,
    pub offset: u64,
    pub start: u64,
    pub end: u64,
    pub line: u64,
    pub flags: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FixedRelocation {
    pub section: u64,
    pub offset: u64,
    pub symbol: u64,
    pub code: u64,
    pub addend: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FixedContext {
    pub source: FixedText,
    pub namespace: FixedText,
    pub symbols: *mut FixedSymbol,
    pub symbol_count: u64,
    pub table: *mut u64,
    pub table_capacity: u64,
    pub section: u64,
    pub offset: u64,
    pub relocations: *mut FixedRelocation,
    pub relocation_count: u64,
    pub relocation_capacity: u64,
}

impl Default for FixedContext {
    fn default() -> Self {
        Self {
            source: FixedText::default(),
            namespace: FixedText::default(),
            symbols: std::ptr::null_mut(),
            symbol_count: 0,
            table: std::ptr::null_mut(),
            table_capacity: 0,
            section: 0,
            offset: 0,
            relocations: std::ptr::null_mut(),
            relocation_count: 0,
            relocation_capacity: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FixedResult {
    pub status: u64,
    pub error: u64,
    pub start: u64,
    pub end: u64,
    pub line: u64,
    pub previous_start: u64,
    pub previous_end: u64,
    pub previous_line: u64,
    pub text: FixedText,
    pub detail: u64,
    pub display: u64,
    pub word: u64,
    pub sections: u64,
    pub symbols: u64,
    pub relocations: u64,
    pub records: u64,
    pub operands: u64,
    pub bytes: u64,
    pub work: u64,
    pub max_work: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FixedSection {
    pub name: FixedText,
    pub kind: u64,
    pub align: u64,
    pub size: u64,
    pub first: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FixedRecord {
    pub section: u64,
    pub offset: u64,
    pub start: u64,
    pub end: u64,
    pub line: u64,
    pub kind: u64,
    pub unit: u64,
    pub first: u64,
    pub count: u64,
    pub size: u64,
    pub mnemonic: FixedText,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FixedGlobal {
    pub start: u64,
    pub end: u64,
    pub line: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FixedInput {
    pub source: FixedText,
    pub namespace: FixedText,
    pub hash_comments: u64,
    pub mode: u64,
    pub max_output: u64,
    pub max_passes: u64,
    pub max_work: u64,
    pub sections: *mut FixedSection,
    pub section_capacity: u64,
    pub symbols: *mut FixedSymbol,
    pub symbol_capacity: u64,
    pub table: *mut u64,
    pub table_capacity: u64,
    pub globals: *mut FixedGlobal,
    pub global_capacity: u64,
    pub records: *mut FixedRecord,
    pub record_capacity: u64,
    pub operands: *mut FixedText,
    pub operand_capacity: u64,
    pub order: *mut u64,
    pub order_capacity: u64,
    pub relocations: *mut FixedRelocation,
    pub relocation_capacity: u64,
    pub bytes: *mut u8,
    pub byte_capacity: u64,
}

impl Default for FixedInput {
    fn default() -> Self {
        Self {
            source: FixedText::default(),
            namespace: FixedText::default(),
            hash_comments: 0,
            mode: 0,
            max_output: 0,
            max_passes: 0,
            max_work: 0,
            sections: std::ptr::null_mut(),
            section_capacity: 0,
            symbols: std::ptr::null_mut(),
            symbol_capacity: 0,
            table: std::ptr::null_mut(),
            table_capacity: 0,
            globals: std::ptr::null_mut(),
            global_capacity: 0,
            records: std::ptr::null_mut(),
            record_capacity: 0,
            operands: std::ptr::null_mut(),
            operand_capacity: 0,
            order: std::ptr::null_mut(),
            order_capacity: 0,
            relocations: std::ptr::null_mut(),
            relocation_capacity: 0,
            bytes: std::ptr::null_mut(),
            byte_capacity: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FixedState {
    pub packet: *mut FixedInput,
    pub context: FixedContext,
    pub section_count: u64,
    pub symbol_count: u64,
    pub definition_count: u64,
    pub global_count: u64,
    pub record_count: u64,
    pub operand_count: u64,
    pub selected: u64,
    pub materialized: u64,
    pub next: u64,
    pub pending: u64,
}

impl Default for FixedState {
    fn default() -> Self {
        Self {
            packet: std::ptr::null_mut(),
            context: FixedContext::default(),
            section_count: 0,
            symbol_count: 0,
            definition_count: 0,
            global_count: 0,
            record_count: 0,
            operand_count: 0,
            selected: 0,
            materialized: 0,
            next: 0,
            pending: 0,
        }
    }
}

pub type Render = fn(u64, &str, u64) -> AsmError;

pub type Entry = unsafe extern "C" fn(*const FixedInput, *mut FixedResult) -> i32;
pub type Encode =
    unsafe extern "C" fn(*const FixedInstruction, *mut FixedContext, *mut FixedResult) -> i32;

const WORK: u64 = 512 * 1024 * 1024;

const WORKSPACE: usize = 128 * 1024 * 1024;

const MISSING: u64 = u64::MAX;

unsafe extern "C" {
    fn whale_fixed_probe(
        source: *const FixedText,
        hash_comments: u64,
        result: *mut FixedResult,
    ) -> i32;

    fn whale_fixed_scalar(
        mode: u64,
        text: *const FixedText,
        bits: u64,
        value: i64,
        alignment: i64,
        result: *mut FixedResult,
    ) -> i32;

    fn whale_fixed_operands(
        text: *const FixedText,
        output: *mut FixedText,
        capacity: u64,
        count: *mut u64,
        result: *mut FixedResult,
    ) -> i32;

    fn whale_fixed_context_index(context: *mut FixedContext, result: *mut FixedResult) -> i32;

    fn whale_fixed_relative(
        context: *mut FixedContext,
        text: *const FixedText,
        code: u64,
        result: *mut FixedResult,
    ) -> i32;

    fn asm_fixed_count(
        instruction: *const FixedInstruction,
        count: u64,
        result: *mut FixedResult,
    ) -> bool;
}

impl FixedText {
    pub fn new(text: &str) -> Self {
        Self { data: text.as_ptr(), count: text.len() as u64 }
    }
}

impl FixedResult {
    pub fn new() -> Self {
        Self { max_work: WORK, ..Self::default() }
    }
}

fn backend() -> AsmError {
    AsmError::Backend("fixed-width Wave transport")
}

fn allocation() -> AsmError {
    error("unable to reserve assembly output")
}

fn workspace() -> AsmError {
    error("assembly workspace limit exceeded")
}

fn allocate<T: Clone + Default>(count: usize) -> Result<Vec<T>, AsmError> {
    let mut output = Vec::new();
    output.try_reserve_exact(count).map_err(|_| allocation())?;
    output.resize(count, T::default());
    Ok(output)
}

fn charge(used: &mut usize, count: usize, width: usize) -> Result<(), AsmError> {
    *used = used
        .checked_add(count.checked_mul(width).ok_or_else(workspace)?)
        .filter(|n| *n <= WORKSPACE)
        .ok_or_else(workspace)?;
    Ok(())
}

fn owned(text: &str) -> Result<String, AsmError> {
    let mut value = String::new();
    value.try_reserve_exact(text.len()).map_err(|_| allocation())?;
    value.push_str(text);
    Ok(value)
}

fn borrowed<'a>(text: FixedText, inputs: &[&'a str]) -> Result<&'a str, AsmError> {
    if text.count == 0 {
        return Ok("");
    }
    let start = text.data as usize;
    let count = usize::try_from(text.count).map_err(|_| backend())?;
    let end = start.checked_add(count).ok_or_else(backend)?;
    for input in inputs {
        let base = input.as_ptr() as usize;
        let limit = base.checked_add(input.len()).ok_or_else(backend)?;
        if start >= base && end <= limit {
            return input.get(start - base..end - base).ok_or_else(backend);
        }
    }
    Err(backend())
}

fn common_error(code: u64, text: &str, detail: u64) -> Option<AsmError> {
    let message = match code {
        1 => format!("{text} requires {detail} operands"),
        2 => format!("invalid integer {text:?}"),
        3 => "integer overflow".into(),
        4 => "invalid immediate field width".into(),
        5 => format!("immediate must fit signed {detail} bits"),
        6 => format!("immediate must fit unsigned {detail} bits"),
        7 => "invalid displacement field".into(),
        8 => format!("branch displacement must be aligned to {detail} bytes"),
        9 => format!("branch displacement exceeds signed {detail}-bit scaled range"),
        10 => "symbol addend exceeds i64".into(),
        11 => format!("invalid symbol name {text:?}"),
        12 => format!("undefined symbol {text:?}; use .extern for external references"),
        13 => format!("undefined symbol {text:?}"),
        14 => "branch displacement exceeds i64".into(),
        15 => "unbalanced memory operand".into(),
        16 => "unclosed memory operand".into(),
        17 => "operand count exceeds 65536".into(),
        18 => "empty operand".into(),
        30 => "section directive takes no operands".into(),
        31 => "symbol directive requires a name".into(),
        32 => "data directive requires values".into(),
        33 => "invalid reservation size".into(),
        34 => "BSS accepts only labels, alignment and zero reservations".into(),
        35 => "instructions require four-byte alignment in a text section".into(),
        36 => "invalid alignment".into(),
        37 => "alignment overflow".into(),
        38 => "alignment must be a nonzero power of two".into(),
        39 => format!("unsupported directive {text}"),
        40 => "data size overflow".into(),
        70 => "symbol count exceeds 65536".into(),
        71 => format!("duplicate symbol {text:?}"),
        72 => "global count exceeds 65536".into(),
        73 => "external symbol is already defined".into(),
        74 => format!(
            "global symbol {text:?} has no definition; use .extern for external declarations"
        ),
        75 => "section count exceeds 4096".into(),
        76 => "section size overflow".into(),
        77 => "Assembly output limit exceeded".into(),
        78 => "record count exceeds 262144".into(),
        79 => "data value does not fit its width".into(),
        80 => "symbol data requires .word or .quad".into(),
        41 | 90 => "assembly input exceeds 8388608 bytes".into(),
        42 | 91 => "assembly pass budget must be nonzero".into(),
        92 => "unable to reserve assembly output".into(),
        93 => "Assembly work limit exceeded".into(),
        94 => return Some(backend()),
        _ => return None,
    };
    Some(error(message))
}

fn no_isa_error(_: u64, _: &str, _: u64) -> AsmError {
    backend()
}

fn failure(
    result: &FixedResult,
    inputs: &[&str],
    render: Render,
    source: Option<(&str, &str)>,
) -> AsmError {
    if result.status != 1
        || result.error == 0
        || result.display > 1
        || result.work > result.max_work
    {
        return backend();
    }
    let text = match borrowed(result.text, inputs) {
        Ok(text) => text,
        Err(error) => return error,
    };
    let name_bytes = source.map_or(0, |(name, _)| name.len());
    if text
        .len()
        .checked_mul(6)
        .and_then(|n| n.checked_add(name_bytes))
        .is_none_or(|n| n > WORKSPACE.saturating_sub(256))
    {
        return workspace();
    }
    let text = if result.display == 1 {
        let mut value = match owned(text) {
            Ok(value) => value,
            Err(error) => return error,
        };
        value.make_ascii_lowercase();
        Cow::Owned(value)
    } else {
        Cow::Borrowed(text)
    };
    let output = common_error(result.error, &text, result.detail)
        .unwrap_or_else(|| render(result.error, &text, result.detail));
    if matches!(output, AsmError::Backend(_)) {
        return output;
    }
    let Some((name, source)) = source else { return output };
    if result.line == 0 {
        return output;
    }
    let span = |start: u64, end: u64, line: u64| -> Option<SourceSpan> {
        (start <= end && end <= source.len() as u64 && line > 0).then_some(SourceSpan {
            start: start as usize,
            end: end as usize,
            line: usize::try_from(line).ok()?,
            column: 1,
        })
    };
    let Some(current) = span(result.start, result.end, result.line) else { return backend() };
    let previous = if result.previous_line == 0 {
        None
    } else {
        let Some(previous) = span(result.previous_start, result.previous_end, result.previous_line)
        else {
            return backend();
        };
        Some(previous)
    };
    output.at(name, current, previous)
}

fn failed(
    status: i32,
    result: &FixedResult,
    inputs: &[&str],
    render: Render,
    source: Option<(&str, &str)>,
) -> AsmError {
    if status != 1 || i32::try_from(result.status).ok() != Some(status) {
        return backend();
    }
    failure(result, inputs, render, source)
}

fn success(status: i32, result: &FixedResult) -> bool {
    status == 0 && result.status == 0 && result.error == 0 && result.work <= result.max_work
}

pub fn check(
    status: i32,
    result: &FixedResult,
    inputs: &[&str],
    render: Render,
) -> Result<(), AsmError> {
    if success(status, result) {
        Ok(())
    } else {
        Err(failed(status, result, inputs, render, None))
    }
}

fn scalar(
    mode: u64,
    text: &str,
    bits: u32,
    value: i64,
    alignment: i64,
) -> Result<FixedResult, AsmError> {
    let input = FixedText::new(text);
    let mut result = FixedResult::new();
    let status =
        unsafe { whale_fixed_scalar(mode, &input, bits.into(), value, alignment, &mut result) };
    if !success(status, &result) {
        return Err(failed(status, &result, &[text], no_isa_error, None));
    }
    Ok(result)
}

pub fn integer(text: &str) -> Result<i128, AsmError> {
    let result = scalar(0, text, 0, 0, 0)?;
    Ok((u128::from(result.word) | (u128::from(result.detail) << 64)) as i128)
}

pub fn field(text: &str, bits: u32, unsigned: bool) -> Result<u32, AsmError> {
    u32::try_from(scalar(if unsigned { 2 } else { 1 }, text, bits, 0, 0)?.word)
        .map_err(|_| backend())
}

pub fn displacement(value: i64, bits: u32, alignment: i64) -> Result<u32, AsmError> {
    u32::try_from(scalar(3, "", bits, value, alignment)?.word).map_err(|_| backend())
}

pub fn count(instruction: &Instruction, expected: usize) -> Result<(), AsmError> {
    let raw = FixedInstruction {
        mnemonic: FixedText::new(&instruction.mnemonic),
        count: instruction.operands.len() as u64,
        ..FixedInstruction::default()
    };
    let mut result = FixedResult::new();
    if !unsafe { asm_fixed_count(&raw, expected as u64, &mut result) } {
        return Err(failure(&result, &[&instruction.mnemonic], no_isa_error, None));
    }
    if !success(0, &result) {
        return Err(backend());
    }
    Ok(())
}

pub fn operands(text: &str) -> Result<Vec<String>, AsmError> {
    let capacity = text.len().saturating_add(1).min(65536);
    let mut raw = allocate::<FixedText>(capacity)?;
    let input = FixedText::new(text);
    let mut count = 0u64;
    let mut result = FixedResult::new();
    let status = unsafe {
        whale_fixed_operands(&input, raw.as_mut_ptr(), capacity as u64, &mut count, &mut result)
    };
    if !success(status, &result) {
        return Err(failed(status, &result, &[text], no_isa_error, None));
    }
    if count > raw.len() as u64 {
        return Err(backend());
    }
    let mut used = raw.len() * std::mem::size_of::<FixedText>();
    charge(&mut used, count as usize, std::mem::size_of::<String>())?;
    let mut output = Vec::new();
    output.try_reserve_exact(count as usize).map_err(|_| allocation())?;
    for raw in raw.into_iter().take(count as usize) {
        let value = borrowed(raw, &[text])?;
        charge(&mut used, value.len(), 1)?;
        output.push(owned(value)?);
    }
    Ok(output)
}

fn table_capacity(symbols: usize) -> Result<usize, AsmError> {
    symbols.checked_mul(2).and_then(|n| n.max(2).checked_next_power_of_two()).ok_or_else(workspace)
}

fn section_name(text: FixedText, source: &str) -> Result<&str, AsmError> {
    if text.data.is_null() {
        return match text.count {
            0 => Ok(".text"),
            1 => Ok(".data"),
            2 => Ok(".rodata"),
            3 => Ok(".bss"),
            _ => Err(backend()),
        };
    }
    borrowed(text, &[source])
}

fn relocation_kind(namespace: &str, code: u64) -> Result<RelocKind, AsmError> {
    Ok(match code {
        u64::MAX => RelocKind::Absolute32,
        value if value == u64::MAX - 1 => RelocKind::Absolute64,
        value => RelocKind::TargetSpecific {
            namespace: owned(namespace)?,
            code: u32::try_from(value).map_err(|_| backend())?,
        },
    })
}

pub fn assemble(
    name: &str,
    source: &str,
    limits: &AssemblyLimits,
    namespace: &str,
    hash_comments: bool,
    entry: Entry,
    render: Render,
) -> Result<AssemblerOutput, AsmError> {
    // Validate allocation metadata before probing source-sized scratch buffers.
    // The Wave driver independently enforces these same execution limits.
    if source.len() > 8 * 1024 * 1024 {
        return Err(error("assembly input exceeds 8388608 bytes"));
    }
    if limits.max_relax_iterations == 0 {
        return Err(error("assembly pass budget must be nonzero"));
    }
    let source_text = FixedText::new(source);
    let mut probe = FixedResult::new();
    let status = unsafe { whale_fixed_probe(&source_text, u64::from(hash_comments), &mut probe) };
    if !success(status, &probe) {
        return Err(failed(status, &probe, &[source, namespace], render, Some((name, source))));
    }
    let sections = usize::try_from(probe.sections).map_err(|_| backend())?;
    let symbols = usize::try_from(probe.symbols).map_err(|_| backend())?;
    let records = usize::try_from(probe.records).map_err(|_| backend())?;
    let operands = usize::try_from(probe.operands).map_err(|_| backend())?;
    let relocations = usize::try_from(probe.relocations).map_err(|_| backend())?;
    let table_size = table_capacity(symbols)?;
    let mut used = 0;
    charge(&mut used, sections, std::mem::size_of::<FixedSection>())?;
    charge(
        &mut used,
        symbols,
        std::mem::size_of::<FixedSymbol>() + std::mem::size_of::<FixedGlobal>() + 8,
    )?;
    charge(&mut used, table_size, 8)?;
    charge(&mut used, records, std::mem::size_of::<FixedRecord>())?;
    charge(&mut used, operands, std::mem::size_of::<FixedText>())?;
    charge(&mut used, relocations, std::mem::size_of::<FixedRelocation>())?;
    let mut raw_sections = allocate::<FixedSection>(sections)?;
    let mut raw_symbols = allocate::<FixedSymbol>(symbols)?;
    let mut globals = allocate::<FixedGlobal>(symbols)?;
    let mut table = allocate::<u64>(table_size)?;
    let mut raw_records = allocate::<FixedRecord>(records)?;
    let mut raw_operands = allocate::<FixedText>(operands)?;
    let mut order = allocate::<u64>(symbols)?;
    let mut raw_relocations = allocate::<FixedRelocation>(relocations)?;
    let mut input = FixedInput {
        source: source_text,
        namespace: FixedText::new(namespace),
        hash_comments: u64::from(hash_comments),
        mode: 0,
        max_output: limits.max_output_bytes as u64,
        max_passes: limits.max_relax_iterations as u64,
        max_work: WORK.saturating_sub(probe.work),
        sections: raw_sections.as_mut_ptr(),
        section_capacity: sections as u64,
        symbols: raw_symbols.as_mut_ptr(),
        symbol_capacity: symbols as u64,
        table: table.as_mut_ptr(),
        table_capacity: table_size as u64,
        globals: globals.as_mut_ptr(),
        global_capacity: symbols as u64,
        records: raw_records.as_mut_ptr(),
        record_capacity: records as u64,
        operands: raw_operands.as_mut_ptr(),
        operand_capacity: operands as u64,
        order: order.as_mut_ptr(),
        order_capacity: symbols as u64,
        relocations: raw_relocations.as_mut_ptr(),
        relocation_capacity: relocations as u64,
        bytes: std::ptr::null_mut(),
        byte_capacity: 0,
    };
    let valid = |result: &FixedResult, input: &FixedInput| {
        result.sections <= input.section_capacity
            && result.symbols <= input.symbol_capacity
            && result.records <= input.record_capacity
            && result.operands <= input.operand_capacity
            && result.relocations <= input.relocation_capacity
            && result.bytes <= input.max_output
            && result.bytes <= 256 * 1024 * 1024
            && result.work <= input.max_work
    };
    let mut result = FixedResult::new();
    let status = unsafe { entry(&input, &mut result) };
    if !success(status, &result) {
        return Err(failed(status, &result, &[source, namespace], render, Some((name, source))));
    }
    if !valid(&result, &input) {
        return Err(backend());
    }
    let size = usize::try_from(result.bytes).map_err(|_| backend())?;
    // The final payload is bounded separately by max_output. Its native staging
    // copy and every reconstructed name/container are admitted as workspace.
    charge(&mut used, size, 1)?;
    let mut bytes = allocate::<u8>(size)?;
    input.mode = 1;
    input.bytes = bytes.as_mut_ptr();
    input.byte_capacity = bytes.len() as u64;
    input.max_work = input.max_work.saturating_sub(result.work);
    let status = unsafe { entry(&input, &mut result) };
    if !success(status, &result) {
        return Err(failed(status, &result, &[source, namespace], render, Some((name, source))));
    }
    if !valid(&result, &input) || result.bytes != bytes.len() as u64 {
        return Err(backend());
    }
    charge(&mut used, result.sections as usize, std::mem::size_of::<AsmSection>())?;
    charge(&mut used, result.symbols as usize, std::mem::size_of::<AsmSymbol>() + 1)?;
    charge(&mut used, result.relocations as usize, 2 * std::mem::size_of::<Relocation>())?;
    for section in raw_sections.iter().take(result.sections as usize) {
        charge(&mut used, section_name(section.name, source)?.len(), 1)?;
    }
    for symbol in raw_symbols.iter().take(result.symbols as usize) {
        charge(&mut used, borrowed(symbol.name, &[source])?.len(), 1)?;
    }
    for relocation in raw_relocations.iter().take(result.relocations as usize) {
        let symbol = raw_symbols
            .get(usize::try_from(relocation.symbol).map_err(|_| backend())?)
            .filter(|_| relocation.symbol < result.symbols)
            .ok_or_else(backend)?;
        charge(&mut used, borrowed(symbol.name, &[source])?.len(), 1)?;
        if relocation.code < u64::MAX - 1 {
            charge(&mut used, namespace.len(), 1)?;
        }
    }
    let mut output = AssemblerOutput { sections: Vec::new(), symbols: Vec::new() };
    output.sections.try_reserve_exact(result.sections as usize).map_err(|_| allocation())?;
    let mut first = 0usize;
    for section in raw_sections.iter().take(result.sections as usize) {
        if !section.align.is_power_of_two() || section.first != first as u64 {
            return Err(backend());
        }
        let kind = match section.kind {
            0 => AsmSectionKind::Text,
            1 => AsmSectionKind::Data,
            2 => AsmSectionKind::ReadOnlyData,
            3 => AsmSectionKind::Bss,
            _ => return Err(backend()),
        };
        let data = if kind == AsmSectionKind::Bss {
            Vec::new()
        } else {
            let length = usize::try_from(section.size).map_err(|_| backend())?;
            let end = first.checked_add(length).ok_or_else(backend)?;
            let slice = bytes.get(first..end).ok_or_else(backend)?;
            let mut data = Vec::new();
            data.try_reserve_exact(length).map_err(|_| allocation())?;
            data.extend_from_slice(slice);
            first = end;
            data
        };
        output.sections.push(AsmSection {
            name: owned(section_name(section.name, source)?)?,
            kind,
            align: section.align,
            data,
            zero_fill: if kind == AsmSectionKind::Bss { section.size } else { 0 },
            relocs: Vec::new(),
        });
    }
    if first != bytes.len() {
        return Err(backend());
    }
    let mut seen = allocate::<bool>(result.symbols as usize)?;
    output.symbols.try_reserve_exact(result.symbols as usize).map_err(|_| allocation())?;
    for id in order.into_iter().take(result.symbols as usize) {
        let index = usize::try_from(id).map_err(|_| backend())?;
        let seen = seen.get_mut(index).ok_or_else(backend)?;
        if *seen {
            return Err(backend());
        }
        *seen = true;
        let symbol = raw_symbols.get(index).ok_or_else(backend)?;
        if symbol.flags & !7 != 0 || symbol.flags & 6 == 0 {
            return Err(backend());
        }
        let section_index = if symbol.section == MISSING {
            None
        } else {
            let index = usize::try_from(symbol.section).map_err(|_| backend())?;
            let section = output.sections.get(index).ok_or_else(backend)?;
            if symbol.offset > section.data.len() as u64 + section.zero_fill {
                return Err(backend());
            }
            Some(index)
        };
        output.symbols.push(AsmSymbol {
            name: owned(borrowed(symbol.name, &[source])?)?,
            section_index,
            offset: symbol.offset,
            is_global: symbol.flags & 1 != 0,
            kind: SymbolKind::NoType,
            size: 0,
            visibility: SymbolVisibility::Default,
        });
    }
    for relocation in raw_relocations.into_iter().take(result.relocations as usize) {
        let section = output
            .sections
            .get_mut(usize::try_from(relocation.section).map_err(|_| backend())?)
            .ok_or_else(backend)?;
        let symbol = raw_symbols
            .get(usize::try_from(relocation.symbol).map_err(|_| backend())?)
            .ok_or_else(backend)?;
        let width = if relocation.code == u64::MAX - 1 { 8 } else { 4 };
        let offset = usize::try_from(relocation.offset).map_err(|_| backend())?;
        if offset.checked_add(width).is_none_or(|end| end > section.data.len()) {
            return Err(backend());
        }
        section.relocs.try_reserve(1).map_err(|_| allocation())?;
        section.relocs.push(Relocation {
            offset,
            symbol: owned(borrowed(symbol.name, &[source])?)?,
            addend: relocation.addend,
            kind: relocation_kind(namespace, relocation.code)?,
        });
    }
    Ok(output)
}

struct Snapshot<'a> {
    symbols: Vec<FixedSymbol>,
    table: Vec<u64>,
    relocations: Vec<FixedRelocation>,
    operands: Vec<FixedText>,
    inputs: Vec<&'a str>,
    namespace: &'a str,
    section: u64,
    offset: u64,
}

impl<'a> Snapshot<'a> {
    fn new(context: &Context<'a>, instruction: &'a Instruction) -> Result<Self, AsmError> {
        let capacity = table_capacity(context.symbols.len())?;
        let mut used = 0usize;
        charge(
            &mut used,
            context.symbols.len(),
            std::mem::size_of::<FixedSymbol>() + std::mem::size_of::<&str>(),
        )?;
        charge(&mut used, capacity, 8)?;
        charge(
            &mut used,
            instruction.operands.len(),
            std::mem::size_of::<FixedText>() + std::mem::size_of::<&str>(),
        )?;
        let mut symbols = Vec::new();
        symbols.try_reserve_exact(context.symbols.len()).map_err(|_| allocation())?;
        let mut operands = Vec::new();
        operands.try_reserve_exact(instruction.operands.len()).map_err(|_| allocation())?;
        let mut inputs = Vec::new();
        inputs
            .try_reserve_exact(
                context
                    .symbols
                    .len()
                    .checked_add(instruction.operands.len())
                    .and_then(|n| n.checked_add(2))
                    .ok_or_else(workspace)?,
            )
            .map_err(|_| allocation())?;
        inputs.push(instruction.mnemonic.as_str());
        inputs.push(context.namespace);
        for operand in &instruction.operands {
            operands.push(FixedText::new(operand));
            inputs.push(operand.as_str());
        }
        for (name, symbol) in context.symbols {
            symbols.push(FixedSymbol {
                name: FixedText::new(name),
                section: symbol.section.map_or(MISSING, |index| index as u64),
                offset: symbol.offset,
                start: symbol.span.start as u64,
                end: symbol.span.end as u64,
                line: symbol.span.line as u64,
                flags: if symbol.section.is_some() { 2 } else { 4 },
            });
            inputs.push(name.as_str());
        }
        Ok(Self {
            symbols,
            table: allocate(capacity)?,
            relocations: allocate(1)?,
            operands,
            inputs,
            namespace: context.namespace,
            section: context.section as u64,
            offset: context.offset,
        })
    }

    fn context(&mut self) -> FixedContext {
        FixedContext {
            namespace: FixedText::new(self.namespace),
            symbols: self.symbols.as_mut_ptr(),
            symbol_count: self.symbols.len() as u64,
            table: self.table.as_mut_ptr(),
            table_capacity: self.table.len() as u64,
            section: self.section,
            offset: self.offset,
            relocations: self.relocations.as_mut_ptr(),
            relocation_capacity: self.relocations.len() as u64,
            ..FixedContext::default()
        }
    }

    fn additions(&self, count: u64) -> Result<Vec<Relocation>, AsmError> {
        if count > self.relocations.len() as u64 {
            return Err(backend());
        }
        let mut output = Vec::new();
        output.try_reserve_exact(count as usize).map_err(|_| allocation())?;
        let mut used = 0usize;
        for relocation in self.relocations.iter().take(count as usize) {
            if relocation.section != self.section || relocation.offset != self.offset {
                return Err(backend());
            }
            let symbol = self
                .symbols
                .get(usize::try_from(relocation.symbol).map_err(|_| backend())?)
                .ok_or_else(backend)?;
            let name = borrowed(symbol.name, &self.inputs)?;
            charge(&mut used, name.len(), 1)?;
            charge(&mut used, self.namespace.len(), 1)?;
            output.push(Relocation {
                offset: usize::try_from(relocation.offset).map_err(|_| backend())?,
                symbol: owned(name)?,
                kind: relocation_kind(self.namespace, relocation.code)?,
                addend: relocation.addend,
            });
        }
        Ok(output)
    }
}

pub fn encode(
    instruction: &Instruction,
    context: &mut Context<'_>,
    encode: Encode,
    render: Render,
) -> Result<u32, AsmError> {
    let mut snapshot = Snapshot::new(context, instruction)?;
    let mut raw_context = snapshot.context();
    let raw = FixedInstruction {
        mnemonic: FixedText::new(&instruction.mnemonic),
        operands: snapshot.operands.as_ptr(),
        count: snapshot.operands.len() as u64,
        fold: 0,
    };
    let mut result = FixedResult::new();
    let status = unsafe { whale_fixed_context_index(&mut raw_context, &mut result) };
    if !success(status, &result) {
        return Err(failed(status, &result, &snapshot.inputs, render, None));
    }
    let status = unsafe { encode(&raw, &mut raw_context, &mut result) };
    if !success(status, &result) {
        return Err(failed(status, &result, &snapshot.inputs, render, None));
    }
    let word = u32::try_from(result.word).map_err(|_| backend())?;
    let additions = snapshot.additions(raw_context.relocation_count)?;
    context.relocations.try_reserve(additions.len()).map_err(|_| allocation())?;
    context.relocations.extend(additions);
    Ok(word)
}

pub fn pc_relative(context: &mut Context<'_>, text: &str, code: u32) -> Result<i64, AsmError> {
    let instruction = Instruction { mnemonic: String::new(), operands: vec![owned(text)?] };
    let mut snapshot = Snapshot::new(context, &instruction)?;
    let mut raw_context = snapshot.context();
    let raw_text = FixedText::new(text);
    let mut result = FixedResult::new();
    let status = unsafe { whale_fixed_context_index(&mut raw_context, &mut result) };
    if !success(status, &result) {
        return Err(failed(status, &result, &snapshot.inputs, no_isa_error, None));
    }
    let status =
        unsafe { whale_fixed_relative(&mut raw_context, &raw_text, code.into(), &mut result) };
    if !success(status, &result) {
        return Err(failed(status, &result, &[text], no_isa_error, None));
    }
    let additions = snapshot.additions(raw_context.relocation_count)?;
    context.relocations.try_reserve(additions.len()).map_err(|_| allocation())?;
    context.relocations.extend(additions);
    Ok(result.word as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_native_faults_cannot_be_reclassified_as_source_errors() {
        let mut result = FixedResult {
            status: 1,
            error: 5,
            detail: 12,
            text: FixedText::new("4096"),
            ..FixedResult::new()
        };
        assert!(matches!(
            check(1, &result, &["4096"], no_isa_error),
            Err(AsmError::EncodeError(_))
        ));
        assert!(matches!(check(2, &result, &["4096"], no_isa_error), Err(AsmError::Backend(_))));
        result.text = FixedText { data: std::ptr::null(), count: 1 };
        assert!(matches!(check(1, &result, &["4096"], no_isa_error), Err(AsmError::Backend(_))));
        result.text = FixedText::new("4096");
        result.work = result.max_work + 1;
        assert!(matches!(check(1, &result, &["4096"], no_isa_error), Err(AsmError::Backend(_))));
    }

    #[test]
    #[cfg(feature = "rust-reference")]
    fn public_context_relative_transport_matches_reference_and_appends_only_native_relocations() {
        let span = SourceSpan { start: 0, end: 1, line: 1, column: 1 };
        let symbols = BTreeMap::from([
            ("target".into(), Symbol { section: Some(2), offset: 8, span }),
            ("ext".into(), Symbol { section: None, offset: 0, span }),
        ]);
        for text in ["target+2", "ext-4", "missing", "4", "18446744073709551616"] {
            let mut actual_relocations = Vec::new();
            let mut reference_relocations = Vec::new();
            let mut actual = Context {
                namespace: "riscv64",
                section: 2,
                offset: 4,
                symbols: &symbols,
                relocations: &mut actual_relocations,
            };
            let mut expected = Context {
                namespace: "riscv64",
                section: 2,
                offset: 4,
                symbols: &symbols,
                relocations: &mut reference_relocations,
            };
            match (actual.pc_relative(text, 17), expected.pc_relative_reference(text, 17)) {
                (Ok(actual), Ok(expected)) => assert_eq!(actual, expected),
                (Err(actual), Err(expected)) => {
                    assert_eq!(actual.to_string(), expected.to_string())
                },
                _ => panic!("relative status differs for {text:?}"),
            }
            let snapshot = |relocations: &[Relocation]| {
                relocations
                    .iter()
                    .map(|r| format!("{}:{:?}:{}:{}", r.offset, r.kind, r.symbol, r.addend))
                    .collect::<Vec<_>>()
            };
            assert_eq!(snapshot(&actual_relocations), snapshot(&reference_relocations));
        }
    }

    #[test]
    #[cfg(feature = "rust-reference")]
    fn common_numeric_and_operand_facades_match_explicit_rust_reference() {
        for text in [
            "0",
            "-0",
            "+0",
            "# 42",
            " - 17 ",
            "0xFF",
            "0x+1",
            "0b+1",
            "0x-1",
            "-0x-1",
            "0x__+1",
            "0x-_1",
            "0x-80000000000000000000000000000000",
            "-0x-80000000000000000000000000000000",
            "0b1010",
            "0_1",
            "0x__FF",
            "170141183460469231731687303715884105727",
            "-170141183460469231731687303715884105727",
            "170141183460469231731687303715884105728",
            "-170141183460469231731687303715884105728",
            "",
            "#",
            "_1",
            "--1",
            "1_",
            "１２",
            "\u{2003}42\u{0085}",
        ] {
            let actual = integer(text);
            let expected = reference::integer(text);
            match (actual, expected) {
                (Ok(actual), Ok(expected)) => assert_eq!(actual, expected, "{text:?}"),
                (Err(actual), Err(expected)) => {
                    assert_eq!(actual.to_string(), expected.to_string(), "{text:?}")
                },
                _ => panic!("numeric status differs for {text:?}"),
            }
            for width in [0, 1, 5, 8, 12, 16, 20, 32, 33] {
                for unsigned in [false, true] {
                    let actual = field(text, width, unsigned);
                    let expected = if unsigned {
                        reference::unsigned(text, width)
                    } else {
                        reference::signed(text, width)
                    };
                    match (actual, expected) {
                        (Ok(actual), Ok(expected)) => assert_eq!(actual, expected),
                        (Err(actual), Err(expected)) => assert_eq!(
                            actual.to_string(),
                            expected.to_string(),
                            "{text:?}, {width}"
                        ),
                        _ => panic!("field status differs"),
                    }
                }
            }
        }
        for text in [
            "",
            "a0,a1,12(sp)",
            " [x0, x1], x2 ",
            "((a,b)),c",
            "a,,b",
            "(a]",
            "(a",
            ",",
            "a,",
            "\u{2003}a\u{0085},b",
        ] {
            match (operands(text), reference::operands(text)) {
                (Ok(actual), Ok(expected)) => assert_eq!(actual, expected),
                (Err(actual), Err(expected)) => {
                    assert_eq!(actual.to_string(), expected.to_string())
                },
                _ => panic!("operand status differs for {text:?}"),
            }
        }
    }
}
