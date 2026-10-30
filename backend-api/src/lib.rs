// SPDX-License-Identifier: MPL-2.0
//! Statically linked, out-of-tree code generation. This is a Rust API, not a dylib ABI.
use diagnostic::{Context, Diagnostic, ToDiagnostic};

use std::collections::{BTreeMap, BTreeSet};

use target_spec::TargetSpec;

pub enum Requirement<'a> {
    Type(&'a ir::Type),
    Declaration(&'a ir::FunctionDecl),
    Instruction(&'a ir::Instruction),
    Terminator(&'a ir::Terminator),
    Global(&'a ir::Global),
    GlobalStorage(&'a ir::GlobalStorage),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Support {
    Direct,
    Expanded,
    Runtime(String),
    Unsupported(String),
}

pub struct Artifact {
    /// Relative portable path, with components at most 255 UTF-8 bytes and
    /// 255 UTF-16 code units. Paths in one
    /// artifact set must be distinct under ASCII case folding and cannot be
    /// component-wise ancestors of one another. Unicode case folding is not applied.
    /// Validated before returning artifacts to callers.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Only the shared checker can construct this proof of verified, supported input.
pub struct CheckedModule<'a> {
    module: &'a ir::Module,
    target: &'a TargetSpec,
    runtimes: BTreeSet<String>,
}

impl CheckedModule<'_> {
    pub fn module(&self) -> &ir::Module {
        self.module
    }

    pub fn target(&self) -> &TargetSpec {
        self.target
    }

    pub fn runtimes(&self) -> &BTreeSet<String> {
        &self.runtimes
    }
}

pub trait Backend: Send + Sync {
    fn target_id(&self) -> &str;

    fn supports_versions(&self, format: u32, semantics: u32) -> bool {
        format == ir::IR_FORMAT_VERSION && semantics == ir::SEMANTICS_VERSION
    }

    /// Must classify every requirement, including signatures and memory effects.
    fn support(&self, requirement: Requirement<'_>, target: &TargetSpec) -> Support;

    /// May return extra legalization or runtime-availability diagnostics.
    fn validate(&self, _module: &CheckedModule<'_>) -> Result<(), Vec<Diagnostic>> {
        Ok(())
    }

    fn emit(&self, module: &CheckedModule<'_>) -> Result<Vec<Artifact>, Vec<Diagnostic>>;
}

pub fn check<'a>(
    backend: &dyn Backend,
    module: &'a ir::Module,
    target: &'a TargetSpec,
    limits: ir::IrLimits,
) -> Result<CheckedModule<'a>, Vec<Diagnostic>> {
    if backend.target_id() != target.id
        || !backend.supports_versions(ir::IR_FORMAT_VERSION, ir::SEMANTICS_VERSION)
    {
        return Err(vec![Diagnostic::new(
            "backend",
            "check",
            "backend.contract",
            "backend target or IR version mismatch",
        )]);
    }
    ir::verify_module_for_target(module, target, limits).map_err(|e| vec![e.diagnostic()])?;
    let mut errors = Vec::new();
    let mut runtimes = BTreeSet::new();
    let mut inspect =
        |requirement, contexts: Vec<Context>| match backend.support(requirement, target) {
            Support::Direct | Support::Expanded => {},
            Support::Runtime(name) if !name.is_empty() => {
                runtimes.insert(name);
            },
            Support::Runtime(_) => errors.push(Diagnostic::new(
                "backend",
                "check",
                "backend.runtime",
                "runtime identity must be nonempty",
            )),
            Support::Unsupported(reason) => {
                let mut d = Diagnostic::new("backend", "check", "backend.unsupported", reason);
                d.contexts = contexts;
                errors.push(d);
            },
        };
    for global in &module.globals {
        inspect(Requirement::Global(global), vec![]);
        inspect(Requirement::Type(&global.ty), vec![]);
    }
    for global in &module.global_storage {
        inspect(Requirement::GlobalStorage(global), vec![]);
        inspect(Requirement::Type(&global.ty), vec![]);
    }
    for declaration in &module.declarations {
        inspect(Requirement::Declaration(declaration), vec![Context::Function(declaration.id.0)]);
        for ty in
            declaration.signature.params.iter().chain(std::iter::once(&declaration.signature.ret))
        {
            inspect(Requirement::Type(ty), vec![Context::Function(declaration.id.0)]);
        }
    }
    for function in &module.functions {
        let site = vec![Context::Function(function.id.0)];
        inspect(Requirement::Type(&function.ret_ty), site.clone());
        for param in &function.params {
            inspect(Requirement::Type(&param.ty), site.clone());
        }
        for (_, ty) in &function.value_types {
            inspect(Requirement::Type(ty), site.clone());
        }
        for block in &function.blocks {
            for (index, instruction) in block.instructions.iter().enumerate() {
                inspect(
                    Requirement::Instruction(instruction),
                    vec![
                        Context::Function(function.id.0),
                        Context::Block(block.id.0),
                        Context::Instruction(index),
                    ],
                );
            }
            inspect(
                Requirement::Terminator(block.terminator.as_ref().expect("verified terminator")),
                vec![Context::Function(function.id.0), Context::Block(block.id.0)],
            );
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let checked = CheckedModule { module, target, runtimes };
    backend.validate(&checked)?;
    Ok(checked)
}

pub fn compile(
    backend: &dyn Backend,
    module: &ir::Module,
    target: &TargetSpec,
    limits: ir::IrLimits,
) -> Result<Vec<Artifact>, Vec<Diagnostic>> {
    let checked = check(backend, module, target, limits)?;
    let artifacts = backend.emit(&checked)?;
    if artifacts.is_empty() {
        return Err(vec![artifact_error("backend emitted no artifacts")]);
    }
    let mut names = BTreeMap::<String, &str>::new();
    for artifact in &artifacts {
        if !artifact.name.split('/').all(portable_component) {
            return Err(vec![artifact_error(&format!(
                "invalid artifact path {:?}",
                artifact.name
            ))]);
        }
        let folded = artifact.name.to_ascii_lowercase();
        // Prefixes must end at a component boundary: `out` and `out2/a.o`
        // are independent files. Check both orders without comparing every pair.
        let ancestor = folded.match_indices('/').find_map(|(at, _)| names.get(&folded[..at]));
        let prefix = format!("{folded}/");
        let descendant = names
            .range(prefix.clone()..)
            .next()
            .filter(|(name, _)| name.starts_with(&prefix))
            .map(|(_, name)| name);
        if let Some(other) = names.get(&folded).or(ancestor).or(descendant) {
            return Err(vec![artifact_error(&format!(
                "artifact path {:?} conflicts with {:?}",
                artifact.name, other
            ))]);
        }
        names.insert(folded, &artifact.name);
    }
    Ok(artifacts)
}

fn portable_component(part: &str) -> bool {
    if part.is_empty()
        || part.len() > 255
        || part.encode_utf16().count() > 255
        || part == "."
        || part == ".."
        || part.ends_with(['.', ' '])
        || part
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return false;
    }
    // Windows also ignores spaces before the extension when recognizing devices.
    let basename = part.split('.').next().expect("nonempty component").trim_end_matches(' ');
    let reserved = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .iter()
        .any(|name| basename.eq_ignore_ascii_case(name));
    let numbered = basename.get(..3).is_some_and(|prefix| {
        prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT")
    }) && matches!(
        basename.get(3..),
        Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
    );
    !reserved && !numbered
}

fn artifact_error(message: &str) -> Diagnostic {
    Diagnostic::new("backend", "emit", "backend.artifact", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicBool, Ordering};

    struct Probe {
        version_ok: bool,
        mode: Support,
        names: Vec<String>,
        emitted: AtomicBool,
    }

    impl Backend for Probe {
        fn target_id(&self) -> &str {
            "x86_64-whale-linux"
        }

        fn supports_versions(&self, _: u32, _: u32) -> bool {
            self.version_ok
        }

        fn support(&self, _: Requirement<'_>, _: &TargetSpec) -> Support {
            self.mode.clone()
        }

        fn emit(&self, _: &CheckedModule<'_>) -> Result<Vec<Artifact>, Vec<Diagnostic>> {
            self.emitted.store(true, Ordering::SeqCst);
            Ok(self
                .names
                .iter()
                .map(|name| Artifact { name: name.clone(), bytes: vec![42] })
                .collect())
        }
    }

    fn probe(mode: Support) -> Probe {
        Probe {
            version_ok: true,
            mode,
            names: vec!["program.bin".into()],
            emitted: AtomicBool::new(false),
        }
    }

    fn module() -> ir::Module {
        let t = ir::Target::X86_64WhaleLinux;
        let mut builder = ir::ModuleBuilder::new(t.name(), t.data_layout());
        let mut f = builder.begin_function("main", vec![], ir::Type::I32);
        let value = f.const_i32(42);
        f.ret(Some(value));
        f.finish();
        builder.finish()
    }

    #[test]
    fn unsupported_and_version_mismatch_never_reach_emission() {
        let m = module();
        let t = ir::Target::X86_64WhaleLinux.specification();
        let p = probe(Support::Unsupported("no i32".into()));
        assert!(compile(&p, &m, &t, ir::IrLimits::default()).is_err());
        assert!(!p.emitted.load(Ordering::SeqCst));
        let mut p = probe(Support::Direct);
        p.version_ok = false;
        let errors = compile(&p, &m, &t, ir::IrLimits::default()).err().unwrap();
        assert_eq!(errors[0].code, "backend.contract");
        assert!(!p.emitted.load(Ordering::SeqCst));
    }

    #[test]
    fn runtime_requirements_are_deduplicated_and_invalid_ids_fail() {
        let m = module();
        let t = ir::Target::X86_64WhaleLinux.specification();
        let p = probe(Support::Runtime("vendor.int32.v1".into()));
        let checked = check(&p, &m, &t, ir::IrLimits::default()).unwrap();
        assert_eq!(checked.runtimes().iter().collect::<Vec<_>>(), vec!["vendor.int32.v1"]);
        assert!(!p.emitted.load(Ordering::SeqCst));
        let p = probe(Support::Runtime(String::new()));
        assert!(check(&p, &m, &t, ir::IrLimits::default()).is_err());
    }

    #[test]
    fn artifact_names_and_nonempty_output_are_checked() {
        let m = module();
        let t = ir::Target::X86_64WhaleLinux.specification();
        for names in [
            vec![],
            vec!["../escape"],
            vec!["/absolute"],
            vec!["a\\b"],
            vec!["a", "a"],
            vec!["a//b"],
            vec!["C:drive"],
            vec!["bad\nname"],
        ] {
            let mut p = probe(Support::Expanded);
            p.names = names.into_iter().map(str::to_owned).collect();
            assert!(compile(&p, &m, &t, ir::IrLimits::default()).is_err());
        }
        let p = probe(Support::Expanded);
        assert_eq!(compile(&p, &m, &t, ir::IrLimits::default()).unwrap()[0].bytes, [42]);
    }

    fn compile_names(names: &[&str]) -> Result<Vec<Artifact>, Vec<Diagnostic>> {
        let m = module();
        let t = ir::Target::X86_64WhaleLinux.specification();
        let mut p = probe(Support::Direct);
        p.names = names.iter().map(|s| (*s).to_owned()).collect();
        compile(&p, &m, &t, ir::IrLimits::default())
    }

    #[test]
    fn artifact_ancestors_and_ascii_case_collisions_fail_in_either_order() {
        for (a, b) in [
            ("out", "out/module.o"),
            ("a/b", "a/b/c.o"),
            ("OUT.o", "out.o"),
            ("Dir/a.o", "dir/A.o"),
            ("Dir", "dir/module.o"),
        ] {
            for names in [[a, b], [b, a]] {
                let errors = compile_names(&names).err().unwrap();
                assert_eq!(errors[0].code, "backend.artifact");
                assert!(errors[0].message.contains(a), "{}", errors[0]);
                assert!(errors[0].message.contains(b), "{}", errors[0]);
            }
        }
        // A non-ancestor sorting between a path and its child must not hide it.
        assert!(compile_names(&["a", "a-b", "a/b.o"]).is_err());
        assert!(compile_names(&["a/b.o", "a-b", "a"]).is_err());
    }

    #[test]
    fn every_artifact_component_must_be_portable() {
        for part in [
            "CON",
            "con.txt",
            "CON .txt",
            "COM1 .o",
            "LPT9 .obj",
            "NUL .txt",
            "PrN",
            "NUL.txt",
            "AUX",
            "COM1.obj",
            "lpt9",
            "COM¹",
            "lpt².txt",
            "CONIN$",
            "CONOUT$.txt",
            "out?.o",
            "a|b.o",
            "a<b",
            "a>b",
            "a*b",
            "a\"b",
            "a:b",
            "out.o.",
            "out.o ",
            "bad\nname",
            "bad\0name",
            "bad\u{85}name",
        ] {
            for name in [part.to_owned(), format!("valid/{part}/file.o")] {
                let errors = compile_names(&[&name]).err().unwrap();
                assert_eq!(errors[0].code, "backend.artifact", "{name:?}");
            }
        }
        for number in 1..=9 {
            for prefix in ["COM", "lPt"] {
                assert!(compile_names(&[&format!("{prefix}{number}.obj")]).is_err());
            }
        }
    }

    #[test]
    fn valid_artifact_names_keep_their_original_spelling() {
        let names = [
            "out",
            "out2/module.o",
            "Dir/a.o",
            "dir/B.o",
            "한글/λ.o",
            "Ä.o",
            "ä.o",
            "COM0",
            "COM10",
            "LPT0.txt",
            "NULled.o",
            "CON name.txt",
            "COM10 .o",
        ];
        let artifacts = compile_names(&names).unwrap();
        assert_eq!(artifacts.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), names);
    }

    #[test]
    fn artifact_components_obey_portable_encoded_length_limits() {
        let ascii_255 = "x".repeat(255);
        let ascii_256 = "x".repeat(256);
        assert!(compile_names(&[&ascii_255]).is_ok());
        let errors = compile_names(&[&ascii_256]).err().unwrap();
        assert_eq!(errors[0].code, "backend.artifact");
        assert!(compile_names(&[&format!("nested/{ascii_255}/file.o")]).is_ok());
        assert!(compile_names(&[&format!("nested/{ascii_256}/file.o")]).is_err());
        assert!(compile_names(&[&"é".repeat(127)]).is_ok());
        assert!(compile_names(&[&"é".repeat(128)]).is_err());
        assert!(compile_names(&[&"🦀".repeat(63)]).is_ok());
        assert!(compile_names(&[&"🦀".repeat(64)]).is_err());
    }
}
