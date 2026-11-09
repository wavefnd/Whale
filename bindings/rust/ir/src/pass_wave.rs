// SPDX-License-Identifier: MPL-2.0
//! Typed host actions for the Wave transactional pass controller.
use super::*;

use std::io::{self, Write};

#[repr(C)]
#[derive(Default)]
struct State {
    phase: u64,
    epoch0: u64,
    epoch1: u64,
    epoch2: u64,
    work: u64,
    changed: u64,
    mask: u64,
    failure_stage: u64,
    failure_detail: u64,
}

#[repr(C)]
#[derive(Default)]
struct Reply {
    action: u64,
    failed: u64,
    changed: u64,
    mask: u64,
}

#[repr(C)]
#[derive(Default)]
struct Output {
    status: u64,
    action: u64,
    stage: u64,
    detail: u64,
    changed: u64,
    mask: u64,
    epoch0: u64,
    epoch1: u64,
    epoch2: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_pass_step(
        state: *mut State,
        reply: *const Reply,
        before: *const u8,
        before_count: u64,
        after: *const u8,
        after_count: u64,
        max_work: u64,
        result: *mut Output,
    ) -> i32;

    fn whale_ir_analysis_current(
        cache_identity: u64,
        cache_epoch: u64,
        state_identity: u64,
        epoch0: u64,
        epoch1: u64,
        epoch2: u64,
        kind: u64,
    ) -> i32;
}

pub(super) fn current<T>(cache: &AnalysisCache<T>, state: &AnalysisState) -> bool {
    let result = unsafe {
        whale_ir_analysis_current(
            std::sync::Arc::as_ptr(&cache.identity) as usize as u64,
            cache.epoch,
            std::sync::Arc::as_ptr(&state.identity) as usize as u64,
            state.epochs[0],
            state.epochs[1],
            state.epochs[2],
            cache.kind as u64,
        )
    };
    match result {
        0 => false,
        1 => true,
        _ => panic!("Wave analysis cache transport"),
    }
}

struct Packet {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for Packet {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("pass equality packet limit exceeded"));
        }
        self.bytes.try_reserve(bytes.len()).map_err(io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn packet(module: &Module, limits: IrLimits) -> Result<Vec<u8>, String> {
    let ceiling = limits.max_analysis_work.saturating_mul(8).min(128 * 1024 * 1024);
    let mut packet = Packet { bytes: Vec::new(), limit: ceiling };
    packet.write_all(&[0; 8]).map_err(|e| e.to_string())?;
    crate::write_module(
        module,
        &mut packet,
        crate::PrintLimits {
            max_output_bytes: ceiling.saturating_sub(8),
            max_nodes: limits.max_nodes,
            max_work: limits.max_analysis_work,
            max_depth: limits.max_type_depth.max(limits.max_const_depth),
            ..crate::PrintLimits::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let canonical_length = packet.bytes.len() as u64 - 8;
    packet.bytes[..8].copy_from_slice(&canonical_length.to_le_bytes());
    // Canonical text contains every verified field except this ordered table.
    // Its types match the printed definitions exactly by pre/post verification.
    packet.write_all(&(module.functions.len() as u64).to_le_bytes()).map_err(|e| e.to_string())?;
    for function in &module.functions {
        packet.write_all(&function.id.0.to_le_bytes()).map_err(|e| e.to_string())?;
        packet
            .write_all(&(function.value_types.len() as u64).to_le_bytes())
            .map_err(|e| e.to_string())?;
        for (id, _) in &function.value_types {
            packet.write_all(&id.0.to_le_bytes()).map_err(|e| e.to_string())?;
        }
    }
    Ok(packet.bytes)
}

pub(super) fn run(
    module: &mut Module,
    pass: &mut impl Pass,
    analyses: &mut AnalysisState,
    limits: IrLimits,
) -> Result<PassChange, PassError> {
    let name = pass.name().to_owned();
    let target = module.target.clone();
    let original_epochs = analyses.epochs;
    let error =
        |stage, cause| PassError { pass: name.clone(), target: target.clone(), stage, cause };
    let mut state = State {
        epoch0: original_epochs[0],
        epoch1: original_epochs[1],
        epoch2: original_epochs[2],
        ..State::default()
    };
    let mut reply = Reply::default();
    let mut before_packet = Vec::new();
    let mut after_packet = Vec::new();
    let mut backup = None;
    let mut cause = None;
    // A complete run has at most nine typed actions; no unbounded host loop.
    for _ in 0..12 {
        let mut output = Output::default();
        let status = unsafe {
            whale_ir_pass_step(
                &mut state,
                &reply,
                before_packet.as_ptr(),
                before_packet.len() as u64,
                after_packet.as_ptr(),
                after_packet.len() as u64,
                limits.max_analysis_work as u64,
                &mut output,
            )
        };
        if status != 0 || output.status != 0 || output.changed > 1 || output.mask > 7 {
            break;
        }
        reply = Reply { action: output.action, ..Reply::default() };
        match output.action {
            1 | 4 => {
                if let Err(e) = crate::verify_module_with_limits(module, limits) {
                    cause = Some(PassFailure::Verification(Box::new(e)));
                    reply.failed = 1;
                }
            },
            2 => match packet(module, limits) {
                Ok(bytes) => {
                    before_packet = bytes;
                    backup = Some(module.clone());
                },
                Err(e) => {
                    cause = Some(PassFailure::Message(e));
                    reply.failed = 1;
                },
            },
            3 => {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pass.run(module))) {
                    Ok(Ok(change)) => {
                        reply.changed = u64::from(change.changed);
                        reply.mask = u64::from(change.invalidates.cfg)
                            | (u64::from(change.invalidates.values) << 1)
                            | (u64::from(change.invalidates.effects) << 2);
                    },
                    Ok(Err(e)) => {
                        cause = Some(PassFailure::Message(e));
                        reply.failed = 1;
                    },
                    Err(_) => {
                        cause = Some(PassFailure::Message("transform panicked".into()));
                        reply.failed = 1;
                    },
                }
            },
            5 => match packet(module, limits) {
                Ok(bytes) => after_packet = bytes,
                Err(e) => {
                    cause = Some(PassFailure::Message(e));
                    reply.failed = 1;
                },
            },
            7 => analyses.epochs = [output.epoch0, output.epoch1, output.epoch2],
            8 => {
                if let Some(before) = backup.take() {
                    discard(std::mem::replace(module, before));
                }
                analyses.epochs = original_epochs;
            },
            9 => {
                return Ok(PassChange {
                    changed: output.changed != 0,
                    invalidates: Invalidates {
                        cfg: output.mask & 1 != 0,
                        values: output.mask & 2 != 0,
                        effects: output.mask & 4 != 0,
                    },
                });
            },
            10 => {
                let stage = match output.stage {
                    0 => PassStage::Before,
                    1 => PassStage::Transform,
                    2 => PassStage::After,
                    3 => PassStage::Contract,
                    _ => break,
                };
                let message = match output.detail {
                    0 => None,
                    1 => {
                        Some("report must match the mutation and invalidate all analyses on change")
                    },
                    2 => Some("analysis epoch exhausted"),
                    3 => Some("pass equality work limit exceeded"),
                    _ => break,
                };
                return Err(error(
                    stage,
                    message.map(|s| PassFailure::Message(s.into())).or(cause).unwrap_or_else(
                        || PassFailure::Message("Wave pass controller missing diagnostic".into()),
                    ),
                ));
            },
            _ => break,
        }
    }
    if let Some(before) = backup {
        discard(std::mem::replace(module, before));
    }
    analyses.epochs = original_epochs;
    Err(error(PassStage::Contract, PassFailure::Message("Wave pass controller transport".into())))
}
