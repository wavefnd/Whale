// SPDX-License-Identifier: MPL-2.0
//! Bounded borrowed AST admission events. Wave owns feature/schema/source rules.
use super::{
    frontend::*,
    limits::{AstLimitError, AstLimits},
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct State {
    nodes: u64,
    strings: u64,
    features: u64,
    numbers: u64,
    sources: u64,
    work: u64,
}

#[repr(C)]
struct Event {
    tag: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    f: u64,
    bytes: *const u8,
    length: u64,
}

impl Default for Event {
    fn default() -> Self {
        Self { tag: 0, a: 0, b: 0, c: 0, d: 0, e: 0, f: 0, bytes: std::ptr::null(), length: 0 }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Slot {
    id: u64,
    occupied: u64,
}

#[repr(C)]
#[derive(Default)]
struct RawResult {
    status: u64,
}

unsafe extern "C" {
    fn whale_ir_ast_inspect_step(
        state: *mut State,
        event: *const Event,
        max_depth: u64,
        max_nodes: u64,
        max_strings: u64,
        max_source_work: u64,
        slots: *mut Slot,
        capacity: u64,
        result: *mut RawResult,
    ) -> i32;

    fn whale_ir_ast_inspect_rehash(
        state: *mut State,
        old: *const Slot,
        old_capacity: u64,
        slots: *mut Slot,
        capacity: u64,
        max_work: u64,
        result: *mut RawResult,
    ) -> i32;
}

fn result(status: i32, record: &RawResult) -> Result<(), AstLimitError> {
    if i32::try_from(record.status).ok() != Some(status) {
        return Err(AstLimitError::Backend);
    }
    match status {
        0 => Ok(()),
        1 => Err(AstLimitError::Source),
        2 => Err(AstLimitError::Depth),
        3 => Err(AstLimitError::Nodes),
        4 => Err(AstLimitError::Strings),
        7 => Err(AstLimitError::SourceWork),
        _ => Err(AstLimitError::Backend),
    }
}

enum Node<'a> {
    Expr(&'a Expr),
    Stmt(&'a Stmt),
    Type(&'a TypeRef),
    Signature(&'a SignatureRef),
}

struct Inspect<'a> {
    limits: AstLimits,
    state: State,
    slots: Vec<Slot>,
    pending: Vec<(Node<'a>, usize)>,
}

impl<'a> Inspect<'a> {
    fn new(limits: AstLimits) -> Self {
        Self {
            limits,
            state: State { numbers: 1, ..State::default() },
            slots: Vec::new(),
            pending: Vec::new(),
        }
    }

    fn step(&mut self, event: Event) -> Result<(), AstLimitError> {
        let mut record = RawResult::default();
        let before = self.state;
        let status = unsafe {
            whale_ir_ast_inspect_step(
                &mut self.state,
                &event,
                self.limits.max_depth as u64,
                self.limits.max_nodes as u64,
                self.limits.max_string_bytes as u64,
                self.limits.max_source_work as u64,
                self.slots.as_mut_ptr(),
                self.slots.len() as u64,
                &mut record,
            )
        };
        result(status, &record)?;
        if self.state.nodes < before.nodes
            || self.state.nodes > self.limits.max_nodes as u64
            || self.state.strings < before.strings
            || self.state.strings > self.limits.max_string_bytes as u64
            || self.state.features > 7
            || self.state.numbers > 1
            || self.state.sources < before.sources
            || self.state.sources > self.state.nodes
            || self.state.work < before.work
            || self.state.work > self.limits.max_source_work as u64
        {
            return Err(AstLimitError::Backend);
        }
        Ok(())
    }

    fn count(&mut self) -> Result<(), AstLimitError> {
        self.step(Event::default())
    }

    fn push(&mut self, node: Node<'a>, depth: usize) -> Result<(), AstLimitError> {
        self.step(Event { tag: 1, a: depth as u64, ..Event::default() })?;
        self.pending.try_reserve(1).map_err(|_| AstLimitError::Nodes)?;
        self.pending.push((node, depth));
        Ok(())
    }

    fn string(&mut self, value: &str) -> Result<(), AstLimitError> {
        self.step(Event {
            tag: 2,
            bytes: value.as_ptr(),
            length: value.len() as u64,
            ..Event::default()
        })
    }

    fn form(&mut self, form: u64, op: u64) -> Result<(), AstLimitError> {
        self.step(Event { tag: 3, a: form, b: op, ..Event::default() })
    }

    fn number(&mut self, tag: u64, bits: u16, value: &str) -> Result<(), AstLimitError> {
        self.step(Event {
            tag,
            a: bits.into(),
            bytes: value.as_ptr(),
            length: value.len() as u64,
            ..Event::default()
        })
    }

    fn source(&mut self, source: &crate::SourceAnnotation) -> Result<(), AstLimitError> {
        let needed = (self.state.sources as usize)
            .checked_add(1)
            .and_then(|n| n.checked_mul(2))
            .and_then(usize::checked_next_power_of_two)
            .ok_or(AstLimitError::Nodes)?;
        if needed > self.slots.len() {
            if needed > self.limits.max_source_work.saturating_sub(self.state.work as usize) {
                return Err(AstLimitError::SourceWork);
            }
            let mut slots = Vec::new();
            slots.try_reserve_exact(needed).map_err(|_| AstLimitError::Nodes)?;
            slots.resize(needed, Slot::default());
            let mut record = RawResult::default();
            let status = unsafe {
                whale_ir_ast_inspect_rehash(
                    &mut self.state,
                    self.slots.as_ptr(),
                    self.slots.len() as u64,
                    slots.as_mut_ptr(),
                    slots.len() as u64,
                    self.limits.max_source_work as u64,
                    &mut record,
                )
            };
            result(status, &record)?;
            self.slots = slots;
        }
        let mut event = Event { tag: 8, a: source.id, ..Event::default() };
        if let Some(span) = &source.source {
            event.b = 1;
            event.c = span.line.into();
            event.d = span.column.into();
            event.e = span.end_line.into();
            event.f = span.end_column.into();
            event.bytes = span.file.as_ptr();
            event.length = span.file.len() as u64;
        }
        self.step(event)
    }
}

pub(super) fn inspect(program: &Program, limits: AstLimits) -> Result<(u8, bool), AstLimitError> {
    inspect_with_usage(program, limits).map(|(features, numbers, _)| (features, numbers))
}

pub(super) fn inspect_with_usage(
    program: &Program,
    limits: AstLimits,
) -> Result<(u8, bool, usize), AstLimitError> {
    let mut i = Inspect::new(limits);
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
                TypeRef::Int { bits, .. } => i.number(4, *bits, "")?,
                TypeRef::Float { bits } => i.number(5, *bits, "")?,
                TypeRef::Ptr(t) | TypeRef::Array { elem: t, .. } => i.push(Node::Type(t), next)?,
                TypeRef::FnPtr(s) => i.push(Node::Signature(s), next)?,
                TypeRef::Struct { fields } => {
                    i.form(2, 0)?;
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
                    i.form(2, 0)?;
                    i.push(Node::Type(ty), next)?;
                    for f in fields {
                        i.count()?;
                        i.string(&f.name)?;
                        i.push(Node::Expr(&f.value), next)?;
                    }
                },
                Expr::Array { ty, elements } => {
                    i.form(2, 0)?;
                    i.push(Node::Type(ty), next)?;
                    for e in elements {
                        i.push(Node::Expr(e), next)?;
                    }
                },
                Expr::Field { base, layout, name } => {
                    i.form(2, 0)?;
                    i.string(name)?;
                    i.push(Node::Expr(base), next)?;
                    i.push(Node::Type(layout), next)?;
                },
                Expr::Index { base, index } | Expr::PointerOffset { base, index } => {
                    i.form(2, 0)?;
                    i.push(Node::Expr(base), next)?;
                    i.push(Node::Expr(index), next)?;
                },
                Expr::VolatileLoad(value) | Expr::Deref(value) | Expr::AddressOf(value) => {
                    i.form(2, 0)?;
                    i.push(Node::Expr(value), next)?;
                },
                Expr::Located { annotation, expression } => {
                    i.source(annotation)?;
                    i.push(Node::Expr(expression), next)?;
                },
                Expr::Neg(value) => {
                    i.form(2, 0)?;
                    i.push(Node::Expr(value), next)?;
                },
                Expr::Cast { value, ty, .. } => {
                    i.form(2, 0)?;
                    i.push(Node::Expr(value), next)?;
                    i.push(Node::Type(ty), next)?;
                },
                Expr::BitNot(value) => {
                    i.form(4, 0)?;
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
                Expr::Lit(Lit::Int { bits, value, .. }) => i.number(6, *bits, value)?,
                Expr::Lit(Lit::Float { bits, value }) => i.number(7, *bits, value)?,
                Expr::Lit(Lit::Bool(_)) => {},
                Expr::Binary { left, op, right } => {
                    i.form(
                        5,
                        match op {
                            BinOpRef::Add => 0,
                            BinOpRef::Sub => 1,
                            BinOpRef::Mul => 2,
                            BinOpRef::BitAnd => 3,
                            BinOpRef::BitOr => 4,
                            BinOpRef::BitXor => 5,
                            BinOpRef::Div => 6,
                            BinOpRef::Rem => 7,
                            BinOpRef::ShiftLeft => 8,
                            BinOpRef::ShiftRight => 9,
                            BinOpRef::LogicalAnd => 10,
                            BinOpRef::LogicalOr => 11,
                        },
                    )?;
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
                    i.form(2, 0)?;
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
    Ok((i.state.features as u8, i.state.numbers != 0, i.state.work as usize))
}
