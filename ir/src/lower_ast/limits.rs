// SPDX-License-Identifier: MPL-2.0
//! Shared, iterative resource inspection for directly constructed frontend ASTs.
use super::frontend::*;

/// Recursive lowering/serde work must stay below this ceiling even with overrides.
/// Thirty-two AST levels also fit serde_json's JSON depth cap, including statement
/// vectors and enum-tag objects that add multiple JSON levels per AST node.
pub const MAX_AST_DEPTH: usize = 32;

/// Bounds are applied before recursive conversion, cloning, or serialization.
#[derive(Clone, Copy, Debug)]
pub struct AstLimits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_string_bytes: usize,
    /// Maximum pretty JSON size; enforced by a bounded serialization writer.
    pub max_output_bytes: usize,
    /// Work units for optional source mapping during O0 construction.
    pub max_source_work: usize,
}

impl Default for AstLimits {
    fn default() -> Self {
        Self {
            max_depth: MAX_AST_DEPTH,
            max_nodes: 100_000,
            max_string_bytes: 8 * 1024 * 1024,
            max_output_bytes: 8 * 1024 * 1024,
            max_source_work: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AstLimitError {
    Source,
    SourceWork,
    Expansion,
    Depth,
    Nodes,
    Strings,
    Output,
    Backend,
}

impl std::fmt::Display for AstLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Source => "invalid source annotation or duplicate original node identity",
            Self::Expansion => "AST storage expansion limit exceeded",
            Self::SourceWork => "AST source mapping work limit exceeded",
            Self::Depth => "AST nesting limit exceeded",
            Self::Nodes => "AST node limit exceeded",
            Self::Strings => "AST string byte limit exceeded",
            Self::Output => "AST JSON exceeds output byte limit",
            Self::Backend => "invalid Wave AST inspection result",
        })
    }
}

impl std::error::Error for AstLimitError {}

#[cfg(any(not(whale_wave_ir), test))]
enum Node<'a> {
    Expr(&'a Expr),
    Stmt(&'a Stmt),
    Type(&'a TypeRef),
    Signature(&'a SignatureRef),
}

#[cfg(any(not(whale_wave_ir), test))]
struct Inspect<'a> {
    limits: AstLimits,
    nodes: usize,
    strings: usize,
    pending: Vec<(Node<'a>, usize)>,
    extended_ops: u8,
    source_ids: std::collections::HashSet<u64>,
    wire_numbers: bool,
}

#[cfg(any(not(whale_wave_ir), test))]
impl<'a> Inspect<'a> {
    fn source(&mut self, annotation: &crate::SourceAnnotation) -> Result<(), AstLimitError> {
        if !self.source_ids.insert(annotation.id) {
            return Err(AstLimitError::Source);
        }
        self.extended_ops |= 2;
        if let Some(s) = &annotation.source {
            self.string(&s.file)?;
            if s.file.is_empty()
                || s.file.contains('\0')
                || s.line == 0
                || s.column == 0
                || s.end_line == 0
                || s.end_column == 0
                || (s.line, s.column) > (s.end_line, s.end_column)
            {
                return Err(AstLimitError::Source);
            }
        }
        Ok(())
    }

    fn count(&mut self) -> Result<(), AstLimitError> {
        self.nodes = self.nodes.checked_add(1).ok_or(AstLimitError::Nodes)?;
        if self.nodes > self.limits.max_nodes {
            return Err(AstLimitError::Nodes);
        }
        Ok(())
    }

    fn string(&mut self, s: &str) -> Result<(), AstLimitError> {
        self.strings = self.strings.checked_add(s.len()).ok_or(AstLimitError::Strings)?;
        if self.strings > self.limits.max_string_bytes {
            return Err(AstLimitError::Strings);
        }
        Ok(())
    }

    fn push(&mut self, node: Node<'a>, depth: usize) -> Result<(), AstLimitError> {
        if depth > self.limits.max_depth.min(MAX_AST_DEPTH) {
            return Err(AstLimitError::Depth);
        }
        self.count()?;
        self.pending.push((node, depth));
        Ok(())
    }
}

/// Inspect an existing AST without recursively visiting it or cloning its data.
pub fn inspect_program(program: &Program, limits: AstLimits) -> Result<(), AstLimitError> {
    inspect_program_features(program, limits).map(|_| ())
}

pub(crate) fn inspect_program_features(
    program: &Program,
    limits: AstLimits,
) -> Result<u8, AstLimitError> {
    inspect_program_details(program, limits).map(|details| details.0)
}

/// JSON Schema widths and numeric spelling are syntax; value ranges and typed
/// expression compatibility remain lowering/verification responsibilities.
pub(crate) fn inspect_program_wire(program: &Program, limits: AstLimits) -> Result<u8, String> {
    let (extended, numbers) =
        inspect_program_details(program, limits).map_err(|e| e.to_string())?;
    if !numbers {
        return Err("AST numeric width or spelling does not match its versioned schema".into());
    }
    Ok(extended)
}

/// Source identity admission work, shared with emitted source-map construction.
#[cfg(whale_wave_ir)]
pub(crate) fn inspect_program_source_work(
    program: &Program,
    limits: AstLimits,
) -> Result<usize, AstLimitError> {
    super::wave_limits::inspect_with_usage(program, limits).map(|(_, _, work)| work)
}

fn inspect_program_details(
    program: &Program,
    limits: AstLimits,
) -> Result<(u8, bool), AstLimitError> {
    #[cfg(whale_wave_ir)]
    {
        super::wave_limits::inspect(program, limits)
    }
    #[cfg(not(whale_wave_ir))]
    {
        inspect_program_reference(program, limits)
    }
}

#[cfg(any(not(whale_wave_ir), test))]
fn inspect_program_reference(
    program: &Program,
    limits: AstLimits,
) -> Result<(u8, bool), AstLimitError> {
    let mut i = Inspect {
        limits,
        nodes: 0,
        strings: 0,
        pending: Vec::new(),
        extended_ops: 0,
        source_ids: std::collections::HashSet::new(),
        wire_numbers: true,
    };
    i.count()?; // program
    for d in &program.declarations {
        i.count()?;
        i.string(&d.name)?;
        if let Some(s) = &d.link_name {
            i.string(s)?;
        }
        i.push(Node::Signature(&d.signature), 1)?;
    }
    for g in &program.globals {
        i.count()?;
        i.string(&g.name)?;
        i.push(Node::Type(&g.ty), 1)?;
        i.push(Node::Expr(&g.init), 1)?;
    }
    for f in &program.functions {
        i.count()?;
        i.string(&f.name)?;
        if let Some(s) = &f.link_name {
            i.string(s)?;
        }
        i.push(Node::Type(&f.return_type), 1)?;
        for p in &f.parameters {
            i.count()?;
            i.string(&p.name)?;
            i.push(Node::Type(&p.ty), 1)?;
        }
        for s in &f.body {
            i.push(Node::Stmt(s), 1)?;
        }
    }
    while let Some((node, depth)) = i.pending.pop() {
        let next = depth + 1;
        match node {
            Node::Signature(s) => {
                i.push(Node::Type(&s.ret), next)?;
                for t in &s.params {
                    i.push(Node::Type(t), next)?;
                }
            },
            Node::Type(t) => match t {
                TypeRef::Int { bits, .. } => i.wire_numbers &= valid_int_width(*bits),
                TypeRef::Float { bits } => i.wire_numbers &= matches!(bits, 16 | 32 | 64),
                TypeRef::Ptr(t) | TypeRef::Array { elem: t, .. } => i.push(Node::Type(t), next)?,
                TypeRef::FnPtr(s) => i.push(Node::Signature(s), next)?,
                TypeRef::Struct { fields } => {
                    i.extended_ops |= 4;
                    for f in fields {
                        i.count()?;
                        i.string(&f.name)?;
                        i.push(Node::Type(&f.ty), next)?;
                    }
                },
                TypeRef::Opaque(s) => i.string(s)?,
                _ => {},
            },
            Node::Expr(e) => match e {
                Expr::Struct { ty, fields } => {
                    i.extended_ops |= 4;
                    i.push(Node::Type(ty), next)?;
                    for f in fields {
                        i.count()?;
                        i.string(&f.name)?;
                        i.push(Node::Expr(&f.value), next)?;
                    }
                },
                Expr::Array { ty, elements } => {
                    i.extended_ops |= 4;
                    i.push(Node::Type(ty), next)?;
                    for e in elements {
                        i.push(Node::Expr(e), next)?;
                    }
                },
                Expr::Field { base, layout, name } => {
                    i.extended_ops |= 4;
                    i.string(name)?;
                    i.push(Node::Expr(base), next)?;
                    i.push(Node::Type(layout), next)?;
                },
                Expr::Index { base, index } | Expr::PointerOffset { base, index } => {
                    i.extended_ops |= 4;
                    i.push(Node::Expr(base), next)?;
                    i.push(Node::Expr(index), next)?;
                },
                Expr::VolatileLoad(value) | Expr::Deref(value) | Expr::AddressOf(value) => {
                    i.extended_ops |= 4;
                    i.push(Node::Expr(value), next)?;
                },
                Expr::Located { annotation, expression } => {
                    i.source(annotation)?;
                    i.push(Node::Expr(expression), next)?;
                },
                Expr::Neg(value) => {
                    i.extended_ops |= 4;
                    i.push(Node::Expr(value), next)?;
                },
                Expr::Cast { value, ty, .. } => {
                    i.extended_ops |= 4;
                    i.push(Node::Expr(value), next)?;
                    i.push(Node::Type(ty), next)?;
                },
                Expr::BitNot(value) => {
                    i.extended_ops |= 1;
                    i.push(Node::Expr(value), next)?;
                },
                Expr::FunctionRef(s) | Expr::Var(s) => i.string(s)?,
                Expr::NullFunction(s) => i.push(Node::Signature(s), next)?,
                Expr::Call { callee, args } => {
                    match callee {
                        CalleeRef::Direct(s) => i.string(s)?,
                        CalleeRef::Indirect(e) => i.push(Node::Expr(e), next)?,
                    }
                    for e in args {
                        i.push(Node::Expr(e), next)?;
                    }
                },
                Expr::Lit(Lit::Int { bits, value, .. }) => {
                    i.string(value)?;
                    let digits = value.strip_prefix('-').unwrap_or(value);
                    i.wire_numbers &= valid_int_width(*bits)
                        && !digits.is_empty()
                        && digits.bytes().all(|b| b.is_ascii_digit());
                },
                Expr::Lit(Lit::Float { bits, value }) => {
                    i.string(value)?;
                    i.wire_numbers &= matches!(bits, 16 | 32 | 64)
                        && value.len() == 2 + usize::from(*bits) / 4
                        && value.starts_with("0x")
                        && value[2..].bytes().all(|b| b.is_ascii_hexdigit());
                },
                Expr::Lit(Lit::Bool(_)) => {},
                Expr::Binary { left, op, right } => {
                    if !matches!(op, BinOpRef::Add | BinOpRef::Sub | BinOpRef::Mul) {
                        i.extended_ops |= 1;
                    }
                    i.push(Node::Expr(left), next)?;
                    i.push(Node::Expr(right), next)?;
                },
                Expr::Cmp { left, right, .. } => {
                    i.push(Node::Expr(left), next)?;
                    i.push(Node::Expr(right), next)?;
                },
            },
            Node::Stmt(s) => match s {
                Stmt::Located { annotation, statement } => {
                    i.source(annotation)?;
                    i.push(Node::Stmt(statement), next)?;
                },
                Stmt::VolatileStore { pointer: place, value } | Stmt::Store { place, value } => {
                    i.extended_ops |= 4;
                    i.push(Node::Expr(place), next)?;
                    i.push(Node::Expr(value), next)?;
                },
                Stmt::Return(e) => {
                    if let Some(e) = e {
                        i.push(Node::Expr(e), next)?;
                    }
                },
                Stmt::ConstDecl { name, ty, init } => {
                    i.string(name)?;
                    i.push(Node::Type(ty), next)?;
                    i.push(Node::Expr(init), next)?;
                },
                Stmt::VarDecl { name, ty, init } => {
                    i.string(name)?;
                    i.push(Node::Type(ty), next)?;
                    if let Some(e) = init {
                        i.push(Node::Expr(e), next)?;
                    }
                },
                Stmt::Assign { name, value } => {
                    i.string(name)?;
                    i.push(Node::Expr(value), next)?;
                },
                Stmt::ExprStmt(e) => i.push(Node::Expr(e), next)?,
                Stmt::If { cond, then_body, else_body } => {
                    i.push(Node::Expr(cond), next)?;
                    for s in then_body.iter().chain(else_body) {
                        i.push(Node::Stmt(s), next)?;
                    }
                },
                Stmt::While { cond, body } => {
                    i.push(Node::Expr(cond), next)?;
                    for s in body {
                        i.push(Node::Stmt(s), next)?;
                    }
                },
                Stmt::Break | Stmt::Continue => {},
            },
        }
    }
    Ok((i.extended_ops, i.wire_numbers))
}

enum Owned {
    Expr(Expr),
    Stmt(Stmt),
    Type(TypeRef),
    Signature(SignatureRef),
}

enum Disposal {
    One(Owned),
    Types(std::vec::IntoIter<TypeRef>),
    Fields(std::vec::IntoIter<StructField>),
    Initializers(std::vec::IntoIter<FieldInit>),
    Exprs(std::vec::IntoIter<Expr>),
    Stmts(std::vec::IntoIter<Stmt>),
}

/// Dispose of an owned rejected tree without its recursively derived destructor.
/// Owning iterators reuse the tree's vectors, keeping extra storage bounded by
/// depth rather than copying all siblings of a wide tree into another allocation.
pub(crate) fn discard_program(program: Program) {
    let mut pending = Vec::new();
    for d in program.declarations {
        discard(Owned::Signature(d.signature), &mut pending);
    }
    for g in program.globals {
        discard(Owned::Type(g.ty), &mut pending);
        discard(Owned::Expr(g.init), &mut pending);
    }
    for f in program.functions {
        discard(Owned::Type(f.return_type), &mut pending);
        for p in f.parameters {
            discard(Owned::Type(p.ty), &mut pending);
        }
        for s in f.body {
            discard(Owned::Stmt(s), &mut pending);
        }
    }
}

fn discard(root: Owned, pending: &mut Vec<Disposal>) {
    pending.push(Disposal::One(root));
    while let Some(work) = pending.pop() {
        let node = match work {
            Disposal::Fields(mut iter) => {
                let Some(f) = iter.next() else { continue };
                pending.push(Disposal::Fields(iter));
                Owned::Type(f.ty)
            },
            Disposal::Initializers(mut iter) => {
                let Some(f) = iter.next() else { continue };
                pending.push(Disposal::Initializers(iter));
                Owned::Expr(f.value)
            },
            Disposal::One(node) => node,
            Disposal::Types(mut iter) => {
                let Some(node) = iter.next() else { continue };
                pending.push(Disposal::Types(iter));
                Owned::Type(node)
            },
            Disposal::Exprs(mut iter) => {
                let Some(node) = iter.next() else { continue };
                pending.push(Disposal::Exprs(iter));
                Owned::Expr(node)
            },
            Disposal::Stmts(mut iter) => {
                let Some(node) = iter.next() else { continue };
                pending.push(Disposal::Stmts(iter));
                Owned::Stmt(node)
            },
        };
        match node {
            Owned::Type(t) => match t {
                TypeRef::Ptr(t) | TypeRef::Array { elem: t, .. } => {
                    pending.push(Disposal::One(Owned::Type(*t)))
                },
                TypeRef::Struct { fields } => pending.push(Disposal::Fields(fields.into_iter())),
                TypeRef::FnPtr(s) => pending.push(Disposal::One(Owned::Signature(*s))),
                _ => {},
            },
            Owned::Signature(s) => {
                pending.push(Disposal::One(Owned::Type(s.ret)));
                pending.push(Disposal::Types(s.params.into_iter()));
            },
            Owned::Expr(e) => match e {
                Expr::Struct { ty, fields } => {
                    pending.push(Disposal::One(Owned::Type(ty)));
                    pending.push(Disposal::Initializers(fields.into_iter()));
                },
                Expr::Array { ty, elements } => {
                    pending.push(Disposal::One(Owned::Type(ty)));
                    pending.push(Disposal::Exprs(elements.into_iter()));
                },
                Expr::Field { base, layout, .. } => {
                    pending.push(Disposal::One(Owned::Type(layout)));
                    pending.push(Disposal::One(Owned::Expr(*base)));
                },
                Expr::Index { base, index } | Expr::PointerOffset { base, index } => {
                    pending.push(Disposal::One(Owned::Expr(*base)));
                    pending.push(Disposal::One(Owned::Expr(*index)));
                },
                Expr::VolatileLoad(value) | Expr::Deref(value) | Expr::AddressOf(value) => {
                    pending.push(Disposal::One(Owned::Expr(*value)))
                },
                Expr::Located { expression, .. } => {
                    pending.push(Disposal::One(Owned::Expr(*expression)))
                },
                Expr::Cast { value, ty, .. } => {
                    pending.push(Disposal::One(Owned::Expr(*value)));
                    pending.push(Disposal::One(Owned::Type(ty)));
                },
                Expr::Neg(value) | Expr::BitNot(value) => {
                    pending.push(Disposal::One(Owned::Expr(*value)))
                },
                Expr::NullFunction(s) => pending.push(Disposal::One(Owned::Signature(s))),
                Expr::Binary { left, right, .. } | Expr::Cmp { left, right, .. } => {
                    pending.push(Disposal::One(Owned::Expr(*right)));
                    pending.push(Disposal::One(Owned::Expr(*left)));
                },
                Expr::Call { callee, args } => {
                    if let CalleeRef::Indirect(e) = callee {
                        pending.push(Disposal::One(Owned::Expr(*e)));
                    }
                    pending.push(Disposal::Exprs(args.into_iter()));
                },
                _ => {},
            },
            Owned::Stmt(s) => match s {
                Stmt::Located { statement, .. } => {
                    pending.push(Disposal::One(Owned::Stmt(*statement)))
                },
                Stmt::VolatileStore { pointer: place, value } | Stmt::Store { place, value } => {
                    pending.push(Disposal::One(Owned::Expr(place)));
                    pending.push(Disposal::One(Owned::Expr(value)));
                },
                Stmt::Return(e) => {
                    if let Some(e) = e {
                        pending.push(Disposal::One(Owned::Expr(e)));
                    }
                },
                Stmt::ConstDecl { ty, init, .. } => {
                    pending.push(Disposal::One(Owned::Type(ty)));
                    pending.push(Disposal::One(Owned::Expr(init)));
                },
                Stmt::VarDecl { ty, init, .. } => {
                    pending.push(Disposal::One(Owned::Type(ty)));
                    if let Some(e) = init {
                        pending.push(Disposal::One(Owned::Expr(e)));
                    }
                },
                Stmt::Assign { value, .. } | Stmt::ExprStmt(value) => {
                    pending.push(Disposal::One(Owned::Expr(value)))
                },
                Stmt::If { cond, then_body, else_body } => {
                    pending.push(Disposal::One(Owned::Expr(cond)));
                    pending.push(Disposal::Stmts(then_body.into_iter()));
                    pending.push(Disposal::Stmts(else_body.into_iter()));
                },
                Stmt::While { cond, body } => {
                    pending.push(Disposal::One(Owned::Expr(cond)));
                    pending.push(Disposal::Stmts(body.into_iter()));
                },
                Stmt::Break | Stmt::Continue => {},
            },
        }
    }
}

#[cfg(any(not(whale_wave_ir), test))]
fn valid_int_width(bits: u16) -> bool {
    matches!(bits, 1 | 8 | 16 | 32 | 64 | 128)
}

#[cfg(all(test, whale_wave_ir))]
mod wave_tests {
    use super::*;

    fn program(expr: Expr) -> Program {
        Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![Function {
                convention: crate::CallingConvention::Whale,
                linkage: crate::Linkage::Internal,
                link_name: None,
                name: "main".into(),
                parameters: vec![],
                return_type: TypeRef::Void,
                body: vec![Stmt::ExprStmt(expr), Stmt::Return(None)],
            }],
        }
    }

    fn compare(program: &Program) {
        for limit in 0..48 {
            for limits in [
                AstLimits { max_nodes: limit, ..Default::default() },
                AstLimits { max_string_bytes: limit, ..Default::default() },
                AstLimits { max_depth: limit, ..Default::default() },
            ] {
                assert_eq!(
                    inspect_program_details(program, limits),
                    inspect_program_reference(program, limits),
                    "limits {limits:?}, program {program:?}"
                );
            }
        }
    }

    #[test]
    fn wave_inspection_preserves_counts_feature_gates_and_numeric_schema() {
        for bits in [0, 1, 8, 16, 32, 64, 128, 256, u16::MAX] {
            for value in [
                "",
                "-",
                "-0",
                "+1",
                "12",
                "-12",
                "000",
                "0x1",
                "١",
                "340282366920938463463374607431768211456",
            ] {
                compare(&program(Expr::Lit(Lit::Int { bits, signed: true, value: value.into() })));
            }
            for value in
                ["", "0x0000", "0x80000000", "0X8000", "0x7ff8000000000000", "0xABCD", "0x12z4"]
            {
                compare(&program(Expr::Lit(Lit::Float { bits, value: value.into() })));
            }
        }
        let lit = || Expr::Lit(Lit::Bool(true));
        for op in [
            BinOpRef::Add,
            BinOpRef::Sub,
            BinOpRef::Mul,
            BinOpRef::BitAnd,
            BinOpRef::BitOr,
            BinOpRef::BitXor,
            BinOpRef::Div,
            BinOpRef::Rem,
            BinOpRef::ShiftLeft,
            BinOpRef::ShiftRight,
            BinOpRef::LogicalAnd,
            BinOpRef::LogicalOr,
        ] {
            compare(&program(Expr::Binary { left: Box::new(lit()), op, right: Box::new(lit()) }));
        }
        compare(&program(Expr::Struct {
            ty: TypeRef::Struct {
                fields: vec![StructField { name: "field".into(), ty: TypeRef::Bool }],
            },
            fields: vec![FieldInit { name: "field".into(), value: lit() }],
        }));
        for defect in 0..8 {
            let mut annotation = crate::SourceAnnotation {
                id: u64::MAX,
                source: Some(crate::FrontendSource {
                    file: "경로/test.wave".into(),
                    line: 3,
                    column: 4,
                    end_line: 4,
                    end_column: 2,
                }),
            };
            match defect {
                1 => annotation.source.as_mut().unwrap().file.clear(),
                2 => annotation.source.as_mut().unwrap().file.push('\0'),
                3 => annotation.source.as_mut().unwrap().line = 0,
                4 => annotation.source.as_mut().unwrap().end_line = 2,
                5 => {
                    let s = annotation.source.as_mut().unwrap();
                    s.end_line = 3;
                    s.end_column = 3;
                },
                6 => annotation.source = None,
                _ => {},
            }
            let expression =
                Expr::Located { annotation: annotation.clone(), expression: Box::new(lit()) };
            let mut p = program(expression.clone());
            if defect == 7 {
                p.functions[0].body.push(Stmt::ExprStmt(expression));
            }
            compare(&p);
        }
    }

    #[test]
    fn source_admission_has_a_separate_shared_work_ceiling() {
        let p = program(Expr::Located {
            annotation: crate::SourceAnnotation { id: 1, source: None },
            expression: Box::new(Expr::Lit(Lit::Bool(true))),
        });
        assert_eq!(
            inspect_program(&p, AstLimits { max_source_work: 0, ..Default::default() }),
            Err(AstLimitError::SourceWork)
        );
        assert_eq!(
            inspect_program(
                &program(Expr::Lit(Lit::Bool(true))),
                AstLimits { max_source_work: 0, ..Default::default() }
            ),
            Ok(())
        );
    }
}
