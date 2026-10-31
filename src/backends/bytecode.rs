//! Explicit built-in bytecode composition; the compiler itself executes in Wave.
use backend_api::{Artifact, Backend, CheckedModule, Requirement, Support};

use diagnostic::Diagnostic;

use target_spec::TargetSpec;

pub(crate) struct Bytecode {
    pub limits: ir::bytecode::BytecodeLimits,
}

impl Backend for Bytecode {
    fn target_id(&self) -> &str {
        "whale-bytecode-v1"
    }

    fn support(&self, _requirement: Requirement<'_>, _target: &TargetSpec) -> Support {
        // Wave performs the complete executable-subset and import check before
        // serializing any bytes, including unreachable instructions.
        Support::Direct
    }

    fn emit(&self, module: &CheckedModule<'_>) -> Result<Vec<Artifact>, Vec<Diagnostic>> {
        ir::bytecode::compile(module.module(), self.limits)
            .map(|bytes| vec![Artifact { name: "program.wbc".into(), bytes }])
            .map_err(|error| {
                vec![Diagnostic::new("bytecode", "compile", "bytecode.compile", error.to_string())]
            })
    }
}
