// SPDX-License-Identifier: MPL-2.0
//! Explicit composition root. No built-in ISA, global state, or dynamic loader.
use assembler::extension::Assembler;

use backend_api::{Artifact, Backend};

use diagnostic::Diagnostic;

use std::collections::{btree_map::Entry, BTreeMap};

use target_spec::TargetSpec;

#[derive(Default)]
pub struct Registry {
    targets: BTreeMap<String, TargetSpec>,
    assemblers: BTreeMap<String, Box<dyn Assembler>>,
    backends: BTreeMap<String, Box<dyn Backend>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_target(&mut self, spec: TargetSpec) -> Result<(), String> {
        spec.validate()?;
        match self.targets.entry(spec.id.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(spec);
                Ok(())
            },
            Entry::Occupied(_) => Err("duplicate target identity".into()),
        }
    }

    pub fn register_assembler(
        &mut self,
        assembler: impl Assembler + 'static,
    ) -> Result<(), String> {
        let id = assembler.id();
        if id.is_empty() || id.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err("invalid assembler identity".into());
        }
        match self.assemblers.entry(id.into()) {
            Entry::Vacant(entry) => {
                entry.insert(Box::new(assembler));
                Ok(())
            },
            Entry::Occupied(_) => Err("duplicate assembler identity".into()),
        }
    }

    pub fn register_backend(&mut self, backend: impl Backend + 'static) -> Result<(), String> {
        let id = backend.target_id();
        if !self.targets.contains_key(id) {
            return Err("backend target is not registered".into());
        }
        match self.backends.entry(id.into()) {
            Entry::Vacant(entry) => {
                entry.insert(Box::new(backend));
                Ok(())
            },
            Entry::Occupied(_) => Err("duplicate backend target".into()),
        }
    }

    pub fn target(&self, id: &str) -> Option<&TargetSpec> {
        self.targets.get(id)
    }

    pub fn target_ids(&self) -> impl Iterator<Item = &str> {
        self.targets.keys().map(String::as_str)
    }

    pub fn assembler_ids(&self) -> impl Iterator<Item = &str> {
        self.assemblers.keys().map(String::as_str)
    }

    pub fn assembler(&self, id: &str) -> Option<&dyn Assembler> {
        self.assemblers.get(id).map(Box::as_ref)
    }

    pub fn backend(&self, id: &str) -> Option<&dyn Backend> {
        self.backends.get(id).map(Box::as_ref)
    }

    pub fn assemble(
        &self,
        id: &str,
        name: &str,
        source: &str,
        limits: &assembler::AssemblyLimits,
    ) -> Result<assembler::AssemblerOutput, assembler::AsmError> {
        let implementation = self.assembler(id).ok_or_else(|| {
            assembler::AsmError::EncodeError(format!("unregistered assembler {id:?}"))
        })?;
        let output = implementation.assemble(name, source, limits)?;
        let size = output.sections.iter().try_fold(0usize, |n, s| n.checked_add(s.data.len()));
        if size.is_none_or(|n| n > limits.max_output_bytes) {
            return Err(assembler::AsmError::EncodeError("Assembly output limit exceeded".into()));
        }
        Ok(output)
    }

    pub fn compile(
        &self,
        module: &ir::Module,
        limits: ir::IrLimits,
    ) -> Result<Vec<Artifact>, Vec<Diagnostic>> {
        let missing = || {
            vec![Diagnostic::new(
                "toolchain",
                "select",
                "toolchain.target",
                format!("no registered backend for {:?}", module.target),
            )]
        };
        let target = self.target(&module.target).ok_or_else(missing)?;
        let backend = self.backend(&module.target).ok_or_else(missing)?;
        backend_api::compile(backend, module, target, limits)
    }
}
