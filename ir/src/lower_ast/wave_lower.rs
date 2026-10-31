// SPDX-License-Identifier: MPL-2.0
//! Borrowed frontend transport and checked materialization of Wave's O0 model.
//! AST/control-flow/type decisions belong to lower_*.wave. This bridge copies
//! shape-only input records and reads a completed, caller-owned typed result.
use super::{frontend as ast, AstLimitError, AstLimits, LowerError};

use crate::*;

use std::collections::HashMap;

const NONE: u64 = u64::MAX;

fn backend() -> LowerError {
    LowerError::Backend("AST lowering transport")
}

fn expansion() -> LowerError {
    LowerError::ResourceLimit(AstLimitError::Expansion)
}

fn zeroed<T: Default + Clone>(count: usize) -> Result<Vec<T>, LowerError> {
    let mut out = Vec::new();
    out.try_reserve_exact(count).map_err(|_| expansion())?;
    out.resize(count, T::default());
    Ok(out)
}

fn filled(count: usize, value: u64) -> Result<Vec<u64>, LowerError> {
    let mut out = zeroed(count)?;
    out.fill(value);
    Ok(out)
}

// Raw pointers do not implement Default on the supported Rust 1.86 MSRV.
// Restrict transport defaults to the actual scalar and pointer field kinds.
trait TransportDefault {
    fn transport_default() -> Self;
}

impl TransportDefault for u64 {
    fn transport_default() -> Self {
        0
    }
}

impl<T> TransportDefault for *const T {
    fn transport_default() -> Self {
        std::ptr::null()
    }
}

impl<T> TransportDefault for *mut T {
    fn transport_default() -> Self {
        std::ptr::null_mut()
    }
}

macro_rules! record { ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
    #[repr(C)] #[derive(Clone, Copy)] struct $name { $($field: $ty),* }
    impl Default for $name {
        fn default() -> Self { Self { $($field: <$ty as TransportDefault>::transport_default()),* } }
    }
} }

record!(Node {
    tag: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    f: u64,
    g: u64,
    h: u64,
    i: u64,
    j: u64,
    k: u64
});
record!(RawString {data:*const u8,length:u64});
record!(RawType { kind: u64, child: u64, first: u64, count: u64, auxiliary: u64 });
record!(Binding { name: u64, ty: u64, value: u64, kind: u64, constant: u64 });
record!(ResultRecord {
    status: u64,
    node: u64,
    expected: u64,
    actual: u64,
    annotation: u64,
    detail: u64,
    work: u64
});
record!(Storage {
    ptr_bits: u64,
    pointer_align: u64,
    scalar_align1: u64,
    scalar_align2: u64,
    scalar_align4: u64,
    scalar_align8: u64,
    scalar_align16: u64,
    large_array_threshold: u64,
    large_array_align: u64,
    has_large_array: u64
});
record!(LayoutNode { kind: u64, child: u64, first: u64, count: u64 });
record!(Layout { size: u64, align: u64, stride: u64, allocation_align: u64, depth: u64 });
record!(Context {
 nodes:*const Node,node_count:u64,edges:*const u64,edge_count:u64,names:*const RawString,name_count:u64,
 types:*mut RawType,type_count:u64,type_capacity:u64,type_edges:*mut u64,type_edge_count:u64,type_edge_capacity:u64,
 type_table:*mut u64,type_table_capacity:u64,type_map:*mut u64,temporary:*mut u64,temporary_count:u64,temporary_capacity:u64,
 name_marks:*mut u64,marker:u64,storage:*const Storage,layout_nodes:*mut LayoutNode,layouts:*mut Layout,layout_offsets:*mut u64,
 annotation:u64,max_depth:u64,max_work:u64,work:u64,result:*mut ResultRecord
});
record!(RawInstruction {
    tag: u64,
    dst: u64,
    ty: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    first: u64,
    count: u64,
    next: u64,
    annotation: u64,
    source: u64
});
record!(RawBlock {
    id: u64,
    function: u64,
    name: u64,
    first: u64,
    last: u64,
    allocation_last: u64,
    instruction_count: u64,
    term_kind: u64,
    term_a: u64,
    term_b: u64,
    term_c: u64,
    term_ty: u64,
    term_annotation: u64,
    term_source: u64
});
record!(RawConstant {
    kind: u64,
    operation: u64,
    ty: u64,
    low: u64,
    high: u64,
    left: u64,
    right: u64,
    reference: u64,
    annotation: u64
});
record!(RawDeclaration {
    name: u64,
    signature: u64,
    linkage: u64,
    link_name: u64,
    defined: u64,
    source: u64
});
record!(RawFunction {
    declaration: u64,
    first_param: u64,
    param_count: u64,
    first_block: u64,
    block_count: u64,
    entry: u64,
    source: u64
});
record!(RawGlobal {
    name: u64,
    ty: u64,
    expression: u64,
    align: u64,
    annotation: u64,
    source: u64
});
record!(FieldBinding { name: u64, ordinal: u64, previous: u64, used: u64 });
record!(RawLoop { condition: u64, exit: u64 });
record!(Annotation {
    original: u64,
    has_source: u64,
    file: u64,
    line: u64,
    column: u64,
    end_line: u64,
    end_column: u64
});
record!(OriginalSlot { id: u64, occupied: u64 });
record!(EvalRequest { expression: u64, kind: u64, identity: u64, annotation: u64 });
record!(Driver {
 core:*mut Context,mode:u64,expansion:u64,max_expansion:u64,
 instructions:*mut RawInstruction,instruction_count:u64,instruction_capacity:u64,
 output_edges:*mut u64,output_edge_count:u64,output_edge_capacity:u64,
 blocks:*mut RawBlock,block_count:u64,block_capacity:u64,constants:*mut RawConstant,
 declarations:*mut RawDeclaration,declaration_count:u64,declaration_capacity:u64,
 functions:*mut RawFunction,function_count:u64,function_capacity:u64,
 globals:*mut RawGlobal,global_count:u64,global_capacity:u64,
 global_bindings:*mut Binding,global_binding_count:u64,bindings:*mut Binding,binding_count:u64,binding_capacity:u64,
 loops:*mut RawLoop,loop_count:u64,loop_capacity:u64,annotations:*const Annotation,annotation_count:u64,
 original_table:*mut OriginalSlot,original_capacity:u64,
 source_order:*mut u64,source_count:u64,source_capacity:u64,source_marks:*mut u64,
 type_sizes:*mut u64,value_types:*mut u64,value_capacity:u64,next_value:u64,declaration_map:*mut u64,
 field_slots:*mut u64,field_bindings:*mut FieldBinding,field_binding_count:u64,field_binding_capacity:u64,
 current_function:u64,current_block:u64,entry:u64,return_type:u64,local_slots:*mut u64,global_slots:*mut u64,
 mapping_order:*mut u64,mapping_count:u64,mapping_capacity:u64,source_work:u64,max_source_work:u64,error_index:u64,requests:*mut EvalRequest,request_count:u64,request_capacity:u64
});
unsafe extern "C" {
    fn whale_ir_lower_o0(driver: *mut Driver, root: u64) -> i32;
}

struct Wire<'a> {
    nodes: Vec<Node>,
    edges: Vec<u64>,
    names: Vec<&'a str>,
    name_ids: HashMap<&'a str, u64>,
    annotations: Vec<&'a SourceAnnotation>,
    raw_annotations: Vec<Annotation>,
    literals: HashMap<u64, &'a ast::Lit>,
}

impl<'a> Wire<'a> {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            names: Vec::new(),
            name_ids: HashMap::new(),
            annotations: Vec::new(),
            raw_annotations: Vec::new(),
            literals: HashMap::new(),
        }
    }

    fn push(&mut self, node: Node) -> Result<u64, LowerError> {
        self.nodes.try_reserve(1).map_err(|_| expansion())?;
        let id = self.nodes.len() as u64;
        self.nodes.push(node);
        Ok(id)
    }

    fn list(&mut self, items: Vec<u64>) -> Result<(u64, u64), LowerError> {
        self.edges.try_reserve(items.len()).map_err(|_| expansion())?;
        let first = self.edges.len() as u64;
        let count = items.len() as u64;
        self.edges.extend(items);
        Ok((first, count))
    }

    fn name(&mut self, name: &'a str) -> Result<u64, LowerError> {
        if let Some(id) = self.name_ids.get(name) {
            return Ok(*id);
        }
        self.names.try_reserve(1).map_err(|_| expansion())?;
        self.name_ids.try_reserve(1).map_err(|_| expansion())?;
        let id = self.names.len() as u64;
        self.names.push(name);
        self.name_ids.insert(name, id);
        Ok(id)
    }

    fn annotation(&mut self, a: &'a SourceAnnotation) -> Result<u64, LowerError> {
        let mut raw = Annotation { original: a.id, ..Annotation::default() };
        if let Some(s) = &a.source {
            raw.has_source = 1;
            raw.file = self.name(&s.file)?;
            raw.line = s.line.into();
            raw.column = s.column.into();
            raw.end_line = s.end_line.into();
            raw.end_column = s.end_column.into();
        }
        self.annotations.try_reserve(1).map_err(|_| expansion())?;
        self.raw_annotations.try_reserve(1).map_err(|_| expansion())?;
        let id = self.annotations.len() as u64;
        self.annotations.push(a);
        self.raw_annotations.push(raw);
        Ok(id)
    }

    fn ty(&mut self, ty: &'a ast::TypeRef) -> Result<u64, LowerError> {
        use ast::TypeRef::*;
        let mut n = Node::default();
        match ty {
            Void => n.tag = 100,
            Bool => n.tag = 101,
            Int { bits, signed } => {
                n.tag = 102;
                n.a = (*bits).into();
                n.b = u64::from(*signed)
            },
            Float { bits } => {
                n.tag = 103;
                n.a = (*bits).into()
            },
            Ptr(child) => {
                n.tag = 104;
                n.a = self.ty(child)?
            },
            FnPtr(sig) => {
                n.tag = 105;
                n.a = self.signature(sig)?
            },
            Array { elem, len } => {
                n.tag = 106;
                n.a = self.ty(elem)?;
                n.b = *len
            },
            Struct { fields } => {
                n.tag = 107;
                let mut ids = Vec::new();
                ids.try_reserve(fields.len()).map_err(|_| expansion())?;
                for f in fields {
                    let a = self.name(&f.name)?;
                    let b = self.ty(&f.ty)?;
                    ids.push(self.push(Node { tag: 108, a, b, ..Node::default() })?)
                }
                (n.a, n.b) = self.list(ids)?
            },
            Opaque(name) => {
                n.tag = 109;
                n.a = self.name(name)?
            },
        }
        self.push(n)
    }

    fn signature(&mut self, sig: &'a ast::SignatureRef) -> Result<u64, LowerError> {
        let mut ids = Vec::new();
        ids.try_reserve(sig.params.len()).map_err(|_| expansion())?;
        for t in &sig.params {
            ids.push(self.ty(t)?)
        }
        let (a, b) = self.list(ids)?;
        let c = self.ty(&sig.ret)?;
        self.push(Node {
            tag: 110,
            a,
            b,
            c,
            d: convention(sig.convention),
            e: u64::from(sig.variadic),
            ..Node::default()
        })
    }

    fn exprs(&mut self, values: &'a [ast::Expr]) -> Result<(u64, u64), LowerError> {
        let mut ids = Vec::new();
        ids.try_reserve(values.len()).map_err(|_| expansion())?;
        for e in values {
            ids.push(self.expr(e)?)
        }
        self.list(ids)
    }

    fn stmts(&mut self, values: &'a [ast::Stmt]) -> Result<(u64, u64), LowerError> {
        let mut ids = Vec::new();
        ids.try_reserve(values.len()).map_err(|_| expansion())?;
        for s in values {
            ids.push(self.stmt(s)?)
        }
        self.list(ids)
    }

    fn expr(&mut self, e: &'a ast::Expr) -> Result<u64, LowerError> {
        use ast::Expr::*;
        let mut n = Node::default();
        match e {
            Located { annotation, expression } => {
                n.tag = 0;
                n.a = self.annotation(annotation)?;
                n.b = self.expr(expression)?
            },
            Struct { ty, fields } => {
                n.tag = 1;
                n.a = self.ty(ty)?;
                let mut ids = Vec::new();
                ids.try_reserve(fields.len()).map_err(|_| expansion())?;
                for f in fields {
                    let a = self.name(&f.name)?;
                    let b = self.expr(&f.value)?;
                    ids.push(self.push(Node { tag: 24, a, b, ..Node::default() })?)
                }
                (n.b, n.c) = self.list(ids)?
            },
            Array { ty, elements } => {
                n.tag = 2;
                n.a = self.ty(ty)?;
                (n.b, n.c) = self.exprs(elements)?
            },
            Field { base, layout, name } => {
                n.tag = 3;
                n.a = self.expr(base)?;
                n.b = self.ty(layout)?;
                n.c = self.name(name)?
            },
            Index { base, index } => {
                n.tag = 4;
                n.a = self.expr(base)?;
                n.b = self.expr(index)?
            },
            AddressOf(value) => {
                n.tag = 5;
                n.a = self.expr(value)?
            },
            Deref(value) => {
                n.tag = 6;
                n.a = self.expr(value)?
            },
            VolatileLoad(value) => {
                n.tag = 7;
                n.a = self.expr(value)?
            },
            PointerOffset { base, index } => {
                n.tag = 8;
                n.a = self.expr(base)?;
                n.b = self.expr(index)?
            },
            BitNot(value) => {
                n.tag = 9;
                n.a = self.expr(value)?
            },
            Neg(value) => {
                n.tag = 10;
                n.a = self.expr(value)?
            },
            Cast { value, ty, kind } => {
                n.tag = 11;
                n.a = self.expr(value)?;
                n.b = self.ty(ty)?;
                n.c = match kind {
                    ast::IntegerCast::Numeric => 0,
                    ast::IntegerCast::Checked => 1,
                }
            },
            FunctionRef(name) => {
                n.tag = 12;
                n.a = self.name(name)?
            },
            NullFunction(sig) => {
                n.tag = 13;
                n.a = self.signature(sig)?
            },
            Call { callee, args } => {
                n.tag = 14;
                match callee {
                    ast::CalleeRef::Direct(name) => {
                        n.a = 0;
                        n.b = self.name(name)?
                    },
                    ast::CalleeRef::Indirect(value) => {
                        n.a = 1;
                        n.b = self.expr(value)?
                    },
                }
                (n.c, n.d) = self.exprs(args)?
            },
            Var(name) => {
                n.tag = 15;
                n.a = self.name(name)?
            },
            Lit(ast::Lit::Bool(b)) => {
                n.tag = 16;
                n.a = u64::from(*b)
            },
            Lit(ast::Lit::Int { bits, signed, value }) => {
                n.tag = 17;
                n.a = (*bits).into();
                n.b = u64::from(*signed);
                n.c = value.as_ptr() as u64;
                n.d = value.len() as u64
            },
            Lit(ast::Lit::Float { bits, value }) => {
                n.tag = 18;
                n.a = (*bits).into();
                n.b = value.as_ptr() as u64;
                n.c = value.len() as u64
            },
            Binary { left, op, right } => {
                n.tag = 19;
                n.a = self.expr(left)?;
                n.b = binop(*op);
                n.c = self.expr(right)?
            },
            Cmp { left, op, right } => {
                n.tag = 20;
                n.a = self.expr(left)?;
                n.b = cmpop(*op);
                n.c = self.expr(right)?
            },
        }
        let id = self.push(n)?;
        if let Lit(lit) = e {
            self.literals.try_reserve(1).map_err(|_| expansion())?;
            self.literals.insert(id, lit);
        }
        Ok(id)
    }

    fn stmt(&mut self, s: &'a ast::Stmt) -> Result<u64, LowerError> {
        use ast::Stmt::*;
        let mut n = Node::default();
        match s {
            Located { annotation, statement } => {
                n.tag = 50;
                n.a = self.annotation(annotation)?;
                n.b = self.stmt(statement)?
            },
            Return(e) => {
                n.tag = 51;
                n.a = match e {
                    Some(e) => self.expr(e)?,
                    None => NONE,
                }
            },
            VolatileStore { pointer, value } => {
                n.tag = 52;
                n.a = self.expr(pointer)?;
                n.b = self.expr(value)?
            },
            Store { place, value } => {
                n.tag = 53;
                n.a = self.expr(place)?;
                n.b = self.expr(value)?
            },
            ConstDecl { name, ty, init } => {
                n.tag = 54;
                n.a = self.name(name)?;
                n.b = self.ty(ty)?;
                n.c = self.expr(init)?
            },
            VarDecl { name, ty, init } => {
                n.tag = 55;
                n.a = self.name(name)?;
                n.b = self.ty(ty)?;
                n.c = match init {
                    Some(e) => self.expr(e)?,
                    None => NONE,
                }
            },
            Assign { name, value } => {
                n.tag = 56;
                n.a = self.name(name)?;
                n.b = self.expr(value)?
            },
            ExprStmt(e) => {
                n.tag = 57;
                n.a = self.expr(e)?
            },
            If { cond, then_body, else_body } => {
                n.tag = 58;
                n.a = self.expr(cond)?;
                (n.b, n.c) = self.stmts(then_body)?;
                (n.d, n.e) = self.stmts(else_body)?
            },
            While { cond, body } => {
                n.tag = 59;
                n.a = self.expr(cond)?;
                (n.b, n.c) = self.stmts(body)?
            },
            Break => n.tag = 60,
            Continue => n.tag = 61,
        }
        self.push(n)
    }

    fn program(&mut self, p: &'a ast::Program) -> Result<u64, LowerError> {
        let mut ids = Vec::new();
        ids.try_reserve(p.declarations.len()).map_err(|_| expansion())?;
        for d in &p.declarations {
            let a = self.name(&d.name)?;
            let b = self.signature(&d.signature)?;
            let link = match &d.link_name {
                Some(s) => self.name(s)?,
                None => NONE,
            };
            ids.push(self.push(Node {
                tag: 201,
                a,
                b,
                c: linkage(d.linkage),
                d: link,
                ..Node::default()
            })?)
        }
        let (a, b) = self.list(ids)?;
        let mut ids = Vec::new();
        ids.try_reserve(p.globals.len()).map_err(|_| expansion())?;
        for g in &p.globals {
            let a = self.name(&g.name)?;
            let b = self.ty(&g.ty)?;
            let c = self.expr(&g.init)?;
            ids.push(self.push(Node { tag: 202, a, b, c, ..Node::default() })?)
        }
        let (c, d) = self.list(ids)?;
        let mut ids = Vec::new();
        ids.try_reserve(p.functions.len()).map_err(|_| expansion())?;
        for f in &p.functions {
            let name = self.name(&f.name)?;
            let mut params = Vec::new();
            params.try_reserve(f.parameters.len()).map_err(|_| expansion())?;
            for p in &f.parameters {
                let a = self.name(&p.name)?;
                let b = self.ty(&p.ty)?;
                params.push(self.push(Node { tag: 204, a, b, ..Node::default() })?)
            }
            let (first, count) = self.list(params)?;
            let ret = self.ty(&f.return_type)?;
            let (body, body_count) = self.stmts(&f.body)?;
            let link = match &f.link_name {
                Some(s) => self.name(s)?,
                None => NONE,
            };
            ids.push(self.push(Node {
                tag: 203,
                a: name,
                b: first,
                c: count,
                d: ret,
                e: body,
                f: body_count,
                g: convention(f.convention),
                h: linkage(f.linkage),
                i: link,
                ..Node::default()
            })?);
        }
        let (e, f) = self.list(ids)?;
        self.push(Node { tag: 200, a, b, c, d, e, f, ..Node::default() })
    }
}

fn convention(c: CallingConvention) -> u64 {
    match c {
        CallingConvention::Whale => 0,
        CallingConvention::SysV64 => 1,
    }
}

fn linkage(l: Linkage) -> u64 {
    match l {
        Linkage::Internal => 0,
        Linkage::External => 1,
    }
}

fn binop(op: ast::BinOpRef) -> u64 {
    use ast::BinOpRef::*;
    match op {
        Add => 0,
        Sub => 1,
        Mul => 2,
        BitAnd => 3,
        BitOr => 4,
        BitXor => 5,
        Div => 6,
        Rem => 7,
        ShiftLeft => 8,
        ShiftRight => 9,
        LogicalAnd => 10,
        LogicalOr => 11,
    }
}

fn cmpop(op: ast::CmpOpRef) -> u64 {
    use ast::CmpOpRef::*;
    match op {
        Eq => 0,
        Ne => 1,
        Lt => 2,
        Le => 3,
        Gt => 4,
        Ge => 5,
    }
}

#[derive(Clone)]
struct Output {
    result: ResultRecord,
    requests: Vec<EvalRequest>,
    types: Vec<RawType>,
    type_edges: Vec<u64>,
    instructions: Vec<RawInstruction>,
    output_edges: Vec<u64>,
    blocks: Vec<RawBlock>,
    constants: Vec<RawConstant>,
    declarations: Vec<RawDeclaration>,
    functions: Vec<RawFunction>,
    globals: Vec<RawGlobal>,
    source_order: Vec<u64>,
    mapping_order: Vec<u64>,
    value_types: Vec<u64>,
    error_index: u64,
    instruction_count: usize,
    edge_count: usize,
    value_count: usize,
}

fn execute(
    w: &Wire<'_>,
    root: u64,
    target: Target,
    limits: AstLimits,
    counts: Option<(usize, usize, usize)>,
) -> Result<Output, LowerError> {
    let n = w.nodes.len();
    let names = w.names.len();
    let annotations = w.annotations.len();
    let four = n.checked_mul(4).ok_or_else(expansion)?;
    let type_cap = four.checked_add(17).ok_or_else(expansion)?;
    let edge_cap = four.checked_add(64).ok_or_else(expansion)?;
    let block_cap = four.checked_add(8).ok_or_else(expansion)?;
    let binding_cap = n.checked_add(8).ok_or_else(expansion)?;
    let table_cap =
        type_cap.checked_mul(2).and_then(usize::checked_next_power_of_two).ok_or_else(expansion)?;
    let original_cap = annotations
        .checked_mul(2)
        .and_then(|n| n.max(2).checked_next_power_of_two())
        .ok_or_else(expansion)?;
    let mut types = zeroed::<RawType>(type_cap)?;
    let mut type_edges = zeroed::<u64>(edge_cap)?;
    let mut type_table = zeroed::<u64>(table_cap)?;
    let mut type_map = filled(n, NONE)?;
    let mut temporary = zeroed::<u64>(edge_cap)?;
    let mut name_marks = zeroed::<u64>(names)?;
    let mut layout_nodes = zeroed::<LayoutNode>(type_cap)?;
    let mut layouts = zeroed::<Layout>(type_cap)?;
    let mut offsets = zeroed::<u64>(edge_cap)?;
    let mut type_sizes = zeroed::<u64>(type_cap)?;
    let mut blocks = zeroed::<RawBlock>(block_cap)?;
    let mut declarations = zeroed::<RawDeclaration>(n)?;
    let mut functions = zeroed::<RawFunction>(n)?;
    let mut globals = zeroed::<RawGlobal>(n)?;
    let mut global_bindings = zeroed::<Binding>(n)?;
    let mut bindings = zeroed::<Binding>(binding_cap)?;
    let mut loops = zeroed::<RawLoop>(binding_cap)?;
    let mut original_table = zeroed::<OriginalSlot>(original_cap)?;
    let mut source_marks = zeroed::<u64>(annotations)?;
    let mut declaration_map = filled(names, NONE)?;
    let mut field_slots = filled(names, NONE)?;
    let mut field_bindings = zeroed::<FieldBinding>(binding_cap)?;
    let mut local_slots = filled(names, NONE)?;
    let mut global_slots = filled(names, NONE)?;
    let (instruction_cap, output_edge_cap, value_cap) = counts.unwrap_or((0, 0, 0));
    let publish = counts.is_some();
    let mut instructions = zeroed::<RawInstruction>(instruction_cap)?;
    let mut output_edges = zeroed::<u64>(output_edge_cap)?;
    let mut value_types = zeroed::<u64>(value_cap)?;
    let mut constants = zeroed::<RawConstant>(n)?;
    let mut requests = zeroed::<EvalRequest>(n)?;
    let mut source_order = zeroed::<u64>(if publish { annotations } else { 0 })?;
    let mut mapping_order = zeroed::<u64>(if publish { annotations } else { 0 })?;
    let raw_names = w
        .names
        .iter()
        .map(|s| RawString { data: s.as_ptr(), length: s.len() as u64 })
        .collect::<Vec<_>>();
    let storage = target.specification().storage;
    let (large_array_threshold, large_array_align) = storage.large_array.unwrap_or((0, 0));
    let storage = Storage {
        ptr_bits: storage.data.ptr_bits.into(),
        pointer_align: storage.pointer_align.into(),
        scalar_align1: storage.scalar_align[0].into(),
        scalar_align2: storage.scalar_align[1].into(),
        scalar_align4: storage.scalar_align[2].into(),
        scalar_align8: storage.scalar_align[3].into(),
        scalar_align16: storage.scalar_align[4].into(),
        large_array_threshold,
        large_array_align: large_array_align.into(),
        has_large_array: u64::from(storage.large_array.is_some()),
    };
    let mut result = ResultRecord::default();
    let mut core = Context {
        nodes: w.nodes.as_ptr(),
        node_count: n as u64,
        edges: w.edges.as_ptr(),
        edge_count: w.edges.len() as u64,
        names: raw_names.as_ptr(),
        name_count: names as u64,
        types: types.as_mut_ptr(),
        type_capacity: type_cap as u64,
        type_edges: type_edges.as_mut_ptr(),
        type_edge_capacity: edge_cap as u64,
        type_table: type_table.as_mut_ptr(),
        type_table_capacity: table_cap as u64,
        type_map: type_map.as_mut_ptr(),
        temporary: temporary.as_mut_ptr(),
        temporary_capacity: edge_cap as u64,
        name_marks: name_marks.as_mut_ptr(),
        storage: &storage,
        layout_nodes: layout_nodes.as_mut_ptr(),
        layouts: layouts.as_mut_ptr(),
        layout_offsets: offsets.as_mut_ptr(),
        max_depth: limits.max_depth.min(super::MAX_AST_DEPTH) as u64,
        max_work: limits.max_nodes.saturating_mul(256).saturating_add(1024) as u64,
        result: &mut result,
        ..Context::default()
    };
    let mut d = Driver {
        core: &mut core,
        mode: u64::from(publish),
        max_expansion: IrLimits::default().max_nodes as u64,
        instructions: if publish { instructions.as_mut_ptr() } else { std::ptr::null_mut() },
        instruction_capacity: instruction_cap as u64,
        output_edges: if publish { output_edges.as_mut_ptr() } else { std::ptr::null_mut() },
        output_edge_capacity: output_edge_cap as u64,
        blocks: blocks.as_mut_ptr(),
        block_capacity: block_cap as u64,
        constants: constants.as_mut_ptr(),
        declarations: declarations.as_mut_ptr(),
        declaration_capacity: n as u64,
        functions: functions.as_mut_ptr(),
        function_capacity: n as u64,
        globals: globals.as_mut_ptr(),
        global_capacity: n as u64,
        global_bindings: global_bindings.as_mut_ptr(),
        bindings: bindings.as_mut_ptr(),
        binding_capacity: binding_cap as u64,
        loops: loops.as_mut_ptr(),
        loop_capacity: binding_cap as u64,
        annotations: w.raw_annotations.as_ptr(),
        annotation_count: annotations as u64,
        original_table: original_table.as_mut_ptr(),
        original_capacity: original_cap as u64,
        source_order: if publish { source_order.as_mut_ptr() } else { std::ptr::null_mut() },
        source_capacity: annotations as u64,
        source_marks: source_marks.as_mut_ptr(),
        type_sizes: type_sizes.as_mut_ptr(),
        value_types: if publish { value_types.as_mut_ptr() } else { std::ptr::null_mut() },
        value_capacity: value_cap as u64,
        declaration_map: declaration_map.as_mut_ptr(),
        field_slots: field_slots.as_mut_ptr(),
        field_bindings: field_bindings.as_mut_ptr(),
        field_binding_capacity: binding_cap as u64,
        local_slots: local_slots.as_mut_ptr(),
        global_slots: global_slots.as_mut_ptr(),
        mapping_order: if publish { mapping_order.as_mut_ptr() } else { std::ptr::null_mut() },
        mapping_capacity: annotations as u64,
        max_source_work: limits.max_source_work as u64,
        requests: requests.as_mut_ptr(),
        request_capacity: n as u64,
        ..Driver::default()
    };
    // Every pointee is caller-owned and remains alive throughout this synchronous
    // call. Wave receives exact capacities and only publishes into the second pass.
    let status = unsafe { whale_ir_lower_o0(&mut d, root) };
    if status < 0
        || result.status != status as u64
        || core.work > core.max_work
        || result.work != core.work
        || core.type_count > core.type_capacity
        || core.type_edge_count > core.type_edge_capacity
        || d.block_count > d.block_capacity
        || d.declaration_count > d.declaration_capacity
        || d.function_count > d.function_capacity
        || d.global_count > d.global_capacity
        || d.source_count > d.source_capacity
        || d.mapping_count > d.mapping_capacity
        || d.source_work > d.max_source_work
        || d.expansion > d.max_expansion
        || d.request_count > d.request_capacity
        || (publish
            && (d.instruction_count > d.instruction_capacity
                || d.output_edge_count > d.output_edge_capacity
                || d.next_value > d.value_capacity))
    {
        return Err(backend());
    }
    requests.truncate(d.request_count as usize);
    types.truncate(core.type_count as usize);
    type_edges.truncate(core.type_edge_count as usize);
    blocks.truncate(d.block_count as usize);
    declarations.truncate(d.declaration_count as usize);
    functions.truncate(d.function_count as usize);
    globals.truncate(d.global_count as usize);
    if publish {
        source_order.truncate(d.source_count as usize);
        mapping_order.truncate(d.mapping_count as usize);
    }
    Ok(Output {
        result,
        requests,
        error_index: d.error_index,
        types,
        type_edges,
        instructions,
        output_edges,
        blocks,
        constants,
        declarations,
        functions,
        globals,
        source_order,
        mapping_order,
        value_types,
        instruction_count: usize::try_from(d.instruction_count).map_err(|_| backend())?,
        edge_count: usize::try_from(d.output_edge_count).map_err(|_| backend())?,
        value_count: usize::try_from(d.next_value).map_err(|_| backend())?,
    })
}

fn index(v: u64) -> Result<usize, LowerError> {
    usize::try_from(v).map_err(|_| backend())
}

fn id(v: u64) -> Result<u32, LowerError> {
    u32::try_from(v).map_err(|_| backend())
}

fn slice<T>(values: &[T], first: u64, count: u64) -> Result<&[T], LowerError> {
    let first = index(first)?;
    let count = index(count)?;
    values.get(first..first.checked_add(count).ok_or_else(backend)?).ok_or_else(backend)
}

fn name<'a>(w: &'a Wire<'_>, v: u64) -> Result<&'a str, LowerError> {
    w.names.get(index(v)?).copied().ok_or_else(backend)
}

fn annotation<'a>(w: &'a Wire<'_>, v: u64) -> Result<Option<&'a SourceAnnotation>, LowerError> {
    if v == NONE {
        Ok(None)
    } else {
        w.annotations.get(index(v)?).copied().map(Some).ok_or_else(backend)
    }
}

fn located(w: &Wire<'_>, v: u64, error: LowerError) -> Result<LowerError, LowerError> {
    Ok(match annotation(w, v)? {
        Some(a) => error.at(a),
        None => error,
    })
}

fn calling(v: u64) -> Result<CallingConvention, LowerError> {
    match v {
        0 => Ok(CallingConvention::Whale),
        1 => Ok(CallingConvention::SysV64),
        _ => Err(backend()),
    }
}

fn link(v: u64) -> Result<Linkage, LowerError> {
    match v {
        0 => Ok(Linkage::Internal),
        1 => Ok(Linkage::External),
        _ => Err(backend()),
    }
}

fn decode_type(o: &Output, root: u64, depth: usize) -> Result<Type, LowerError> {
    decode_type_inner(o, root, depth, &mut IrLimits::default().max_nodes)
}

fn decode_type_inner(
    o: &Output,
    root: u64,
    depth: usize,
    remaining: &mut usize,
) -> Result<Type, LowerError> {
    *remaining = remaining.checked_sub(1).ok_or_else(backend)?;
    if depth > super::MAX_AST_DEPTH {
        return Err(backend());
    }
    let t = o.types.get(index(root)?).ok_or_else(backend)?;
    Ok(match t.kind {
        0 => Type::Void,
        1 => Type::Bool,
        2 => Type::I1,
        3 => Type::U1,
        4 => Type::I8,
        5 => Type::I16,
        6 => Type::I32,
        7 => Type::I64,
        8 => Type::I128,
        9 => Type::U8,
        10 => Type::U16,
        11 => Type::U32,
        12 => Type::U64,
        13 => Type::U128,
        14 => Type::F16,
        15 => Type::F32,
        16 => Type::F64,
        17 => Type::Ptr(Box::new(decode_type_inner(o, t.child, depth + 1, remaining)?)),
        18 => Type::FnPtr(Box::new(FunctionSignature {
            params: slice(&o.type_edges, t.first, t.count)?
                .iter()
                .map(|id| decode_type_inner(o, *id, depth + 1, remaining))
                .collect::<Result<_, _>>()?,
            ret: decode_type_inner(o, t.child, depth + 1, remaining)?,
            convention: calling(t.auxiliary)?,
            variadic: false,
        })),
        19 => {
            Type::Array(Box::new(decode_type_inner(o, t.child, depth + 1, remaining)?), t.auxiliary)
        },
        20 | 21 => {
            let fields = slice(&o.type_edges, t.first, t.count)?
                .iter()
                .map(|id| decode_type_inner(o, *id, depth + 1, remaining))
                .collect::<Result<_, _>>()?;
            if t.kind == 20 {
                Type::Struct(fields)
            } else {
                Type::Tuple(fields)
            }
        },
        _ => return Err(backend()),
    })
}

fn signature(o: &Output, ty: u64) -> Result<FunctionSignature, LowerError> {
    match decode_type(o, ty, 0)? {
        Type::FnPtr(sig) => Ok(*sig),
        _ => Err(backend()),
    }
}

fn payload(category: u64, low: u64, high: u64, ty: &Type) -> Result<ConstValue, LowerError> {
    let bits = u128::from(low) | (u128::from(high) << 64);
    Ok(match category {
        1 if low <= 1 && high == 0 => ConstValue::Bool(low != 0),
        2 => ConstValue::I(bits as i128),
        3 => ConstValue::U(bits),
        4 if high == 0 => ConstValue::F(match ty {
            Type::F16 if low <= u16::MAX.into() => FloatBits::F16(low as u16),
            Type::F32 if low <= u32::MAX.into() => FloatBits::F32(low as u32),
            Type::F64 => FloatBits::F64(low),
            _ => return Err(backend()),
        }),
        _ => return Err(backend()),
    })
}

fn const_binary(op: u64) -> Result<ConstBinaryOp, LowerError> {
    use ConstBinaryOp::*;
    Ok(match op {
        0 => Add,
        1 => Sub,
        2 => Mul,
        3 => BitAnd,
        4 => BitOr,
        5 => BitXor,
        6 => Div,
        7 => Rem,
        8 => ShiftLeft,
        9 => ShiftRight,
        10 => LogicalAnd,
        11 => LogicalOr,
        _ => return Err(backend()),
    })
}

fn const_compare(op: u64) -> Result<ConstCompareOp, LowerError> {
    use ConstCompareOp::*;
    Ok(match op {
        0 => Eq,
        1 => Ne,
        2 => Lt,
        3 => Le,
        4 => Gt,
        5 => Ge,
        _ => return Err(backend()),
    })
}

fn decode_constant(o: &Output, root: u64, depth: usize) -> Result<ConstExpr, LowerError> {
    decode_constant_inner(o, root, depth, &mut IrLimits::default().max_nodes)
}

fn decode_constant_inner(
    o: &Output,
    root: u64,
    depth: usize,
    remaining: &mut usize,
) -> Result<ConstExpr, LowerError> {
    *remaining = remaining.checked_sub(1).ok_or_else(backend)?;
    if depth > super::MAX_AST_DEPTH {
        return Err(backend());
    }
    let c = o.constants.get(index(root)?).ok_or_else(backend)?;
    let ty = decode_type(o, c.ty, 0)?;
    let kind = match c.kind {
        0 => ConstExprKind::Literal(payload(c.operation, c.low, c.high, &ty)?),
        1 => ConstExprKind::Reference(ConstRef::Local(ValueId(id(c.reference)?))),
        2 => ConstExprKind::Reference(ConstRef::Global(GlobalId(id(c.reference)?))),
        3 => ConstExprKind::Neg(Box::new(decode_constant_inner(o, c.left, depth + 1, remaining)?)),
        4 => ConstExprKind::Not(Box::new(decode_constant_inner(o, c.left, depth + 1, remaining)?)),
        5 => ConstExprKind::Binary {
            op: const_binary(c.operation)?,
            left: Box::new(decode_constant_inner(o, c.left, depth + 1, remaining)?),
            right: Box::new(decode_constant_inner(o, c.right, depth + 1, remaining)?),
        },
        6 => ConstExprKind::Compare {
            op: const_compare(c.operation)?,
            left: Box::new(decode_constant_inner(o, c.left, depth + 1, remaining)?),
            right: Box::new(decode_constant_inner(o, c.right, depth + 1, remaining)?),
        },
        _ => return Err(backend()),
    };
    Ok(ConstExpr { ty, kind })
}

struct Evaluated {
    root: u64,
    expression: ConstExpr,
    value: ConstValue,
}

type ConstantResults = HashMap<ConstRef, Evaluated>;

fn evaluate_requests(w: &Wire<'_>, o: &Output) -> Result<ConstantResults, LowerError> {
    let mut values = ConstantResults::new();
    values.try_reserve(o.requests.len()).map_err(|_| expansion())?;
    for request in &o.requests {
        let key = match request.kind {
            0 => ConstRef::Global(GlobalId(id(request.identity)?)),
            1 => ConstRef::Local(ValueId(id(request.identity)?)),
            _ => return Err(backend()),
        };
        let expression = decode_constant(o, request.expression, 0)?;
        let value = expression
            .evaluate_with_error_site(
                &|reference| {
                    values
                        .get(&reference)
                        .map(|entry| (entry.expression.ty.clone(), entry.value.clone()))
                },
                IrLimits::default(),
            )
            .map_err(|(error, failed)| {
                constant_error(
                    w,
                    o,
                    request.expression,
                    &expression,
                    failed,
                    error,
                    request.annotation,
                )
            })?;
        if values.insert(key, Evaluated { root: request.expression, expression, value }).is_some() {
            return Err(backend());
        }
    }
    Ok(values)
}

fn constant_error(
    w: &Wire<'_>,
    o: &Output,
    root: u64,
    expression: &ConstExpr,
    failed: &ConstExpr,
    error: ConstEvalError,
    fallback: u64,
) -> LowerError {
    let mut pending = vec![(root, expression)];
    while let Some((raw, typed)) = pending.pop() {
        let Some(c) = index(raw).ok().and_then(|i| o.constants.get(i)) else { return backend() };
        if std::ptr::eq(typed, failed) {
            return located(
                w,
                if c.annotation == NONE { fallback } else { c.annotation },
                LowerError::ConstantEvaluation(error),
            )
            .unwrap_or_else(|e| e);
        }
        match &typed.kind {
            ConstExprKind::Neg(v) | ConstExprKind::Not(v) => pending.push((c.left, v)),
            ConstExprKind::Binary { left, right, .. }
            | ConstExprKind::Compare { left, right, .. } => {
                pending.push((c.right, right));
                pending.push((c.left, left));
            },
            _ => {},
        }
    }
    backend()
}

fn kernel_error(w: &Wire<'_>, o: &Output) -> Result<LowerError, LowerError> {
    let r = &o.result;
    let raw = w.nodes.get(index(r.node)?).ok_or_else(backend)?;
    let err = match r.status {
        1 => LowerError::ResourceLimit(AstLimitError::SourceWork),
        2 => LowerError::ResourceLimit(AstLimitError::Depth),
        3 | 4 => return Err(backend()),
        5 => LowerError::UnsupportedType(match raw.tag {
            102 | 17 => format!("int bits={} signed={}", raw.a, raw.b != 0),
            103 | 18 => format!("float bits={}", raw.a),
            109 => format!("opaque {}", name(w, raw.a)?),
            _ => return Err(backend()),
        }),
        6 => LowerError::TypeMismatch {
            expected: decode_type(o, r.expected, 0)?,
            got: decode_type(o, r.actual, 0)?,
        },
        7 => LowerError::UnsupportedExpr,
        8 => LowerError::DuplicateFunction(name(w, r.detail)?.to_owned()),
        9 => LowerError::DuplicateParameter {
            func: name(w, raw.a)?.to_owned(),
            param: name(w, r.detail)?.to_owned(),
        },
        10 => LowerError::DuplicateGlobal(name(w, r.detail)?.to_owned()),
        11 => LowerError::UnknownVariable(name(w, r.detail)?.to_owned()),
        12 => LowerError::AssignToConst(name(w, r.detail)?.to_owned()),
        13 => LowerError::BreakOutsideLoop,
        14 => LowerError::ContinueOutsideLoop,
        15 => LowerError::ValueReturnedFromVoid,
        16 => LowerError::VoidCallUsedAsValue,
        17 => LowerError::NonConstExpr,
        18 => LowerError::Aggregate(match r.detail {
            1 => "duplicate or empty struct field".into(),
            2 => "initial structs require scalar fields".into(),
            3 => format!("unknown field {:?}", name(w, raw.c)?),
            4 => "struct initializer field count mismatch".into(),
            5 => "duplicate field initializer".into(),
            6 => format!("unknown initializer field {:?}", name(w, raw.a)?),
            7 => "initial array literals require scalar elements".into(),
            8 => "array literal length mismatch".into(),
            9 => "aggregate by-value signatures are unsupported".into(),
            _ => return Err(backend()),
        }),
        19 => {
            let lit = w.literals.get(&r.node).ok_or_else(backend)?;
            match lit {
                ast::Lit::Int { signed, value, .. } => match r.detail {
                    2 | 4 => LowerError::NumericLiteral(
                        "expected decimal digits with an optional signed minus".into(),
                    ),
                    3 if value.parse::<i128>().is_err() => {
                        LowerError::NumericLiteral("integer exceeds i128 range".into())
                    },
                    5 if value.parse::<u128>().is_err() => {
                        LowerError::NumericLiteral("integer exceeds u128 range".into())
                    },
                    3 | 5 | 9 => LowerError::InvalidLiteral {
                        ty: decode_type(o, r.expected, 0)?,
                        value: if *signed {
                            ConstValue::I(value.parse().map_err(|_| backend())?)
                        } else {
                            ConstValue::U(value.parse().map_err(|_| backend())?)
                        },
                    },
                    _ => return Err(backend()),
                },
                ast::Lit::Float { bits, value } => LowerError::NumericLiteral(
                    FloatBits::parse(*bits, value).err().ok_or_else(backend)?,
                ),
                _ => return Err(backend()),
            }
        },
        20 => LowerError::Layout(match r.detail {
            1 => LayoutError::Void,
            2 => LayoutError::DepthLimit,
            3 => LayoutError::Overflow,
            4 | 5 => return Err(backend()),
            _ => return Err(backend()),
        }),
        21 => LowerError::Call(match r.detail {
            1 => CallError::InvalidSignature(
                "void parameters, malformed types and variadic calls are unsupported".into(),
            ),
            2 => CallError::UnknownFunctionName(name(w, r.expected)?.to_owned()),
            3 => CallError::NotFunctionPointer(ValueId(id(r.expected)?)),
            4 => CallError::ArgumentCount { expected: index(r.expected)?, got: index(r.actual)? },
            5 => CallError::ArgumentType {
                index: index(o.error_index)?,
                expected: decode_type(o, r.expected, 0)?,
                got: decode_type(o, r.actual, 0)?,
            },
            6 => CallError::ConflictingDeclaration(name(w, r.expected)?.to_owned()),
            7 => CallError::MissingDefinition(FunctionId(id(r.expected)?)),
            8 => CallError::DuplicateDefinition(FunctionId(id(r.expected)?)),
            9 => CallError::InvalidLinkName(name(w, r.expected)?.to_owned()),
            10 => CallError::LinkNameCollision(name(w, r.expected)?.to_owned()),
            _ => return Err(backend()),
        }),
        22 => LowerError::ResourceLimit(AstLimitError::SourceWork),
        23 => LowerError::ResourceLimit(AstLimitError::Expansion),
        24 => LowerError::UnsupportedStmt,
        _ => return Err(backend()),
    };
    if r.status == 22 {
        Ok(err)
    } else {
        located(w, r.annotation, err)
    }
}

fn runtime_bin(op: u64) -> Result<BinOp, LowerError> {
    use BinOp::*;
    Ok(match op {
        0 => Add,
        1 => Sub,
        2 => Mul,
        3 => UDiv,
        4 => SDiv,
        5 => URem,
        6 => SRem,
        7 => FAdd,
        8 => FSub,
        9 => FMul,
        10 => FDiv,
        11 => FRem,
        12 => And,
        13 => Or,
        14 => Xor,
        15 => Shl,
        16 => LShr,
        17 => AShr,
        _ => return Err(backend()),
    })
}

fn runtime_cmp(op: u64) -> Result<CmpOp, LowerError> {
    use CmpOp::*;
    Ok(match op {
        0 => Eq,
        1 => Ne,
        2 => SLt,
        3 => SLe,
        4 => SGt,
        5 => SGe,
        6 => ULt,
        7 => ULe,
        8 => UGt,
        9 => UGe,
        10 => FEq,
        11 => FNe,
        12 => FLt,
        13 => FLe,
        14 => FGt,
        15 => FGe,
        _ => return Err(backend()),
    })
}

fn runtime_cast(op: u64) -> Result<CastOp, LowerError> {
    use CastOp::*;
    Ok(match op {
        0 => CheckedInt,
        1 => ZExt,
        2 => SExt,
        3 => Trunc,
        4 => FExt,
        5 => FTrunc,
        6 => IToF_S,
        7 => IToF_U,
        8 => FToI_S,
        9 => FToI_U,
        10 => Bitcast,
        11 => PtrToInt,
        12 => IntToPtr,
        _ => return Err(backend()),
    })
}

struct FunctionRange {
    values: u64,
    end_values: u64,
    blocks: u64,
    end_blocks: u64,
}

impl FunctionRange {
    fn value(&self, v: u64) -> Result<ValueId, LowerError> {
        if v < self.values || v >= self.end_values {
            return Err(backend());
        }
        Ok(ValueId(id(v)?))
    }

    fn block(&self, v: u64) -> Result<BlockId, LowerError> {
        if v < self.blocks || v >= self.end_blocks {
            return Err(backend());
        }
        Ok(BlockId(id(v)?))
    }
}

fn instruction(
    w: &Wire<'_>,
    o: &Output,
    r: &RawInstruction,
    range: &FunctionRange,
    global_values: &[(Type, ConstValue)],
    constants: &mut ConstantResults,
) -> Result<Instruction, LowerError> {
    let ty = || decode_type(o, r.ty, 0);
    let value = |v| range.value(v);
    let dst = || value(r.dst);
    let align = |v| id(v);
    Ok(match r.tag {
        3 => {
            let dst = dst()?;
            let evaluated = constants.remove(&ConstRef::Local(dst)).ok_or_else(backend)?;
            if evaluated.root != r.b {
                return Err(backend());
            }
            Instruction::ConstDecl {
                dst,
                name: name(w, r.a)?.to_owned(),
                expression: evaluated.expression,
                value: evaluated.value,
            }
        },
        4 => Instruction::Const {
            dst: dst()?,
            ty: ty()?,
            value: match r.a {
                1 => global_values.get(index(r.b)?).map(|(_, v)| v.clone()).ok_or_else(backend)?,
                2 => payload(r.d, r.b, r.c, &ty()?)?,
                _ => return Err(backend()),
            },
        },
        7 => Instruction::Bin {
            dst: dst()?,
            op: runtime_bin(r.a)?,
            ty: ty()?,
            lhs: value(r.b)?,
            rhs: value(r.c)?,
        },
        8 => Instruction::Neg { dst: dst()?, ty: ty()?, src: value(r.a)? },
        9 => Instruction::Not { dst: dst()?, ty: ty()?, src: value(r.a)? },
        10 => Instruction::Cmp {
            dst: dst()?,
            op: runtime_cmp(r.a)?,
            ty: ty()?,
            lhs: value(r.b)?,
            rhs: value(r.c)?,
        },
        11 => Instruction::ICmp {
            dst: dst()?,
            pred: match r.a {
                0 => ICmpPred::Eq,
                1 => ICmpPred::Ne,
                2 => ICmpPred::Ult,
                3 => ICmpPred::Ule,
                4 => ICmpPred::Ugt,
                5 => ICmpPred::Uge,
                6 => ICmpPred::Slt,
                7 => ICmpPred::Sle,
                8 => ICmpPred::Sgt,
                9 => ICmpPred::Sge,
                _ => return Err(backend()),
            },
            ty: ty()?,
            lhs: value(r.b)?,
            rhs: value(r.c)?,
        },
        14 => Instruction::Cast {
            dst: dst()?,
            op: runtime_cast(r.a)?,
            dst_ty: ty()?,
            src_ty: decode_type(o, r.b, 0)?,
            src: value(r.c)?,
        },
        15 => {
            let edges = slice(&o.output_edges, r.first, r.count)?;
            if edges.len() % 2 != 0 {
                return Err(backend());
            }
            Instruction::Phi {
                dst: dst()?,
                ty: ty()?,
                incomings: edges
                    .chunks_exact(2)
                    .map(|p| Ok((value(p[0])?, range.block(p[1])?)))
                    .collect::<Result<_, LowerError>>()?,
            }
        },
        18 => Instruction::Alloca { dst: dst()?, ty: ty()?, align: align(r.a)? },
        19 => Instruction::VolatileLoad {
            dst: dst()?,
            ty: ty()?,
            ptr: value(r.a)?,
            align: align(r.b)?,
        },
        20 => Instruction::VolatileStore {
            ty: ty()?,
            value: value(r.a)?,
            ptr: value(r.b)?,
            align: align(r.c)?,
        },
        21 => Instruction::Load { dst: dst()?, ty: ty()?, ptr: value(r.a)?, align: align(r.b)? },
        22 => Instruction::Uninit { ty: ty()?, ptr: value(r.a)?, align: align(r.b)? },
        23 => Instruction::Store {
            ty: ty()?,
            value: value(r.a)?,
            ptr: value(r.b)?,
            align: align(r.c)?,
        },
        24 => Instruction::Gep {
            dst: dst()?,
            dst_ty: ty()?,
            base_ptr: value(r.a)?,
            indices: slice(&o.output_edges, r.first, r.count)?
                .iter()
                .map(|v| value(*v))
                .collect::<Result<_, _>>()?,
        },
        25 => Instruction::Memcpy {
            dst: value(r.a)?,
            src: value(r.b)?,
            n: value(r.c)?,
            align: align(r.d)?,
        },
        27 => Instruction::NullFunction { dst: dst()?, signature: signature(o, r.ty)? },
        28 => Instruction::FunctionAddr {
            dst: dst()?,
            function: FunctionId(id(r.a)?),
            signature: signature(o, r.ty)?,
        },
        29 => Instruction::Call {
            convention: calling(r.a)?,
            dst: if r.dst == NONE { None } else { Some(dst()?) },
            ret_ty: ty()?,
            callee: match r.b {
                0 => Callee::Direct(FunctionId(id(r.c)?)),
                1 => Callee::Indirect(value(r.c)?),
                _ => return Err(backend()),
            },
            args: slice(&o.output_edges, r.first, r.count)?
                .iter()
                .map(|v| value(*v))
                .collect::<Result<_, _>>()?,
        },
        30 => Instruction::TrapIf {
            cond: value(r.a)?,
            reason: match r.b {
                1 => "array index out of bounds",
                _ => return Err(backend()),
            }
            .into(),
        },
        _ => return Err(backend()),
    })
}

fn terminator(o: &Output, r: &RawBlock, range: &FunctionRange) -> Result<Terminator, LowerError> {
    Ok(match r.term_kind {
        1 => Terminator::Br { target: range.block(r.term_a)? },
        2 => Terminator::CBr {
            cond: range.value(r.term_a)?,
            then_bb: range.block(r.term_b)?,
            else_bb: range.block(r.term_c)?,
        },
        3 => Terminator::Ret {
            ty: decode_type(o, r.term_ty, 0)?,
            value: if r.term_a == NONE { None } else { Some(range.value(r.term_a)?) },
        },
        4 => Terminator::Trap {
            reason: match r.term_a {
                0 => "missing return",
                _ => return Err(backend()),
            }
            .into(),
        },
        _ => return Err(backend()),
    })
}

fn materialize(
    w: &Wire<'_>,
    o: &Output,
    target: Target,
    datalayout: DataLayout,
    mut constants: ConstantResults,
) -> Result<Module, LowerError> {
    let mut module = Module::new(target.name(), datalayout);
    module.declarations.try_reserve_exact(o.declarations.len()).map_err(|_| expansion())?;
    for (ordinal, d) in o.declarations.iter().enumerate() {
        module.declarations.push(FunctionDecl {
            id: FunctionId(u32::try_from(ordinal).map_err(|_| backend())?),
            name: name(w, d.name)?.to_owned(),
            signature: signature(o, d.signature)?,
            linkage: link(d.linkage)?,
            link_name: if d.link_name == NONE {
                None
            } else {
                Some(name(w, d.link_name)?.to_owned())
            },
        });
    }
    let mut mappings = zeroed::<Vec<SourceTarget>>(w.annotations.len())?;
    let mut globals = Vec::new();
    globals.try_reserve_exact(o.globals.len()).map_err(|_| expansion())?;
    for (ordinal, g) in o.globals.iter().enumerate() {
        let ty = decode_type(o, g.ty, 0)?;
        let gid = GlobalId(u32::try_from(ordinal).map_err(|_| backend())?);
        let evaluated = constants.remove(&ConstRef::Global(gid)).ok_or_else(backend)?;
        if evaluated.root != g.expression {
            return Err(backend());
        }
        let init_expr = evaluated.expression;
        let init = evaluated.value;
        module.globals.push(Global {
            id: gid,
            name: name(w, g.name)?.to_owned(),
            ty: ty.clone(),
            init: init.clone(),
            init_expr,
            align: id(g.align)?,
        });
        globals.push((ty, init));
        if g.annotation != NONE {
            mappings
                .get_mut(index(g.annotation)?)
                .ok_or_else(backend)?
                .push(SourceTarget::Global(gid));
        }
    }
    let mut visited = zeroed::<bool>(o.instructions.len())?;
    for (ordinal, f) in o.functions.iter().enumerate() {
        let decl = module.declarations.get(index(f.declaration)?).ok_or_else(backend)?;
        let raw = w.nodes.get(index(f.source)?).ok_or_else(backend)?;
        if raw.tag != 203 || f.param_count != raw.c {
            return Err(backend());
        }
        let end_values =
            o.functions.get(ordinal + 1).map_or(o.value_count as u64, |f| f.first_param);
        let end_blocks = f.first_block.checked_add(f.block_count).ok_or_else(backend)?;
        let range =
            FunctionRange { values: f.first_param, end_values, blocks: f.first_block, end_blocks };
        if end_values < f.first_param
            || end_values > o.value_count as u64
            || end_blocks > o.blocks.len() as u64
        {
            return Err(backend());
        }
        let mut params = Vec::new();
        params.try_reserve_exact(index(f.param_count)?).map_err(|_| expansion())?;
        for (ordinal, param) in slice(&w.edges, raw.b, raw.c)?.iter().enumerate() {
            let p = w.nodes.get(index(*param)?).ok_or_else(backend)?;
            if p.tag != 204 {
                return Err(backend());
            }
            let v = f.first_param.checked_add(ordinal as u64).ok_or_else(backend)?;
            let ty = *o.value_types.get(index(v)?).ok_or_else(backend)?;
            params.push(Param {
                name: name(w, p.a)?.to_owned(),
                id: range.value(v)?,
                ty: decode_type(o, ty, 0)?,
            });
        }
        let mut blocks = Vec::new();
        blocks.try_reserve_exact(index(f.block_count)?).map_err(|_| expansion())?;
        for (block_offset, b) in slice(&o.blocks, f.first_block, f.block_count)?.iter().enumerate()
        {
            let block_id = f.first_block + block_offset as u64;
            if b.id != block_id || b.function != ordinal as u64 {
                return Err(backend());
            }
            let bid = range.block(b.id)?;
            let mut instructions = Vec::new();
            instructions.try_reserve_exact(index(b.instruction_count)?).map_err(|_| expansion())?;
            let mut next = b.first;
            let mut last = NONE;
            for instruction_index in 0..index(b.instruction_count)? {
                let raw_index = index(next)?;
                let record = o.instructions.get(raw_index).ok_or_else(backend)?;
                let seen = visited.get_mut(raw_index).ok_or_else(backend)?;
                if *seen {
                    return Err(backend());
                }
                *seen = true;
                instructions.push(instruction(w, o, record, &range, &globals, &mut constants)?);
                if record.annotation != NONE {
                    mappings.get_mut(index(record.annotation)?).ok_or_else(backend)?.push(
                        SourceTarget::Instruction {
                            function: decl.id,
                            block: bid,
                            index: instruction_index,
                        },
                    );
                }
                last = next;
                next = record.next;
            }
            if next != NONE || last != b.last {
                return Err(backend());
            }
            let terminator = terminator(o, b, &range)?;
            if b.term_annotation != NONE {
                mappings
                    .get_mut(index(b.term_annotation)?)
                    .ok_or_else(backend)?
                    .push(SourceTarget::Terminator { function: decl.id, block: bid });
            }
            let block_name = match b.name {
                0 => "entry",
                1 => "unreachable.cont",
                2 => "if.then",
                3 => "if.else",
                4 => "if.cont",
                5 => "while.cond",
                6 => "while.body",
                7 => "while.exit",
                8 => "logical.rhs",
                9 => "logical.cont",
                _ => return Err(backend()),
            };
            blocks.push(BasicBlock {
                id: bid,
                name: block_name.into(),
                instructions,
                terminator: Some(terminator),
            });
        }
        let value_types = slice(&o.value_types, f.first_param, end_values - f.first_param)?
            .iter()
            .enumerate()
            .map(|(offset, ty)| {
                Ok((range.value(f.first_param + offset as u64)?, decode_type(o, *ty, 0)?))
            })
            .collect::<Result<_, LowerError>>()?;
        module.functions.push(Function {
            id: decl.id,
            name: decl.name.clone(),
            params,
            ret_ty: decl.signature.ret.clone(),
            blocks,
            entry: range.block(f.entry)?,
            value_types,
        });
    }
    if !constants.is_empty() || visited.iter().any(|seen| !*seen) {
        return Err(backend());
    }
    let mut files = HashMap::<&str, u32>::new();
    let mut originals = zeroed::<bool>(w.annotations.len())?;
    for a in &o.source_order {
        let index = index(*a)?;
        let seen = originals.get_mut(index).ok_or_else(backend)?;
        if *seen {
            return Err(backend());
        }
        *seen = true;
        let annotation = w.annotations.get(index).ok_or_else(backend)?;
        let span = if let Some(source) = &annotation.source {
            let file = if let Some(id) = files.get(source.file.as_str()) {
                *id
            } else {
                let id = u32::try_from(module.sources.files.len()).map_err(|_| backend())?;
                module.sources.files.push(SourceFile { id, name: source.file.clone() });
                files.insert(&source.file, id);
                id
            };
            Some(SourceSpan {
                file,
                line: source.line,
                column: source.column,
                end_line: source.end_line,
                end_column: source.end_column,
            })
        } else {
            None
        };
        module.sources.originals.push(OriginalNode { id: annotation.id, span });
    }
    let mut completed = zeroed::<bool>(w.annotations.len())?;
    for a in &o.mapping_order {
        let at = index(*a)?;
        let seen = completed.get_mut(at).ok_or_else(backend)?;
        if *seen || !originals.get(at).copied().unwrap_or(false) {
            return Err(backend());
        }
        *seen = true;
        let annotation = w.annotations.get(at).ok_or_else(backend)?;
        for target in std::mem::take(mappings.get_mut(at).ok_or_else(backend)?) {
            module.sources.mappings.push(SourceMapping { target, original: annotation.id });
        }
    }
    if mappings.iter().any(|bucket| !bucket.is_empty()) {
        return Err(backend());
    }
    Ok(module)
}

// Internal ABI/capacity faults take priority over user arithmetic. For genuine
// later AST failures, completed constant requests retain source evaluation order.
fn check_kernel_status(w: &Wire<'_>, o: &Output) -> Result<(), LowerError> {
    if o.result.status == 0 {
        return Ok(());
    }
    let error = kernel_error(w, o).unwrap_or_else(|error| error);
    if matches!(error.root_cause(), LowerError::Backend(_)) {
        return Err(error);
    }
    evaluate_requests(w, o)?;
    Err(error)
}

pub(super) fn lower(
    program: &ast::Program,
    target: &str,
    datalayout: DataLayout,
    limits: AstLimits,
) -> Result<Module, LowerError> {
    let source_work = super::limits::inspect_program_source_work(program, limits)
        .map_err(LowerError::ResourceLimit)?;
    let limits = AstLimits {
        max_source_work: limits
            .max_source_work
            .checked_sub(source_work)
            .ok_or(LowerError::ResourceLimit(AstLimitError::SourceWork))?,
        ..limits
    };
    let module_target = target;
    let target = if target == "whale-bytecode-v1" {
        // Select the shared storage contract explicitly; retain the requested output target.
        Target::X86_64WhaleLinux
    } else {
        Target::lookup(target).map_err(LowerError::Target)?
    };
    target.validate_layout(datalayout).map_err(LowerError::Target)?;
    let mut wire = Wire::new();
    let root = wire.program(program)?;
    let counted = execute(&wire, root, target, limits, None)?;
    check_kernel_status(&wire, &counted)?;
    let counts = (counted.instruction_count, counted.edge_count, counted.value_count);
    drop(counted);
    let output = execute(&wire, root, target, limits, Some(counts))?;
    check_kernel_status(&wire, &output)?;
    if (output.instruction_count, output.edge_count, output.value_count) != counts {
        return Err(backend());
    }
    let constants = evaluate_requests(&wire, &output)?;
    let mut module = materialize(&wire, &output, target, datalayout, constants)?;
    module.target = module_target.to_owned();
    Ok(module)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_backend_records_are_errors_before_any_module_is_returned() {
        let program =
            ast::Program {
                declarations: vec![],
                globals: vec![],
                functions: vec![ast::Function {
                    name: "transport".into(),
                    convention: CallingConvention::Whale,
                    linkage: Linkage::Internal,
                    link_name: None,
                    parameters: vec![],
                    return_type: ast::TypeRef::Int { bits: 32, signed: true },
                    body: vec![ast::Stmt::Located {
                        annotation: SourceAnnotation { id: 1, source: None },
                        statement: Box::new(ast::Stmt::Return(Some(ast::Expr::Lit(
                            ast::Lit::Int { bits: 32, signed: true, value: "42".into() },
                        )))),
                    }],
                }],
            };
        let mut wire = Wire::new();
        let root = wire.program(&program).unwrap();
        let target = Target::X86_64WhaleLinux;
        let limits = AstLimits::default();
        let probe = execute(&wire, root, target, limits, None).unwrap();
        assert_eq!(probe.result.status, 0);
        let output = execute(
            &wire,
            root,
            target,
            limits,
            Some((probe.instruction_count, probe.edge_count, probe.value_count)),
        )
        .unwrap();
        let render = |o: &Output| -> Result<Module, LowerError> {
            materialize(&wire, o, target, target.data_layout(), evaluate_requests(&wire, o)?)
        };
        assert!(render(&output).is_ok());
        let mut invalid = output.clone();
        invalid.instructions[0].next = 0;
        assert!(matches!(render(&invalid), Err(LowerError::Backend(_))));
        let mut invalid = output.clone();
        invalid.instructions[0].ty = NONE;
        assert!(matches!(render(&invalid), Err(LowerError::Backend(_))));
        let mut invalid = output.clone();
        invalid.mapping_order.push(invalid.mapping_order[0]);
        assert!(matches!(render(&invalid), Err(LowerError::Backend(_))));
        let mut invalid = output.clone();
        let cycle = invalid.types.len() as u64;
        invalid.types.push(RawType { kind: 17, child: cycle, ..RawType::default() });
        invalid.instructions[0].ty = cycle;
        assert!(matches!(render(&invalid), Err(LowerError::Backend(_))));

        // A completed user arithmetic prefix cannot disguise a backend fault.
        // Real later AST errors still preserve that prefix's evaluation order.
        let mut arithmetic = program.clone();
        let integer = |value: &str| {
            ast::Expr::Lit(ast::Lit::Int { bits: 32, signed: true, value: value.into() })
        };
        arithmetic.globals.push(ast::GlobalConst {
            name: "bad".into(),
            ty: ast::TypeRef::Int { bits: 32, signed: true },
            init: ast::Expr::Binary {
                left: Box::new(integer("1")),
                op: ast::BinOpRef::Div,
                right: Box::new(integer("0")),
            },
        });
        let mut wire = Wire::new();
        let root = wire.program(&arithmetic).unwrap();
        let complete = execute(&wire, root, target, limits, None).unwrap();
        assert_eq!(complete.result.status, 0);
        assert!(matches!(
            evaluate_requests(&wire, &complete),
            Err(LowerError::ConstantEvaluation(ConstEvalError::DivisionByZero))
        ));
        for status in [3, 4, 255] {
            let mut invalid = complete.clone();
            invalid.result.status = status;
            assert!(matches!(check_kernel_status(&wire, &invalid), Err(LowerError::Backend(_))));
        }
        let mut invalid = complete.clone();
        invalid.result.status = 7;
        invalid.result.node = NONE;
        assert!(matches!(check_kernel_status(&wire, &invalid), Err(LowerError::Backend(_))));
        let mut invalid = complete.clone();
        invalid.result.status = 6;
        invalid.result.expected = NONE;
        assert!(matches!(check_kernel_status(&wire, &invalid), Err(LowerError::Backend(_))));
        for status in [7, 22] {
            let mut later = complete.clone();
            later.result.status = status;
            assert!(matches!(
                check_kernel_status(&wire, &later),
                Err(LowerError::ConstantEvaluation(ConstEvalError::DivisionByZero))
            ));
        }
    }
}
