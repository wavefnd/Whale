use super::{
    archive,
    executable::write_executable_checked,
    selection::{select, LinkInput},
};

use crate::diagnostic::{LinkError, LinkErrorKind};

use object::{ObjectFile, ObjectReadLimits};

pub struct Linker {
    inputs: Vec<LinkInput>,
    output_format: OutputFormat,
    entry: String,
    read_budget: archive::ReadBudget,
}

pub enum OutputFormat {
    ELF64Executable,
}

impl Linker {
    pub fn new(format: OutputFormat) -> Self {
        Self::with_read_limits(format, ObjectReadLimits::default())
    }

    /// Reader limits are cumulative across every byte-loaded object and archive.
    /// Already-owned objects passed to `add_object` remain caller allocations.
    pub fn with_read_limits(format: OutputFormat, limits: ObjectReadLimits) -> Self {
        Self {
            inputs: Vec::new(),
            output_format: format,
            entry: "_start".into(),
            read_budget: archive::ReadBudget::new(limits),
        }
    }

    pub fn add_object(&mut self, object: ObjectFile) {
        self.inputs.push(LinkInput::Object(object));
    }

    pub fn add_object_bytes(&mut self, bytes: &[u8]) -> Result<(), LinkError> {
        let index = self.inputs.len();
        let object = self.read_budget.object(bytes).map_err(|mut error| {
            error.contexts.push(diagnostic::Context::Input(index));
            error
        })?;
        self.inputs.push(LinkInput::Object(object));
        Ok(())
    }

    pub fn add_archive(&mut self, bytes: &[u8]) -> Result<(), LinkError> {
        let index = self.inputs.len();
        let members = self.read_budget.archive(bytes).map_err(|mut error| {
            error.contexts.push(diagnostic::Context::Input(index));
            error
        })?;
        self.inputs.push(LinkInput::Archive(members));
        Ok(())
    }

    pub fn set_entry(&mut self, entry: impl Into<String>) {
        self.entry = entry.into();
    }

    pub fn link(&mut self) -> Result<Vec<u8>, String> {
        self.link_checked().map_err(|e| e.to_string())
    }

    pub fn link_checked(&mut self) -> Result<Vec<u8>, LinkError> {
        self.link_with_limit(256 * 1024 * 1024)
    }

    pub fn link_with_limit(&mut self, max_bytes: u64) -> Result<Vec<u8>, LinkError> {
        for (index, input) in self.inputs.iter().enumerate() {
            match input {
                LinkInput::Object(o) => o
                    .target
                    .validate_amd64()
                    .map_err(|e| {
                        object::ObjectError::new(
                            "validate",
                            object::ObjectErrorKind::InvalidTarget,
                            e.to_string(),
                        )
                    })
                    .and_then(|_| o.validate_diagnostic())
                    .map_err(|e| LinkError {
                        kind: LinkErrorKind::Input,
                        message: format!("link input {index}: {e}"),
                        contexts: {
                            let mut contexts = e.contexts.clone();
                            contexts.push(diagnostic::Context::Input(index));
                            contexts
                        },
                        input_cause: Some(Box::new(e)),
                        relocation_cause: None,
                    })?,
                LinkInput::Archive(members) => {
                    for m in members {
                        m.object.validate().map_err(|e| {
                            LinkError::new(
                                LinkErrorKind::Input,
                                format!("member {:?}: {e}", m.name),
                            )
                        })?;
                    }
                },
            }
        }

        let objects = select(&self.inputs, &self.entry, max_bytes)
            .map_err(|e| LinkError::new(LinkErrorKind::Selection, e))?;

        // Resolve first to preserve the public unresolved-reference diagnostic boundary.
        let mut symbols = super::symbol_table::SymbolTable::new();
        symbols.resolve(&objects).map_err(|e| LinkError::new(LinkErrorKind::Symbols, e))?;

        match self.output_format {
            OutputFormat::ELF64Executable => {
                write_executable_checked(&objects, &self.entry, max_bytes)
            },
        }
    }
}
