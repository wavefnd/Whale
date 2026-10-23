// SPDX-License-Identifier: MPL-2.0
//! Typed control-flow records and analysis limits.
//! Analysis currently reports an explicit unavailable capability.
use crate::{DecodeError, Instruction};

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfgTarget {
    Address(u64),
    Symbol(u64),
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowOverride {
    pub field_offset: usize,
    pub width: u8,
    pub target: CfgTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfgLimits {
    pub max_input_bytes: usize,
    pub max_instructions: usize,
    pub max_blocks: usize,
    pub max_edges: usize,
    pub max_stops: usize,
    pub max_work: u64,
    /// Combined owned workspace, output and normalized input buffer limit.
    pub max_workspace_bytes: usize,
    pub follow_calls: bool,
}

impl Default for CfgLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 8 * 1024 * 1024,
            max_instructions: 16384,
            max_blocks: 8192,
            max_edges: 65536,
            max_stops: 8192,
            max_work: 64 * 1024 * 1024,
            max_workspace_bytes: 128 * 1024 * 1024,
            follow_calls: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfgResource {
    InputBytes,
    Instructions,
    Blocks,
    Edges,
    Stops,
    Work,
    Memory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CfgError {
    CoreUnavailable,
    Invalid(&'static str),
    Limit { resource: CfgResource, limit: u64 },
    Allocation,
    DecodeOutput(DecodeError),
}

impl fmt::Display for CfgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoreUnavailable => {
                f.write_str("control-flow analysis is unavailable in this build")
            },
            Self::Invalid(reason) => write!(f, "invalid control-flow input/result: {reason}"),
            Self::Limit { resource, limit } => {
                write!(f, "control-flow {resource:?} limit exceeded ({limit})")
            },
            Self::Allocation => f.write_str("unable to allocate bounded control-flow buffers"),
            Self::DecodeOutput(error) => write!(f, "control-flow decode record: {error}"),
        }
    }
}

impl std::error::Error for CfgError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::DecodeOutput(e) = self {
            Some(e)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    FallThrough,
    Call,
    Jump,
    ConditionalTaken,
    ConditionalFallThrough,
    LoopTaken,
    LoopFallThrough,
    IndirectCall,
    IndirectJump,
    Syscall,
    Memory,
    Address,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: usize,
    pub kind: EdgeKind,
    pub target: CfgTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopKind {
    Invalid,
    Truncated,
    Unsupported,
    Overlong,
    Overlap,
    EndOfRange,
    IndirectJump,
    Syscall,
    AddressOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stop {
    pub offset: usize,
    pub kind: StopKind,
    pub conflict: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BasicBlock {
    pub offset: usize,
    pub end: usize,
    pub instruction_indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlFlowGraph {
    pub base: u64,
    /// Discovery order; basic blocks are separately ordered by byte offset.
    pub instructions: Vec<Instruction>,
    pub blocks: Vec<BasicBlock>,
    pub edges: Vec<Edge>,
    pub stops: Vec<Stop>,
    pub work: u64,
}

/// Report the missing control-flow capability without interpreting input bytes.
/// The typed input and result structures remain available to callers.
pub fn analyze(
    _input: &[u8],
    _base: u64,
    _seeds: &[usize],
    _overlays: &[FlowOverride],
    _limits: CfgLimits,
) -> Result<ControlFlowGraph, CfgError> {
    Err(CfgError::CoreUnavailable)
}
