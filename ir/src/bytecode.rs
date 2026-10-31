// SPDX-License-Identifier: MPL-2.0
//! Versioned Whale bytecode. Wave writes, loads, verifies and executes the format;
//! Rust owns initialized buffers and the existing typed memory adapter.
use crate::{ConstValue, FunctionId, InterpreterError, InterpreterOptions, Module, Type};

/// Resource ceilings apply to the entire image, including unreachable functions.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct BytecodeLimits {
    pub max_input_bytes: u64,
    pub max_decoded_bytes: u64,
    pub max_records: u64,
    pub max_work: u64,
    pub max_type_depth: u64,
    pub max_workspace_bytes: u64,
}

impl Default for BytecodeLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 64 * 1024 * 1024,
            max_decoded_bytes: 128 * 1024 * 1024,
            max_records: 1_000_000,
            max_work: 67_108_864,
            max_type_depth: 128,
            max_workspace_bytes: 256 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub enum BytecodeError {
    Unavailable,
    Target,
    Allocation,
    Invalid { status: u64, code: u64, table: u64, index: u64 },
    Backend(&'static str),
    Interpreter(InterpreterError),
}

impl std::fmt::Display for BytecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("Wave bytecode is unavailable in this build"),
            Self::Target => {
                f.write_str("bytecode requires the whale-bytecode-v1 target and layout")
            },
            Self::Allocation => f.write_str("unable to allocate bounded bytecode buffers"),
            Self::Invalid { status, code, table, index } => {
                write!(f,
                "invalid bytecode (status {status}, code {code}, table {table}, record {index})")
            },
            Self::Backend(reason) => f.write_str(reason),
            Self::Interpreter(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for BytecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Interpreter(error) => Some(error),
            _ => None,
        }
    }
}

impl From<InterpreterError> for BytecodeError {
    fn from(error: InterpreterError) -> Self {
        Self::Interpreter(error)
    }
}

type Result<T> = std::result::Result<T, BytecodeError>;

pub const fn available() -> bool {
    cfg!(whale_wave_ir)
}

#[derive(Clone, Debug)]
pub struct FunctionInfo {
    pub id: FunctionId,
    pub params: Vec<Type>,
    pub ret: Type,
    pub has_body: bool,
}

#[derive(Clone, Debug)]
pub struct ImageInfo {
    pub functions: Vec<FunctionInfo>,
    pub globals: usize,
    pub instructions: usize,
    pub decoded_bytes: u64,
}

/// A typed value preserves callable origin without granting pointer authority.
#[derive(Clone, Debug)]
pub struct BytecodeValue {
    value: crate::RuntimeValue<'static>,
    #[cfg_attr(not(whale_wave_ir), allow(dead_code))]
    origin: u64,
}

impl BytecodeValue {
    pub fn scalar(ty: Type, value: ConstValue) -> std::result::Result<Self, crate::CallError> {
        Ok(Self { value: crate::RuntimeValue::scalar(ty, value)?, origin: 0 })
    }

    pub fn raw_pointer(ty: Type, address: u64) -> std::result::Result<Self, crate::CallError> {
        Ok(Self { value: crate::RuntimeValue::raw_pointer(ty, address)?, origin: 0 })
    }

    pub fn null_function(
        signature: crate::FunctionSignature,
    ) -> std::result::Result<Self, crate::CallError> {
        Ok(Self { value: crate::RuntimeValue::null_function(signature)?, origin: 0 })
    }

    pub fn ty(&self) -> &Type {
        self.value.ty()
    }

    pub fn scalar_value(&self) -> Option<&ConstValue> {
        self.value.scalar_value()
    }

    pub fn pointer_address(&self) -> Option<u64> {
        self.value.pointer_address()
    }

    pub fn function_target(&self) -> Option<FunctionId> {
        self.value.function_target()
    }
}

#[derive(Clone, Debug)]
pub struct BytecodeRunResult {
    pub value: Option<BytecodeValue>,
    pub steps: u64,
}

/// A fully verified owned image. No disk-derived runtime layout plans are trusted.
pub struct BytecodeProgram {
    #[cfg(whale_wave_ir)]
    native: native::Owned<'static>,
    #[cfg_attr(not(whale_wave_ir), allow(dead_code))]
    limits: BytecodeLimits,
    #[cfg_attr(not(whale_wave_ir), allow(dead_code))]
    identity: u64,
}

impl BytecodeProgram {
    pub fn load(bytes: &[u8], limits: BytecodeLimits) -> Result<Self> {
        #[cfg(whale_wave_ir)]
        {
            native::load(bytes, limits)
        }
        #[cfg(not(whale_wave_ir))]
        {
            let _ = (bytes, limits);
            Err(BytecodeError::Unavailable)
        }
    }

    pub fn verify(&mut self) -> Result<ImageInfo> {
        #[cfg(whale_wave_ir)]
        {
            self.native.verify(self.limits)?;
            self.native.info()
        }
        #[cfg(not(whale_wave_ir))]
        {
            Err(BytecodeError::Unavailable)
        }
    }

    pub fn function(&self, id: FunctionId) -> Result<Option<FunctionInfo>> {
        #[cfg(whale_wave_ir)]
        {
            self.native.function(id)
        }
        #[cfg(not(whale_wave_ir))]
        {
            let _ = id;
            Err(BytecodeError::Unavailable)
        }
    }

    pub fn run(
        &mut self,
        function: FunctionId,
        arguments: &[ConstValue],
        options: InterpreterOptions,
    ) -> Result<BytecodeRunResult> {
        #[cfg(whale_wave_ir)]
        {
            self.native.run(function, arguments, options, self.limits, self.identity)
        }
        #[cfg(not(whale_wave_ir))]
        {
            let _ = (function, arguments, options);
            Err(BytecodeError::Unavailable)
        }
    }

    pub fn run_values(
        &mut self,
        function: FunctionId,
        arguments: &[BytecodeValue],
        options: InterpreterOptions,
    ) -> Result<BytecodeRunResult> {
        #[cfg(whale_wave_ir)]
        {
            self.native.run_values(function, arguments, options, self.limits, self.identity)
        }
        #[cfg(not(whale_wave_ir))]
        {
            let _ = (function, arguments, options);
            Err(BytecodeError::Unavailable)
        }
    }

    pub fn function_value(&self, function: FunctionId) -> Result<BytecodeValue> {
        #[cfg(whale_wave_ir)]
        {
            Ok(BytecodeValue {
                value: self.native.runtime.callable_value(function)?,
                origin: self.identity,
            })
        }
        #[cfg(not(whale_wave_ir))]
        {
            let _ = function;
            Err(BytecodeError::Unavailable)
        }
    }
}

pub fn compile(module: &Module, limits: BytecodeLimits) -> Result<Vec<u8>> {
    #[cfg(whale_wave_ir)]
    {
        native::compile(module, limits)
    }
    #[cfg(not(whale_wave_ir))]
    {
        let _ = (module, limits);
        Err(BytecodeError::Unavailable)
    }
}

pub fn verify_bytes(bytes: &[u8], limits: BytecodeLimits) -> Result<ImageInfo> {
    BytecodeProgram::load(bytes, limits)?.verify()
}

pub fn run(
    bytes: &[u8],
    function: FunctionId,
    arguments: &[ConstValue],
    options: InterpreterOptions,
    limits: BytecodeLimits,
) -> Result<BytecodeRunResult> {
    BytecodeProgram::load(bytes, limits)?.run(function, arguments, options)
}

#[cfg(whale_wave_ir)]
mod native {
    use super::*;

    use crate::interpreter::runtime_wave::{
        self, Program, RawBlock, RawFunction, RawGlobal, RawInstruction, RawType,
    };

    use std::sync::atomic::{AtomicU64, Ordering};

    const MISSING: u64 = u64::MAX;

    const BACKEND: &str = "invalid Wave bytecode transport";

    static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);

    macro_rules! wire {
        ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
            #[repr(C)]
            #[derive(Clone, Copy, Default)]
            struct $name { $($field: $ty),* }
        };
    }

    wire!(Site { function: u64, block: u64, instruction: u64, value: u64 });
    wire!(Text { first: u64, count: u64 });
    wire!(File { id: u64, text: u64 });
    wire!(Original {
        id: u64,
        has_span: u64,
        file: u64,
        line: u64,
        column: u64,
        end_line: u64,
        end_column: u64
    });
    wire!(Mapping { kind: u64, function: u64, block: u64, index: u64, original: u64 });
    wire!(Counts {
        types: u64,
        edges: u64,
        functions: u64,
        blocks: u64,
        instructions: u64,
        operands: u64,
        value_types: u64,
        globals: u64,
        sites: u64,
        texts: u64,
        text_bytes: u64,
        files: u64,
        originals: u64,
        mappings: u64,
        value_ids: u64,
        constant_global_ids: u64
    });
    wire!(RawResult {
        status: u64,
        code: u64,
        table: u64,
        index: u64,
        work: u64,
        decoded_bytes: u64,
        scratch_words: u64,
        counts: Counts
    });
    #[repr(C)]
    struct RawProgram {
        types: *mut RawType,
        type_count: u64,
        edges: *mut u64,
        edge_count: u64,
        functions: *mut RawFunction,
        function_count: u64,
        blocks: *mut RawBlock,
        block_count: u64,
        instructions: *mut RawInstruction,
        instruction_count: u64,
        operands: *mut u64,
        operand_count: u64,
        value_types: *mut u64,
        value_type_count: u64,
        globals: *mut RawGlobal,
        global_count: u64,
        sites: *mut Site,
        site_count: u64,
        texts: *mut Text,
        text_count: u64,
        text_bytes: *mut u8,
        text_byte_count: u64,
        files: *mut File,
        file_count: u64,
        originals: *mut Original,
        original_count: u64,
        mappings: *mut Mapping,
        mapping_count: u64,
        value_ids: *mut u64,
        value_id_count: u64,
        constant_global_ids: *mut u64,
        constant_global_count: u64,
    }

    unsafe extern "C" {
        fn whale_ir_bytecode_probe(
            data: *const u8,
            count: u64,
            limits: *const BytecodeLimits,
            result: *mut RawResult,
        ) -> i32;

        fn whale_ir_bytecode_load(
            data: *const u8,
            count: u64,
            program: *mut RawProgram,
            scratch: *mut u64,
            capacity: u64,
            limits: *const BytecodeLimits,
            result: *mut RawResult,
        ) -> i32;

        fn whale_ir_bytecode_verify(
            program: *mut RawProgram,
            scratch: *mut u64,
            capacity: u64,
            limits: *const BytecodeLimits,
            result: *mut RawResult,
        ) -> i32;

        fn whale_ir_bytecode_write(
            program: *mut RawProgram,
            output: *mut u8,
            capacity: u64,
            required: *mut u64,
            scratch: *mut u64,
            scratch_capacity: u64,
            limits: *const BytecodeLimits,
            result: *mut RawResult,
        ) -> i32;
    }

    fn buffer<T: Default>(count: u64) -> Result<Vec<T>> {
        let count = usize::try_from(count).map_err(|_| BytecodeError::Allocation)?;
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| BytecodeError::Allocation)?;
        values.resize_with(count, T::default);
        Ok(values)
    }

    fn check(status: i32, result: &RawResult, limits: BytecodeLimits) -> Result<()> {
        if status < 0
            || status as u64 != result.status
            || result.status > 5
            || result.work > limits.max_work
        {
            return Err(BytecodeError::Backend(BACKEND));
        }
        if status != 0 {
            return Err(BytecodeError::Invalid {
                status: result.status,
                code: result.code,
                table: result.table,
                index: result.index,
            });
        }
        Ok(())
    }

    pub(super) struct Owned<'a> {
        pub(super) runtime: Program<'a>,
        sites: Vec<Site>,
        texts: Vec<Text>,
        text_bytes: Vec<u8>,
        files: Vec<File>,
        originals: Vec<Original>,
        mappings: Vec<Mapping>,
        constant_globals: Vec<u64>,
        scratch: Vec<u64>,
        decoded_bytes: u64,
    }

    impl<'a> Owned<'a> {
        fn empty(counts: Counts) -> Result<Self> {
            let mut runtime = Program::new(None)?;
            runtime.types = buffer(counts.types)?;
            runtime.edges = buffer(counts.edges)?;
            runtime.functions = buffer(counts.functions)?;
            runtime.blocks = buffer(counts.blocks)?;
            runtime.instructions = buffer(counts.instructions)?;
            runtime.operands = buffer(counts.operands)?;
            runtime.value_types = buffer(counts.value_types)?;
            runtime.value_ids = buffer(counts.value_ids)?;
            runtime.globals = buffer(counts.globals)?;
            Ok(Self {
                runtime,
                sites: buffer(counts.sites)?,
                texts: buffer(counts.texts)?,
                text_bytes: buffer(counts.text_bytes)?,
                files: buffer(counts.files)?,
                originals: buffer(counts.originals)?,
                mappings: buffer(counts.mappings)?,
                constant_globals: buffer(counts.constant_global_ids)?,
                scratch: Vec::new(),
                decoded_bytes: 0,
            })
        }

        fn packet(&mut self) -> RawProgram {
            RawProgram {
                types: self.runtime.types.as_mut_ptr(),
                type_count: self.runtime.types.len() as u64,
                edges: self.runtime.edges.as_mut_ptr(),
                edge_count: self.runtime.edges.len() as u64,
                functions: self.runtime.functions.as_mut_ptr(),
                function_count: self.runtime.functions.len() as u64,
                blocks: self.runtime.blocks.as_mut_ptr(),
                block_count: self.runtime.blocks.len() as u64,
                instructions: self.runtime.instructions.as_mut_ptr(),
                instruction_count: self.runtime.instructions.len() as u64,
                operands: self.runtime.operands.as_mut_ptr(),
                operand_count: self.runtime.operands.len() as u64,
                value_types: self.runtime.value_types.as_mut_ptr(),
                value_type_count: self.runtime.value_types.len() as u64,
                globals: self.runtime.globals.as_mut_ptr(),
                global_count: self.runtime.globals.len() as u64,
                sites: self.sites.as_mut_ptr(),
                site_count: self.sites.len() as u64,
                texts: self.texts.as_mut_ptr(),
                text_count: self.texts.len() as u64,
                text_bytes: self.text_bytes.as_mut_ptr(),
                text_byte_count: self.text_bytes.len() as u64,
                files: self.files.as_mut_ptr(),
                file_count: self.files.len() as u64,
                originals: self.originals.as_mut_ptr(),
                original_count: self.originals.len() as u64,
                mappings: self.mappings.as_mut_ptr(),
                mapping_count: self.mappings.len() as u64,
                value_ids: self.runtime.value_ids.as_mut_ptr(),
                value_id_count: self.runtime.value_ids.len() as u64,
                constant_global_ids: self.constant_globals.as_mut_ptr(),
                constant_global_count: self.constant_globals.len() as u64,
            }
        }

        pub(super) fn verify(&mut self, limits: BytecodeLimits) -> Result<()> {
            let mut packet = self.packet();
            let mut result = RawResult::default();
            // Native verification queries exact scratch before inspecting semantic records.
            let status = unsafe {
                whale_ir_bytecode_verify(&mut packet, std::ptr::null_mut(), 0, &limits, &mut result)
            };
            if status != 2 || result.status != 2 {
                return check(status, &result, limits);
            }
            self.scratch = Vec::new();
            self.scratch = buffer(result.scratch_words)?;
            let status = unsafe {
                whale_ir_bytecode_verify(
                    &mut packet,
                    self.scratch.as_mut_ptr(),
                    self.scratch.len() as u64,
                    &limits,
                    &mut result,
                )
            };
            check(status, &result, limits)?;
            self.decoded_bytes = result.decoded_bytes;
            Ok(())
        }

        fn text(&self, index: u64) -> Result<&str> {
            let span = self.texts.get(index as usize).ok_or(BytecodeError::Backend(BACKEND))?;
            let first = usize::try_from(span.first).map_err(|_| BytecodeError::Backend(BACKEND))?;
            let end = first
                .checked_add(
                    usize::try_from(span.count).map_err(|_| BytecodeError::Backend(BACKEND))?,
                )
                .ok_or(BytecodeError::Backend(BACKEND))?;
            std::str::from_utf8(
                self.text_bytes.get(first..end).ok_or(BytecodeError::Backend(BACKEND))?,
            )
            .map_err(|_| BytecodeError::Backend(BACKEND))
        }

        fn materialize(&mut self) -> Result<()> {
            self.runtime.materialize_types()?;
            self.runtime.sites = self
                .sites
                .iter()
                .map(|site| crate::ExecutionSite {
                    function: FunctionId(site.function as u32),
                    block: crate::BlockId(site.block as u32),
                    instruction: site.instruction as usize,
                    value: (site.value != MISSING).then_some(crate::ValueId(site.value as u32)),
                })
                .collect();
            // Text indices also carry trap-reason identity; keep their exact order.
            self.runtime.reasons = (0..self.texts.len())
                .map(|index| {
                    self.text(index as u64).map(|text| std::borrow::Cow::Owned(text.to_owned()))
                })
                .collect::<Result<Vec<_>>>()?;
            self.runtime.sources.files = self
                .files
                .iter()
                .map(|file| {
                    Ok(crate::SourceFile {
                        id: file.id as u32,
                        name: self.text(file.text)?.to_owned(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            self.runtime.sources.originals = self
                .originals
                .iter()
                .map(|raw| crate::OriginalNode {
                    id: raw.id,
                    span: (raw.has_span != 0).then_some(crate::SourceSpan {
                        file: raw.file as u32,
                        line: raw.line as u32,
                        column: raw.column as u32,
                        end_line: raw.end_line as u32,
                        end_column: raw.end_column as u32,
                    }),
                })
                .collect();
            self.runtime.sources.mappings = self
                .mappings
                .iter()
                .map(|raw| {
                    Ok(crate::SourceMapping {
                        original: raw.original,
                        target: match raw.kind {
                            0 => crate::SourceTarget::Instruction {
                                function: FunctionId(raw.function as u32),
                                block: crate::BlockId(raw.block as u32),
                                index: raw.index as usize,
                            },
                            1 => crate::SourceTarget::Terminator {
                                function: FunctionId(raw.function as u32),
                                block: crate::BlockId(raw.block as u32),
                            },
                            2 => crate::SourceTarget::Global(crate::GlobalId(raw.function as u32)),
                            _ => return Err(BytecodeError::Backend(BACKEND)),
                        },
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(())
        }

        pub(super) fn function(&self, id: FunctionId) -> Result<Option<FunctionInfo>> {
            let Some(function) = self.runtime.function_record(id) else {
                return Ok(None);
            };
            let Type::FnPtr(signature) = self.runtime.type_model(function.signature)? else {
                return Err(BytecodeError::Backend(BACKEND));
            };
            Ok(Some(FunctionInfo {
                id,
                params: signature.params,
                ret: signature.ret,
                has_body: function.has_body != 0,
            }))
        }

        pub(super) fn info(&self) -> Result<ImageInfo> {
            Ok(ImageInfo {
                functions: self
                    .runtime
                    .functions
                    .iter()
                    .map(|function| {
                        self.function(FunctionId(function.id as u32))?
                            .ok_or(BytecodeError::Backend(BACKEND))
                    })
                    .collect::<Result<Vec<_>>>()?,
                globals: self.runtime.globals.len(),
                instructions: self.runtime.instructions.len(),
                decoded_bytes: self.decoded_bytes,
            })
        }

        pub(super) fn run(
            &mut self,
            function: FunctionId,
            arguments: &[ConstValue],
            options: InterpreterOptions,
            limits: BytecodeLimits,
            identity: u64,
        ) -> Result<BytecodeRunResult>
        where
            'a: 'static,
        {
            self.verify(limits)?;
            let values = runtime_wave::scalar_arguments(&self.runtime, function, arguments)?;
            let mut packet = self.packet();
            let mut admission_result = RawResult::default();
            let admission = runtime_wave::RuntimeAdmission {
                program: (&mut packet as *mut RawProgram).cast(),
                scratch: self.scratch.as_mut_ptr(),
                capacity: self.scratch.len() as u64,
                limits,
                result: (&mut admission_result as *mut RawResult).cast(),
            };
            let result = runtime_wave::execute(
                &mut self.runtime,
                function,
                values,
                false,
                options,
                Some(admission),
            )?;
            Ok(BytecodeRunResult {
                value: result.value.map(|value| BytecodeValue { value, origin: identity }),
                steps: result.steps,
            })
        }

        pub(super) fn run_values(
            &mut self,
            function: FunctionId,
            arguments: &[BytecodeValue],
            options: InterpreterOptions,
            limits: BytecodeLimits,
            identity: u64,
        ) -> Result<BytecodeRunResult>
        where
            'a: 'static,
        {
            argument_preflight(arguments, limits, self.decoded_bytes, self.scratch.len())?;
            let inputs = arguments
                .iter()
                .map(|value| (&value.value, value.origin == identity))
                .collect::<Vec<_>>();
            let original_types = self.runtime.types.len();
            let original_edges = self.runtime.edges.len();
            let original_decoded = self.decoded_bytes;
            let original_scratch = self.scratch.len();
            let outcome = (|| -> Result<crate::RuntimeInterpreterResult<'static>> {
                let ids = self.runtime.prepare_argument_types(&inputs)?;
                self.verify(limits)?;
                self.runtime.materialize_types()?;
                let values = self.runtime.typed_arguments(&inputs, &ids)?;
                let mut packet = self.packet();
                let mut admission_result = RawResult::default();
                let admission = runtime_wave::RuntimeAdmission {
                    program: (&mut packet as *mut RawProgram).cast(),
                    scratch: self.scratch.as_mut_ptr(),
                    capacity: self.scratch.len() as u64,
                    limits,
                    result: (&mut admission_result as *mut RawResult).cast(),
                };
                runtime_wave::execute(
                    &mut self.runtime,
                    function,
                    values,
                    false,
                    options,
                    Some(admission),
                )
                .map_err(BytecodeError::from)
            })();
            self.runtime.rollback_argument_types(original_types, original_edges);
            self.decoded_bytes = original_decoded;
            self.scratch.truncate(original_scratch);
            let result = outcome?;
            Ok(BytecodeRunResult {
                value: result.value.map(|value| BytecodeValue { value, origin: identity }),
                steps: result.steps,
            })
        }
    }

    pub(super) fn load(bytes: &[u8], limits: BytecodeLimits) -> Result<BytecodeProgram> {
        let mut result = RawResult::default();
        let status = unsafe {
            whale_ir_bytecode_probe(bytes.as_ptr(), bytes.len() as u64, &limits, &mut result)
        };
        check(status, &result, limits)?;
        let mut native = Owned::empty(result.counts)?;
        native.scratch = buffer(result.scratch_words)?;
        let mut packet = native.packet();
        let status = unsafe {
            whale_ir_bytecode_load(
                bytes.as_ptr(),
                bytes.len() as u64,
                &mut packet,
                native.scratch.as_mut_ptr(),
                native.scratch.len() as u64,
                &limits,
                &mut result,
            )
        };
        check(status, &result, limits)?;
        native.decoded_bytes = result.decoded_bytes;
        native.materialize()?;
        let identity = NEXT_IDENTITY
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| BytecodeError::Backend("bytecode program identity exhausted"))?;
        Ok(BytecodeProgram { native, limits, identity })
    }

    fn argument_preflight(
        arguments: &[BytecodeValue],
        limits: BytecodeLimits,
        decoded: u64,
        scratch_words: usize,
    ) -> Result<()> {
        let limited = || BytecodeError::Invalid {
            status: 2,
            code: 4,
            table: 0,
            index: limits.max_workspace_bytes,
        };
        let available = limits
            .max_workspace_bytes
            .checked_sub(decoded)
            .and_then(|n| n.checked_sub((scratch_words as u64).checked_mul(8)?))
            .ok_or_else(limited)?;
        let nodes = (available / 1024).min(limits.max_records).min(limits.max_work / 16);
        let ir_limits = crate::IrLimits {
            max_nodes: usize::try_from(nodes)
                .unwrap_or(usize::MAX)
                .min(crate::IrLimits::default().max_nodes),
            max_type_depth: usize::try_from(limits.max_type_depth)
                .unwrap_or(usize::MAX)
                .min(crate::limits::MAX_IR_NESTING),
            ..Default::default()
        };
        let mut budget = crate::limits::Budget::new(ir_limits).map_err(|_| limited())?;
        budget.nodes(arguments.len()).map_err(|_| limited())?;
        for value in arguments {
            budget.ty(value.ty()).map_err(|_| limited())?;
        }
        Ok(())
    }

    /// Allocation arithmetic only: Wave still admits every semantic record.
    /// Count annotated input nodes before making normalized tables or strings.
    fn transport_preflight(module: &Module, limits: BytecodeLimits) -> Result<crate::IrLimits> {
        let limited = || BytecodeError::Invalid {
            status: 2,
            code: 4,
            table: 0,
            index: limits.max_workspace_bytes,
        };
        if limits.max_input_bytes < 576 || limits.max_workspace_bytes < 2048 || limits.max_work == 0
        {
            return Err(limited());
        }
        // A visited input node can contribute records, hash slots, initialized
        // verification scratch and typed metadata. This conservative allowance
        // includes Vec growth and the preflight worklist itself.
        let nodes = limits
            .max_records
            .min(limits.max_workspace_bytes / 1024)
            .min(limits.max_decoded_bytes / 256)
            .min(limits.max_work / 16);
        let node_limit =
            usize::try_from(nodes).unwrap_or(usize::MAX).min(crate::IrLimits::default().max_nodes);
        let type_depth = usize::try_from(limits.max_type_depth)
            .unwrap_or(usize::MAX)
            .min(crate::limits::MAX_IR_NESTING);
        let ir_limits = crate::IrLimits {
            max_nodes: node_limit,
            max_type_depth: type_depth,
            max_const_depth: type_depth,
            max_input_bytes: usize::try_from(
                limits.max_input_bytes.min(limits.max_workspace_bytes / 2).min(limits.max_work / 8),
            )
            .unwrap_or(usize::MAX),
            max_analysis_work: usize::try_from(limits.max_work).unwrap_or(usize::MAX),
            ..Default::default()
        };
        let visited = crate::limits::check_module_with_usage(module, ir_limits)
            .map_err(|_| limited())? as u64;
        let mut text_bytes = 0_u64;
        let mut serialized_text = 0_u64;
        let mut charge_text = |length: usize, serialized: bool| -> Result<()> {
            text_bytes = text_bytes.checked_add(length as u64).ok_or_else(limited)?;
            if serialized {
                serialized_text = serialized_text.checked_add(length as u64).ok_or_else(limited)?;
                if serialized_text.checked_add(576).is_none_or(|n| n > limits.max_input_bytes) {
                    return Err(limited());
                }
            }
            if text_bytes > limits.max_workspace_bytes / 2 || text_bytes > limits.max_work / 8 {
                return Err(limited());
            }
            Ok(())
        };
        // Names are not serialized, but error construction may retain them.
        for global in &module.globals {
            charge_text(global.name.len(), false)?;
        }
        for global in &module.global_storage {
            charge_text(global.name.len(), false)?;
            if let Some(name) = &global.link_name {
                charge_text(name.len(), false)?;
            }
        }
        for declaration in &module.declarations {
            charge_text(declaration.name.len(), false)?;
            if let Some(name) = &declaration.link_name {
                charge_text(name.len(), false)?;
            }
        }
        for file in &module.sources.files {
            charge_text(file.name.len(), true)?;
        }
        for function in &module.functions {
            charge_text(function.name.len(), false)?;
            for parameter in &function.params {
                charge_text(parameter.name.len(), false)?;
            }
            for block in &function.blocks {
                charge_text(block.name.len(), false)?;
                for instruction in &block.instructions {
                    match instruction {
                        crate::Instruction::TrapIf { reason, .. } => {
                            charge_text(reason.len(), true)?
                        },
                        crate::Instruction::ConstDecl { name, .. } => {
                            charge_text(name.len(), false)?
                        },
                        _ => {},
                    }
                }
                if let Some(crate::Terminator::Trap { reason }) = &block.terminator {
                    charge_text(reason.len(), true)?;
                }
            }
        }
        let allocation_bytes = visited
            .checked_mul(1024)
            .and_then(|n| n.checked_add(2048))
            .and_then(|n| text_bytes.checked_mul(2).and_then(|text| n.checked_add(text)))
            .ok_or_else(limited)?;
        let decoded_bytes = visited
            .checked_mul(256)
            .and_then(|n| n.checked_add(serialized_text))
            .ok_or_else(limited)?;
        let work = visited
            .checked_mul(16)
            .and_then(|n| text_bytes.checked_mul(8).and_then(|text| n.checked_add(text)))
            .ok_or_else(limited)?;
        if allocation_bytes > limits.max_workspace_bytes
            || decoded_bytes > limits.max_decoded_bytes
            || work > limits.max_work
        {
            return Err(limited());
        }
        Ok(ir_limits)
    }

    pub(super) fn compile(module: &Module, limits: BytecodeLimits) -> Result<Vec<u8>> {
        if module.target != "whale-bytecode-v1"
            || module.datalayout != crate::DataLayout::default_64bit_le()
        {
            return Err(BytecodeError::Target);
        }
        let ir_limits = transport_preflight(module, limits)?;
        crate::verify_module_with_limits(module, ir_limits)
            .map_err(|error| InterpreterError::Verification(Box::new(error)))?;
        let mut runtime = Program::new(Some(module))?;
        runtime.marshal()?;
        let mut owned = Owned {
            runtime,
            sites: Vec::new(),
            texts: Vec::new(),
            text_bytes: Vec::new(),
            files: Vec::new(),
            originals: Vec::new(),
            mappings: Vec::new(),
            constant_globals: module.globals.iter().map(|global| u64::from(global.id.0)).collect(),
            scratch: Vec::new(),
            decoded_bytes: 0,
        };
        owned.sites = owned
            .runtime
            .sites
            .iter()
            .map(|site| Site {
                function: u64::from(site.function.0),
                block: u64::from(site.block.0),
                instruction: site.instruction as u64,
                value: site.value.map_or(MISSING, |value| u64::from(value.0)),
            })
            .collect();
        for reason in &owned.runtime.reasons {
            owned
                .texts
                .push(Text { first: owned.text_bytes.len() as u64, count: reason.len() as u64 });
            owned.text_bytes.extend_from_slice(reason.as_bytes());
        }
        for file in &module.sources.files {
            owned.files.push(File { id: u64::from(file.id), text: owned.texts.len() as u64 });
            owned
                .texts
                .push(Text { first: owned.text_bytes.len() as u64, count: file.name.len() as u64 });
            owned.text_bytes.extend_from_slice(file.name.as_bytes());
        }
        owned.originals = module
            .sources
            .originals
            .iter()
            .map(|node| {
                let mut raw = Original { id: node.id, ..Original::default() };
                if let Some(span) = &node.span {
                    raw.has_span = 1;
                    raw.file = span.file.into();
                    raw.line = span.line.into();
                    raw.column = span.column.into();
                    raw.end_line = span.end_line.into();
                    raw.end_column = span.end_column.into();
                }
                raw
            })
            .collect();
        owned.mappings = module
            .sources
            .mappings
            .iter()
            .map(|mapping| {
                let (kind, function, block, index) = match mapping.target {
                    crate::SourceTarget::Instruction { function, block, index } => {
                        (0, function.0 as u64, block.0 as u64, index as u64)
                    },
                    crate::SourceTarget::Terminator { function, block } => {
                        (1, function.0 as u64, block.0 as u64, 0)
                    },
                    crate::SourceTarget::Global(global) => (2, global.0 as u64, 0, 0),
                };
                Mapping { kind, function, block, index, original: mapping.original }
            })
            .collect();
        owned.verify(limits)?;
        let mut packet = owned.packet();
        let mut result = RawResult::default();
        let mut required = 0;
        let status = unsafe {
            whale_ir_bytecode_write(
                &mut packet,
                std::ptr::null_mut(),
                0,
                &mut required,
                owned.scratch.as_mut_ptr(),
                owned.scratch.len() as u64,
                &limits,
                &mut result,
            )
        };
        check(status, &result, limits)?;
        if required > limits.max_input_bytes {
            return Err(BytecodeError::Backend(BACKEND));
        }
        let mut output = buffer(required)?;
        let status = unsafe {
            whale_ir_bytecode_write(
                &mut packet,
                output.as_mut_ptr(),
                output.len() as u64,
                &mut required,
                owned.scratch.as_mut_ptr(),
                owned.scratch.len() as u64,
                &limits,
                &mut result,
            )
        };
        check(status, &result, limits)?;
        if required != output.len() as u64 {
            return Err(BytecodeError::Backend(BACKEND));
        }
        Ok(output)
    }
}
