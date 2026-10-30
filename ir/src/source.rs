// SPDX-License-Identifier: MPL-2.0
//! Optional original-node identities. Metadata never reads the filesystem and
//! does not infer a source location from SSA values, names or canonical text.
use crate::{BlockId, FunctionId, GlobalId, Module};

use diagnostic::EscapedName;

use std::collections::{HashMap, HashSet};

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontendSource {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceAnnotation {
    pub id: u64,
    pub source: Option<FrontendSource>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    pub id: u32,
    pub name: String,
}

/// One-based Unicode-scalar line/column coordinates, with an exclusive end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    pub file: u32,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl SourceSpan {
    pub fn valid(&self) -> bool {
        self.line > 0
            && self.column > 0
            && self.end_line > 0
            && self.end_column > 0
            && (self.line, self.column) <= (self.end_line, self.end_column)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalNode {
    pub id: u64,
    pub span: Option<SourceSpan>,
}

/// Instruction ordinals include side-effect-only operations and unused const
/// declarations. Terminators have an explicit identity separate from ordinals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceTarget {
    Instruction { function: FunctionId, block: BlockId, index: usize },
    Terminator { function: FunctionId, block: BlockId },
    Global(GlobalId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMapping {
    pub target: SourceTarget,
    pub original: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceMetadata {
    pub files: Vec<SourceFile>,
    pub originals: Vec<OriginalNode>,
    pub mappings: Vec<SourceMapping>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMetadataError(pub &'static str);

impl std::fmt::Display for SourceMetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for SourceMetadataError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalLocation {
    pub original: u64,
    pub source: Option<(String, SourceSpan)>,
}

impl std::fmt::Display for OriginalLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some((file, span)) = &self.source {
            write!(f, "{}:{}:{} ", EscapedName(file), span.line, span.column)?;
        }
        write!(f, "original @n{}", self.original)
    }
}

impl SourceMetadata {
    #[cfg(feature = "socket")]
    pub(crate) fn add_frontend_original(&mut self, annotation: &SourceAnnotation) {
        // The AST boundary has rejected duplicate IDs. A retained constant
        // wrapper may be encountered again when its declaration is mapped.
        if self.originals.iter().any(|node| node.id == annotation.id) {
            return;
        }
        let span = annotation.source.as_ref().map(|s| {
            let file = self
                .files
                .iter()
                .find(|file| file.name == s.file)
                .map(|file| file.id)
                .unwrap_or_else(|| {
                    let id = self.files.len() as u32;
                    self.files.push(SourceFile { id, name: s.file.clone() });
                    id
                });
            SourceSpan {
                file,
                line: s.line,
                column: s.column,
                end_line: s.end_line,
                end_column: s.end_column,
            }
        });
        self.originals.push(OriginalNode { id: annotation.id, span });
    }

    pub fn validate(&self, module: &Module) -> Result<(), SourceMetadataError> {
        if self.files.is_empty() && self.originals.is_empty() && self.mappings.is_empty() {
            return Ok(());
        }
        let invalid = |s| SourceMetadataError(s);
        let mut files = HashSet::new();
        for file in &self.files {
            if file.name.is_empty() || file.name.contains('\0') || !files.insert(file.id) {
                return Err(invalid("invalid or duplicate source file"));
            }
        }
        let mut nodes = HashSet::new();
        for node in &self.originals {
            if !nodes.insert(node.id) {
                return Err(invalid("duplicate original node identity"));
            }
            if let Some(span) = &node.span {
                if !files.contains(&span.file) || !span.valid() {
                    return Err(invalid("invalid original source span"));
                }
            }
        }
        let blocks: HashMap<_, _> = module
            .functions
            .iter()
            .flat_map(|f| f.blocks.iter().map(move |b| ((f.id, b.id), b)))
            .collect();
        let globals: HashSet<_> = module
            .globals
            .iter()
            .map(|g| g.id)
            .chain(module.global_storage.iter().map(|g| g.id))
            .collect();
        let mut targets = HashSet::new();
        for mapping in &self.mappings {
            if !nodes.contains(&mapping.original) || !targets.insert(mapping.target) {
                return Err(invalid("unknown original node or duplicate source mapping"));
            }
            let exists = match mapping.target {
                SourceTarget::Instruction { function, block, index } => {
                    blocks.get(&(function, block)).is_some_and(|b| index < b.instructions.len())
                },
                SourceTarget::Terminator { function, block } => {
                    blocks.get(&(function, block)).is_some_and(|b| b.terminator.is_some())
                },
                SourceTarget::Global(id) => globals.contains(&id),
            };
            if !exists {
                return Err(invalid("source mapping targets a missing IR operation"));
            }
        }
        Ok(())
    }

    pub fn original(&self, target: SourceTarget) -> Option<&OriginalNode> {
        let id = self.mappings.iter().find(|m| m.target == target)?.original;
        self.originals.iter().find(|o| o.id == id)
    }

    pub fn location(&self, target: SourceTarget) -> Option<OriginalLocation> {
        let original = self.original(target)?;
        let source = original.span.as_ref().and_then(|span| {
            self.files.iter().find(|f| f.id == span.file).map(|f| (f.name.clone(), span.clone()))
        });
        Some(OriginalLocation { original: original.id, source })
    }

    /// Required lowering/splitting passes propagate the original ID to every
    /// generated operation. The caller validates the destination after editing.
    pub fn copy_mapping(
        &mut self,
        from: SourceTarget,
        to: SourceTarget,
    ) -> Result<(), SourceMetadataError> {
        let id = self
            .mappings
            .iter()
            .find(|m| m.target == from)
            .map(|m| m.original)
            .ok_or(SourceMetadataError("missing source mapping to propagate"))?;
        if self.mappings.iter().any(|m| m.target == to) {
            return Err(SourceMetadataError("destination already has a source mapping"));
        }
        self.mappings.push(SourceMapping { target: to, original: id });
        Ok(())
    }
}
