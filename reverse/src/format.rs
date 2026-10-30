//! Deterministic human and structured rendering of decoded facts.
use crate::{image::Annotation, Instruction, Operand, PresentationError};

use serde_json::{json, Value};

pub fn operand_text(operand: &Operand, inst: &Instruction, base: u64) -> String {
    match operand {
        Operand::Register(r) => r.name(),
        Operand::Immediate(i) => {
            if i.sign_extended {
                format!("{}", i.signed_value())
            } else {
                format!("0x{:x}", i.value)
            }
        },
        Operand::Relative(r) => inst
            .relative_target(base)
            .map_or_else(|| format!("relative({})", r.displacement), |v| format!("0x{v:x}")),
        Operand::Memory(m) => {
            let mut parts = Vec::new();
            if m.rip_relative {
                parts.push("rel".into());
            } else if m.base.is_none() && m.index.is_none() {
                parts.push("abs".into());
            }
            if let Some(base) = m.base {
                parts.push(base.name());
            }
            if let Some(index) = m.index {
                parts.push(format!("{}*{}", index.name(), m.scale));
            }
            let mut expression = parts.join(" + ");
            if m.rip_relative || (m.base.is_none() && m.index.is_none()) {
                expression.push(' ');
            } else if m.displacement >= 0 && !expression.is_empty() {
                expression.push_str(" + ");
            }
            if m.displacement < 0 {
                expression.push_str(&format!("-0x{:x}", m.displacement.unsigned_abs()));
            } else {
                expression.push_str(&format!("0x{:x}", m.displacement));
            }
            format!(
                "{} [{}]",
                match m.width {
                    8 => "byte",
                    16 => "word",
                    32 => "dword",
                    64 => "qword",
                    _ => "memory",
                },
                expression
            )
        },
    }
}

/// Fallible because public instruction records can contain invalid lengths.
pub fn instruction_text(
    inst: &Instruction,
    base: u64,
    annotations: &[&Annotation],
) -> Result<String, PresentationError> {
    let bytes =
        inst.original_bytes()?.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
    let operands = inst
        .operands
        .iter()
        .map(|o| {
            if matches!(o, Operand::Relative(_)) && !annotations.is_empty() {
                format!("reloc({}{:+})", annotations[0].symbol, annotations[0].addend)
            } else {
                operand_text(o, inst, base)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let address = base
        .checked_add(inst.offset as u64)
        .map_or_else(|| "overflow".into(), |n| format!("{n:016x}"));
    let mut result = format!(
        "{address}  {bytes:<44}  {}{}{}",
        inst.mnemonic(),
        if operands.is_empty() { "" } else { " " },
        operands
    );
    for a in annotations {
        result
            .push_str(&format!(" ; {:?} {}{:+} @section+{}", a.kind, a.symbol, a.addend, a.offset));
    }
    Ok(result)
}

/// Fallible because public instruction records can contain invalid lengths.
pub fn instruction_json(
    inst: &Instruction,
    base: u64,
    annotations: &[&Annotation],
) -> Result<Value, PresentationError> {
    let bytes = inst.original_bytes()?;
    let operands=inst.operands.iter().map(|o|match o{
  Operand::Register(r)=>json!({"kind":"register","name":r.name(),"class":format!("{:?}",r.class),"index":r.index,"width":r.width,"high8":r.high8}),
  Operand::Immediate(i)=>json!({"kind":"immediate","bits":i.value,"width":i.width,"encoded_width":i.encoded_width,"sign_extended":i.sign_extended}),
  Operand::Relative(r)=>json!({"kind":"relative","displacement":r.displacement,"encoded_width":r.encoded_width}),
  Operand::Memory(m)=>json!({"kind":"memory","width":m.width,"base":m.base.map(|r|r.name()),"index":m.index.map(|r|r.name()),"scale":m.scale,"displacement":m.displacement,"rip_relative":m.rip_relative})
 }).collect::<Vec<_>>();
    let field = |f: Option<crate::EncodingField>| {
        f.map(|f| json!({"offset":f.offset,"width_bytes":f.width,"bits":f.bits}))
    };
    Ok(
        json!({"offset":inst.offset,"address":base.checked_add(inst.offset as u64),"length":inst.length,"bytes":bytes,"mnemonic":inst.mnemonic(),"operands":operands,"flow":format!("{:?}",inst.flow),"target":if annotations.is_empty(){inst.relative_target(base)}else{None},"encoding":{"prefixes":inst.encoding.prefixes,"rex":inst.encoding.rex,"opcode_offset":inst.encoding.opcode_offset,"opcode_length":inst.encoding.opcode_len,"modrm":inst.encoding.modrm.map(|b|json!({"offset":b.offset,"value":b.value})),"sib":inst.encoding.sib.map(|b|json!({"offset":b.offset,"value":b.value})),"displacement":field(inst.encoding.displacement),"immediate":field(inst.encoding.immediate)},"relocations":annotations.iter().map(|a|json!({"offset":a.offset,"symbol":a.symbol,"addend":a.addend,"kind":format!("{:?}",a.kind),"width_bytes":a.width})).collect::<Vec<_>>() }),
    )
}
