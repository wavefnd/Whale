// SPDX-License-Identifier: MPL-2.0
//! Borrowed namespace transport; immutable payload allocations are retained for
//! the lifetime of the Wave-owned module namespace records.
use super::wave::{Builder, Node, ResultRecord, State, Value};

use crate::{
    CallError, FunctionDecl, FunctionId, GlobalId, GlobalStorage, GlobalStorageError,
    GlobalStorageInit, IrLimits, LimitError, Linkage, Resource, Type,
};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Record {
    kind: u64,
    id: u64,
    flags: u64,
    name: *const u8,
    name_count: u64,
    link: *const u8,
    link_count: u64,
    types: *const Node,
    type_count: u64,
    defined: u64,
    align: u64,
    initializer: u64,
    low: u64,
    high: u64,
}

pub(super) struct Payload {
    _name: String,
    _link: Option<String>,
    _types: Vec<Node>,
}

unsafe extern "C" {
    fn whale_ir_builder_function_capacity(
        state: *const State,
        entries: *const Record,
        count: u64,
        id: u64,
        nodes: u64,
        work: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_builder_signature(
        types: *const Node,
        count: u64,
        depth: u64,
        nodes: u64,
        work: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_builder_pointer(
        types: *const Node,
        type_count: u64,
        values: *const Value,
        value_count: u64,
        first_value: u64,
        lhs: u64,
        rhs: u64,
        pair: u64,
        max_work: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_builder_call(
        types: *const Node,
        type_count: u64,
        values: *const Value,
        value_count: u64,
        first_value: u64,
        entries: *const Record,
        entry_count: u64,
        indirect: u64,
        callee: u64,
        arguments: *const u64,
        argument_count: u64,
        check_arguments: u64,
        max_work: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_builder_namespace(
        state: *mut State,
        entries: *const Record,
        entry_count: u64,
        candidate: *const Record,
        mode: u64,
        legacy: u64,
        depth: u64,
        nodes: u64,
        bytes: u64,
        work: u64,
        output: *mut Record,
        capacity: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_builder_definition(
        entries: *mut Record,
        count: u64,
        id: u64,
        parameters: u64,
        mode: u64,
        work: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_builder_function_lookup(
        entries: *const Record,
        count: u64,
        name: *const u8,
        name_count: u64,
        work: u64,
        result: *mut ResultRecord,
    ) -> i32;
}

impl Builder {
    fn namespace_error(&self, status: i32) -> LimitError {
        match status {
            2 => LimitError { resource: Resource::TypeDepth, limit: self.limits.max_type_depth },
            3 => LimitError { resource: Resource::Nodes, limit: self.limits.max_nodes },
            19 => LimitError { resource: Resource::InputBytes, limit: self.limits.max_input_bytes },
            _ => LimitError {
                resource: Resource::AnalysisWork,
                limit: self.limits.max_analysis_work,
            },
        }
    }

    fn signature_nodes(
        &self,
        signature: &crate::FunctionSignature,
        limits: IrLimits,
    ) -> Result<Vec<Node>, CallError> {
        // Only bound the borrowed transport here. Callable legality belongs to
        // Wave's namespace/signature walk and does not use a Rust success path.
        crate::limits::Budget::new(limits)
            .and_then(|mut budget| budget.signature(signature))
            .map_err(CallError::ResourceLimit)?;
        let mut nodes = vec![Node {
            kind: 18,
            depth: 0,
            children: signature.params.len() as u64 + 1,
            auxiliary: u64::from(signature.variadic) * 2
                + u64::from(signature.convention == crate::CallingConvention::SysV64),
        }];
        for ty in std::iter::once(&signature.ret).chain(signature.params.iter()) {
            let mut children = self.flatten(ty).map_err(|error| match error {
                crate::BuilderError::ResourceLimit(error) => CallError::ResourceLimit(error),
                error => CallError::Builder(error),
            })?;
            for node in &mut children {
                node.depth += 1;
            }
            nodes.extend(children);
        }
        Ok(nodes)
    }

    #[allow(clippy::too_many_arguments)]
    fn namespace(
        &mut self,
        kind: u64,
        flags: u64,
        name: &str,
        link: Option<&str>,
        types: Vec<Node>,
        legacy: bool,
        limits: IrLimits,
        align: u64,
        initializer: u64,
        low: u64,
        high: u64,
    ) -> Result<u64, i32> {
        let mut candidate = Record {
            kind,
            id: 0,
            flags,
            name: name.as_ptr(),
            name_count: name.len() as u64,
            link: link.map_or(std::ptr::null(), str::as_ptr),
            link_count: link.map_or(0, str::len) as u64,
            types: types.as_ptr(),
            type_count: types.len() as u64,
            defined: 0,
            align,
            initializer,
            low,
            high,
        };
        let depth =
            self.limits.max_type_depth.min(limits.max_type_depth) as u64 + u64::from(kind == 0);
        let nodes = self.limits.max_nodes.min(limits.max_nodes) as u64;
        let bytes = self.limits.max_input_bytes.min(limits.max_input_bytes) as u64;
        let work = self.limits.max_analysis_work.min(limits.max_analysis_work) as u64;
        let before = self.state;
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_builder_namespace(
                &mut self.state,
                self.namespaces.as_ptr(),
                self.namespaces.len() as u64,
                &candidate,
                0,
                u64::from(legacy),
                depth,
                nodes,
                bytes,
                work,
                std::ptr::null_mut(),
                0,
                &mut result,
            )
        };
        if status != result.status as i32 || self.state != before {
            self.state = before;
            return Err(1);
        }
        if status != 0 {
            return Err(status);
        }
        let admitted_value = result.value;
        let admitted_work = result.work;
        if admitted_work > work.saturating_sub(before.work) {
            return Err(1);
        }
        if result.detail == 1 {
            let status = unsafe {
                whale_ir_builder_namespace(
                    &mut self.state,
                    self.namespaces.as_ptr(),
                    self.namespaces.len() as u64,
                    &candidate,
                    1,
                    u64::from(legacy),
                    depth,
                    nodes,
                    bytes,
                    work,
                    std::ptr::null_mut(),
                    0,
                    &mut result,
                )
            };
            let mut expected = before;
            expected.work = expected.work.checked_add(admitted_work).ok_or(1)?;
            return if status == 0
                && result.status == 0
                && self.state == expected
                && result.value == admitted_value
                && result.work == admitted_work
                && result.detail == 1
            {
                Ok(result.value)
            } else {
                self.state = before;
                Err(1)
            };
        }
        // Every string/type copy and Vec growth follows successful Wave admission.
        let owned = |text: &str| -> Result<String, i32> {
            let mut string = String::new();
            string.try_reserve_exact(text.len()).map_err(|_| 3)?;
            string.push_str(text);
            Ok(string)
        };
        let payload =
            Payload { _name: owned(name)?, _link: link.map(owned).transpose()?, _types: types };
        candidate.name = payload._name.as_ptr();
        candidate.link = payload._link.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());
        candidate.types = payload._types.as_ptr();
        self.namespaces.try_reserve(1).map_err(|_| 3)?;
        self.namespace_payloads.try_reserve(1).map_err(|_| 3)?;
        let count = self.namespaces.len();
        let mut expected = before;
        if kind == 0 {
            expected.next_function = expected.next_function.checked_add(1).ok_or(1)?;
        } else {
            expected.next_global = expected.next_global.checked_add(1).ok_or(1)?;
        }
        expected.namespace_nodes = expected
            .namespace_nodes
            .checked_add(candidate.type_count)
            .and_then(|n| n.checked_add(1))
            .ok_or(1)?;
        expected.namespace_bytes = expected
            .namespace_bytes
            .checked_add(candidate.name_count)
            .and_then(|n| n.checked_add(candidate.link_count))
            .ok_or(1)?;
        expected.work = expected.work.checked_add(admitted_work).ok_or(1)?;
        let mut expected_record = candidate;
        expected_record.id = admitted_value;
        self.namespaces.push(candidate);
        let status = unsafe {
            whale_ir_builder_namespace(
                &mut self.state,
                self.namespaces.as_ptr(),
                count as u64,
                &candidate,
                1,
                u64::from(legacy),
                depth,
                nodes,
                bytes,
                work,
                self.namespaces.as_mut_ptr(),
                self.namespaces.len() as u64,
                &mut result,
            )
        };
        if status != 0
            || result.status != 0
            || result.value != admitted_value
            || result.work != admitted_work
            || self.state != expected
            || self.namespaces[count] != expected_record
        {
            self.namespaces.truncate(count);
            self.state = before;
            return Err(1);
        }
        self.namespace_payloads.push(payload);
        Ok(result.value)
    }

    pub(super) fn signature(&self, signature: &crate::FunctionSignature) -> Result<(), CallError> {
        let nodes = self.signature_nodes(signature, self.limits)?;
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_builder_signature(
                nodes.as_ptr(),
                nodes.len() as u64,
                self.limits.max_type_depth as u64 + 1,
                self.limits.max_nodes as u64,
                self.limits.max_analysis_work.saturating_sub(self.state.work as usize) as u64,
                &mut result,
            )
        };
        match status {
            0 => Ok(()),
            17 => Err(CallError::InvalidSignature(
                "void parameters, malformed types and variadic calls are unsupported".into(),
            )),
            18 => Err(CallError::InvalidSignature(
                "SysV64 aggregate signatures are unsupported".into(),
            )),
            status => Err(CallError::ResourceLimit(self.namespace_error(status))),
        }
    }

    pub(super) fn pointer(
        &self,
        first_value: u32,
        lhs: crate::ValueId,
        rhs: Option<crate::ValueId>,
    ) -> Result<(), crate::BuilderError> {
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_builder_pointer(
                self.types.as_ptr(),
                self.types.len() as u64,
                self.values.as_ptr(),
                self.values.len() as u64,
                first_value.into(),
                lhs.0.into(),
                rhs.map_or(0, |id| id.0).into(),
                u64::from(rhs.is_some()),
                self.limits.max_analysis_work.saturating_sub(self.state.work as usize) as u64,
                &mut result,
            )
        };
        if status == 0 {
            Ok(())
        } else if status == 28 {
            Err(crate::BuilderError::InvalidPointerOperands)
        } else {
            Err(self.error(status, result.detail))
        }
    }

    pub(super) fn declaration(
        &mut self,
        declaration: &FunctionDecl,
        limits: IrLimits,
        legacy: bool,
    ) -> Result<FunctionId, CallError> {
        let types = self.signature_nodes(&declaration.signature, limits)?;
        let flags = u64::from(declaration.linkage == Linkage::External)
            | (u64::from(declaration.link_name.is_some()) << 1);
        self.namespace(
            0,
            flags,
            &declaration.name,
            declaration.link_name.as_deref(),
            types,
            legacy,
            limits,
            0,
            0,
            0,
            0,
        )
        .map(|id| FunctionId(id as u32))
        .map_err(|status| match status {
            12 => CallError::ConflictingDeclaration(declaration.name.clone()),
            11 => CallError::LinkNameCollision(declaration.link_name.clone().unwrap_or_default()),
            16 => CallError::InvalidLinkName(declaration.name.clone()),
            17 => CallError::InvalidSignature(
                "void parameters, malformed types and variadic calls are unsupported".into(),
            ),
            18 => CallError::InvalidSignature("SysV64 aggregate signatures are unsupported".into()),
            2 | 3 | 4 | 19 => CallError::ResourceLimit(self.namespace_error(status)),
            _ => CallError::Builder(self.error(status, 0)),
        })
    }

    pub(super) fn global_constant(
        &mut self,
        name: &str,
        ty: &Type,
    ) -> Result<GlobalId, crate::BuilderError> {
        let types = self.flatten(ty)?;
        self.namespace(1, 0, name, None, types, true, self.limits, 0, 0, 0, 0)
            .map(|id| GlobalId(id as u32))
            .map_err(|status| self.error(status, 0))
    }

    pub(super) fn storage(
        &mut self,
        storage: &GlobalStorage,
    ) -> Result<GlobalId, GlobalStorageError> {
        let types = self.flatten(&storage.ty).map_err(|error| match error {
            crate::BuilderError::ResourceLimit(error) => GlobalStorageError::ResourceLimit(error),
            _ => GlobalStorageError::InvalidType,
        })?;
        let (initializer, low, high) = match &storage.init {
            GlobalStorageInit::Uninitialized => (0, 0, 0),
            GlobalStorageInit::Value(crate::ConstValue::I(value)) => {
                (1, *value as u64, (*value >> 64) as u64)
            },
            GlobalStorageInit::Value(crate::ConstValue::U(value)) => {
                (2, *value as u64, (*value >> 64) as u64)
            },
            GlobalStorageInit::Value(crate::ConstValue::Bool(value)) => (3, u64::from(*value), 0),
            GlobalStorageInit::Value(crate::ConstValue::F(value)) => (0, value.bits(), 0),
        };
        let initializer =
            if let GlobalStorageInit::Value(crate::ConstValue::F(value)) = &storage.init {
                match value {
                    crate::FloatBits::F16(_) => 4,
                    crate::FloatBits::F32(_) => 5,
                    crate::FloatBits::F64(_) => 6,
                }
            } else {
                initializer
            };
        let flags = u64::from(storage.linkage == Linkage::External)
            | (u64::from(storage.link_name.is_some()) << 1)
            | (u64::from(storage.readonly) << 2);
        self.namespace(
            2,
            flags,
            &storage.name,
            storage.link_name.as_deref(),
            types,
            false,
            self.limits,
            storage.align.into(),
            initializer,
            low,
            high,
        )
        .map(|id| GlobalId(id as u32))
        .map_err(|status| match status {
            10 => GlobalStorageError::DuplicateName,
            11 => GlobalStorageError::LinkNameCollision,
            16 => GlobalStorageError::InvalidLinkName,
            25 => GlobalStorageError::InvalidAlignment,
            26 => GlobalStorageError::ReadOnlyUninitialized,
            27 => GlobalStorageError::InvalidInitializer,
            2 | 3 | 4 | 19 => GlobalStorageError::ResourceLimit(self.namespace_error(status)),
            5 => GlobalStorageError::DuplicateIdentity,
            _ => GlobalStorageError::InvalidType,
        })
    }

    pub(super) fn lookup_function(&self, name: &str) -> Result<FunctionId, CallError> {
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_builder_function_lookup(
                self.namespaces.as_ptr(),
                self.namespaces.len() as u64,
                name.as_ptr(),
                name.len() as u64,
                self.limits.max_analysis_work as u64,
                &mut result,
            )
        };
        match status {
            0 => Ok(FunctionId(result.value as u32)),
            13 => Err(CallError::UnknownFunctionName(name.into())),
            status => Err(CallError::ResourceLimit(self.namespace_error(status))),
        }
    }

    pub(super) fn check_call(
        &self,
        first_value: u32,
        callee: &crate::Callee,
        arguments: Option<&[crate::ValueId]>,
        signature: Option<&crate::FunctionSignature>,
        function: &crate::Function,
    ) -> Result<(), CallError> {
        let arguments_count = arguments.map_or(0, <[crate::ValueId]>::len);
        if arguments_count > self.limits.max_analysis_work {
            return Err(CallError::ResourceLimit(self.namespace_error(4)));
        }
        let arguments_raw: Vec<u64> =
            arguments.unwrap_or(&[]).iter().map(|id| id.0.into()).collect();
        let (indirect, callee_id) = match callee {
            crate::Callee::Direct(id) => (0, id.0),
            crate::Callee::Indirect(id) => (1, id.0),
        };
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_builder_call(
                self.types.as_ptr(),
                self.types.len() as u64,
                self.values.as_ptr(),
                self.values.len() as u64,
                first_value.into(),
                self.namespaces.as_ptr(),
                self.namespaces.len() as u64,
                indirect,
                callee_id.into(),
                arguments_raw.as_ptr(),
                arguments_raw.len() as u64,
                u64::from(arguments.is_some()),
                self.limits.max_analysis_work.saturating_sub(self.state.work as usize) as u64,
                &mut result,
            )
        };
        match status {
            0 => Ok(()),
            8 => Err(CallError::UnknownValue(crate::ValueId(result.detail as u32))),
            13 => Err(CallError::UnknownFunction(FunctionId(callee_id))),
            21 => Err(CallError::NotFunctionPointer(crate::ValueId(callee_id))),
            22 => Err(CallError::ArgumentCount {
                expected: result.detail as usize,
                got: arguments_count,
            }),
            23 => {
                let index = result.detail as usize;
                let expected = signature.and_then(|signature| signature.params.get(index));
                let got = arguments
                    .and_then(|args| args.get(index))
                    .and_then(|id| function.value_type(*id));
                match (expected, got) {
                    (Some(expected), Some(got)) => Err(CallError::ArgumentType {
                        index,
                        expected: expected.clone(),
                        got: got.clone(),
                    }),
                    _ => Err(CallError::Builder(crate::BuilderError::InvalidType)),
                }
            },
            17 => Err(CallError::InvalidSignature(
                "void parameters, malformed types and variadic calls are unsupported".into(),
            )),
            18 => Err(CallError::InvalidSignature(
                "SysV64 aggregate signatures are unsupported".into(),
            )),
            status => Err(CallError::ResourceLimit(self.namespace_error(status))),
        }
    }

    pub(super) fn preflight_function(&self, id: FunctionId) -> Result<(), CallError> {
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_builder_function_capacity(
                &self.state,
                self.namespaces.as_ptr(),
                self.namespaces.len() as u64,
                id.0.into(),
                self.limits.max_nodes as u64,
                self.limits.max_analysis_work as u64,
                &mut result,
            )
        };
        match status {
            0 => Ok(()),
            5 => Err(CallError::Builder(crate::BuilderError::IdentityOverflow)),
            13 => Err(CallError::UnknownFunction(id)),
            status => Err(CallError::ResourceLimit(self.namespace_error(status))),
        }
    }

    pub(super) fn definition(
        &mut self,
        id: FunctionId,
        parameters: usize,
        commit: bool,
    ) -> Result<(), CallError> {
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_builder_definition(
                self.namespaces.as_mut_ptr(),
                self.namespaces.len() as u64,
                id.0.into(),
                parameters as u64,
                u64::from(commit),
                self.limits.max_analysis_work as u64,
                &mut result,
            )
        };
        match status {
            0 => Ok(()),
            13 => Err(CallError::UnknownFunction(id)),
            14 => Err(CallError::DuplicateDefinition(id)),
            15 => Err(CallError::DefinitionMismatch(id)),
            17 => Err(CallError::InvalidSignature(
                "void parameters, malformed types and variadic calls are unsupported".into(),
            )),
            18 => Err(CallError::InvalidSignature(
                "SysV64 aggregate signatures are unsupported".into(),
            )),
            status => Err(CallError::ResourceLimit(self.namespace_error(status))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declaration(name: &str, link: Option<&str>, parameter: Type) -> FunctionDecl {
        FunctionDecl {
            id: FunctionId(999),
            name: name.into(),
            signature: crate::FunctionSignature::whale(vec![parameter], Type::Void),
            linkage: if link.is_some() { Linkage::External } else { Linkage::Internal },
            link_name: link.map(str::to_owned),
        }
    }

    #[test]
    fn namespace_duplicates_and_collisions_are_atomic_and_share_global_ids() {
        let mut builder = Builder::new(IrLimits::default());
        let first = declaration("function", Some("export"), Type::I32);
        assert_eq!(builder.declaration(&first, IrLimits::default(), false), Ok(FunctionId(0)));
        assert_eq!(builder.declaration(&first, IrLimits::default(), false), Ok(FunctionId(0)));
        let before = builder.state;
        assert_eq!(
            builder.declaration(
                &declaration("function", Some("export"), Type::U32),
                IrLimits::default(),
                false
            ),
            Err(CallError::ConflictingDeclaration("function".into()))
        );
        assert_eq!(builder.state, before);
        let mut storage = GlobalStorage::internal(
            "data",
            Type::U32,
            GlobalStorageInit::Value(crate::ConstValue::U(9)),
            false,
            4,
        );
        storage.linkage = Linkage::External;
        storage.link_name = Some("export".into());
        assert_eq!(builder.storage(&storage), Err(GlobalStorageError::LinkNameCollision));
        assert_eq!(builder.state, before);
        storage.link_name = Some("data_export".into());
        assert_eq!(builder.storage(&storage), Ok(GlobalId(0)));
        assert_eq!(builder.global_constant("constant", &Type::U32), Ok(GlobalId(1)));
        storage.name = "constant".into();
        storage.link_name = Some("third_export".into());
        let before = builder.state;
        assert_eq!(builder.storage(&storage), Err(GlobalStorageError::DuplicateName));
        assert_eq!(builder.state, before);
        assert_eq!(builder.lookup_function("function"), Ok(FunctionId(0)));
        assert_eq!(builder.namespaces.len(), 3);
    }

    #[test]
    fn wave_callable_and_storage_validation_matches_the_reference_inventory() {
        let types = [
            Type::Void,
            Type::Bool,
            Type::I1,
            Type::U1,
            Type::I8,
            Type::U8,
            Type::I32,
            Type::U32,
            Type::I128,
            Type::U128,
            Type::F16,
            Type::F32,
            Type::F64,
            Type::ptr_to(Type::Void),
            Type::ptr_to(Type::I32),
            Type::Array(Box::new(Type::Void), 1),
            Type::Tuple(vec![Type::Void]),
            Type::Tuple(vec![Type::I32]),
            Type::FnPtr(Box::new(crate::FunctionSignature::whale(vec![Type::Void], Type::Void))),
        ];
        for ty in types {
            let candidate = declaration("function", None, ty.clone());
            let reference =
                crate::function::validate_decl_with_limits(&candidate, IrLimits::default());
            let result = Builder::new(IrLimits::default()).declaration(
                &candidate,
                IrLimits::default(),
                false,
            );
            assert_eq!(result.map(|_| ()), reference, "{ty:?}");
            for init in [
                GlobalStorageInit::Uninitialized,
                GlobalStorageInit::Value(crate::ConstValue::I(-1)),
                GlobalStorageInit::Value(crate::ConstValue::U(1)),
                GlobalStorageInit::Value(crate::ConstValue::Bool(true)),
                GlobalStorageInit::Value(crate::ConstValue::F(crate::FloatBits::F32(0x7fa00001))),
            ] {
                let storage = GlobalStorage::internal("data", ty.clone(), init, false, 4);
                let reference = storage.validate_with_limits(IrLimits::default());
                let result = Builder::new(IrLimits::default()).storage(&storage).map(|_| ());
                assert_eq!(result, reference, "{storage:?}");
            }
        }
    }

    #[test]
    fn definitions_publish_on_commit_and_rejected_names_do_not_copy_payloads() {
        let mut builder = Builder::new(IrLimits::default());
        builder
            .declaration(&declaration("callee", None, Type::I32), IrLimits::default(), false)
            .unwrap();
        let before = builder.state;
        assert_eq!(
            builder.definition(FunctionId(0), 0, false),
            Err(CallError::DefinitionMismatch(FunctionId(0)))
        );
        builder.definition(FunctionId(0), 1, false).unwrap();
        builder.definition(FunctionId(0), 1, false).unwrap();
        assert_eq!(builder.state, before);
        builder.definition(FunctionId(0), 1, true).unwrap();
        assert_eq!(
            builder.definition(FunctionId(0), 1, false),
            Err(CallError::DuplicateDefinition(FunctionId(0)))
        );
        let mut tiny = Builder::new(IrLimits { max_input_bytes: 3, ..IrLimits::default() });
        let before = tiny.state;
        let error = tiny
            .declaration(&declaration("long", None, Type::I32), IrLimits::default(), false)
            .unwrap_err();
        assert!(matches!(
            error,
            CallError::ResourceLimit(LimitError { resource: Resource::InputBytes, .. })
        ));
        assert_eq!(tiny.state, before);
        assert!(tiny.namespaces.is_empty());
        assert!(tiny.namespace_payloads.is_empty());
    }
}
