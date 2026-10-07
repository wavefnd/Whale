// SPDX-License-Identifier: MPL-2.0
use super::{
    lexer::{Kind, Token},
    ParseError, ParseErrorKind,
};
use crate::*;
pub(super) struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    at: usize,
    pub limits: IrLimits,
    nodes: usize,
    pub(super) format_version: u32,
    locations: std::collections::HashMap<String, super::SourceLocation>,
}
impl<'a> Parser<'a> {
    pub fn new(tokens: Vec<Token<'a>>, limits: IrLimits) -> Self {
        Self {
            tokens,
            at: 0,
            limits,
            nodes: 0,
            format_version: 0,
            locations: std::collections::HashMap::new(),
        }
    }
    pub fn verification_location(&self, e: &VerifyError) -> super::SourceLocation {
        use VerifyError::*;
        let name = match e {
            ForbiddenUndef { func, .. }
            | InvalidMemoryLayout { func, .. }
            | InvalidCast { func, .. }
            | Call { func, .. }
            | NonDominatingValue { func, .. }
            | InvalidPhi { func, .. }
            | InvalidGep { func, .. }
            | InvalidInstructionType { func, .. }
            | OperandTypeMismatch { func, .. }
            | InvalidMemoryAlignment { func, .. }
            | InvalidSwitchCase { func, .. }
            | DuplicateSwitchCase { func, .. }
            | InvalidConstant { func, .. }
            | ConditionTypeMismatch { func, .. }
            | DuplicateBlock { func, .. }
            | DuplicateValue { func, .. }
            | DuplicateValueType { func, .. }
            | UnexpectedValueType { func, .. }
            | ValueTypeMismatch { func, .. }
            | InvalidEntryBlock { func, .. }
            | EntryHasPredecessor { func, .. }
            | InvalidBranchTarget { func, .. }
            | MissingValueType { func, .. }
            | VoidParameter { func, .. }
            | UnterminatedBlock { func, .. }
            | RetTypeMismatch { func, .. }
            | UseOfUndefinedValue { func, .. } => Some(func),
            DuplicateFunction { name }
            | DuplicateGlobal { name }
            | InvalidGlobalInitializer { name, .. }
            | InvalidGlobalAlignment { name, .. } => Some(name),
            InvalidConstExpression { scope, .. } => Some(scope),
            _ => None,
        };
        name.and_then(|n| self.locations.get(n))
            .copied()
            .unwrap_or(self.tokens[0].location)
    }
    pub fn error(&self, s: &str) -> ParseError {
        ParseError {
            location: self.tokens[self.at].location,
            kind: ParseErrorKind::Syntax(s.into()),
        }
    }
    pub fn previous_error(&self, message: &str) -> ParseError {
        ParseError {
            location: self.tokens[self.at.saturating_sub(1)].location,
            kind: ParseErrorKind::Syntax(message.into()),
        }
    }
    pub fn location(&self) -> super::SourceLocation {
        self.tokens[self.at].location
    }
    pub fn limit(&self, resource: Resource, limit: usize) -> ParseError {
        ParseError {
            location: self.tokens[self.at].location,
            kind: ParseErrorKind::ResourceLimit(LimitError { resource, limit }),
        }
    }
    pub fn node(&mut self) -> Result<(), ParseError> {
        if self.nodes >= self.limits.max_nodes {
            return Err(self.limit(Resource::Nodes, self.limits.max_nodes));
        }
        self.nodes += 1;
        Ok(())
    }
    pub fn peek(&self) -> &str {
        match &self.tokens[self.at].kind {
            Kind::Word(s) | Kind::Mark(s) => s,
            Kind::String(_) => "<string>",
        }
    }
    pub fn done(&self) -> bool {
        self.peek() == "<eof>"
    }
    pub fn eat(&mut self, s: &str) -> bool {
        if self.peek() == s {
            self.at += 1;
            true
        } else {
            false
        }
    }
    pub fn expect(&mut self, s: &str) -> Result<(), ParseError> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{s}', found '{}'", self.peek())))
        }
    }
    pub fn word(&mut self) -> Result<&'a str, ParseError> {
        match self.tokens[self.at].kind {
            Kind::Word(s) => {
                self.at += 1;
                Ok(s)
            }
            _ => Err(self.error("expected word")),
        }
    }
    pub fn string(&mut self) -> Result<String, ParseError> {
        match &mut self.tokens[self.at].kind {
            Kind::String(s) => {
                let s = std::mem::take(s);
                self.at += 1;
                Ok(s)
            }
            _ => Err(self.error("expected quoted string")),
        }
    }
    pub fn number<T: std::str::FromStr>(&mut self) -> Result<T, ParseError> {
        let s = self.peek();
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(self.error("expected unsigned decimal integer"));
        }
        let value = s.parse().map_err(|_| self.error("integer out of range"))?;
        self.at += 1;
        Ok(value)
    }
    pub fn id(&mut self, prefix: &str) -> Result<u32, ParseError> {
        let s = self
            .peek()
            .strip_prefix(prefix)
            .ok_or_else(|| self.error(&format!("expected {prefix} ID")))?;
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(self.error("invalid numeric ID"));
        }
        let id = s.parse().map_err(|_| self.error("ID out of range"))?;
        self.at += 1;
        Ok(id)
    }
    pub fn value(&mut self) -> Result<ValueId, ParseError> {
        self.id("%v").map(ValueId)
    }
    pub fn block_id(&mut self) -> Result<BlockId, ParseError> {
        self.id("%b").map(BlockId)
    }
    pub fn comma(&mut self) -> Result<(), ParseError> {
        self.expect(",")
    }
    pub fn align(&mut self) -> Result<u32, ParseError> {
        self.comma()?;
        self.expect("align")?;
        self.number()
    }
    pub fn convention(&mut self) -> Result<CallingConvention, ParseError> {
        match self.word()? {
            "whale" => Ok(CallingConvention::Whale),
            "sysv64" => Ok(CallingConvention::SysV64),
            _ => Err(self.previous_error("unknown calling convention")),
        }
    }
    pub fn signature(&mut self, depth: usize) -> Result<FunctionSignature, ParseError> {
        let convention = self.convention()?;
        self.expect("(")?;
        let params = self.types(")", depth)?;
        self.expect("->")?;
        let ret = self.ty(depth)?;
        Ok(FunctionSignature {
            params,
            ret,
            convention,
            variadic: false,
        })
    }
    fn types(&mut self, end: &str, depth: usize) -> Result<Vec<Type>, ParseError> {
        let mut types = Vec::new();
        if !self.eat(end) {
            loop {
                types.push(self.ty(depth)?);
                if self.eat(end) {
                    break;
                }
                self.comma()?;
            }
        }
        Ok(types)
    }
    pub fn ty(&mut self, depth: usize) -> Result<Type, ParseError> {
        if depth > self.limits.max_type_depth {
            return Err(self.limit(Resource::TypeDepth, self.limits.max_type_depth));
        }
        self.node()?;
        Ok(match self.word()? {
            "void" => Type::Void,
            "bool" => Type::Bool,
            "i1" => Type::I1,
            "u1" => Type::U1,
            "i8" => Type::I8,
            "i16" => Type::I16,
            "i32" => Type::I32,
            "i64" => Type::I64,
            "i128" => Type::I128,
            "u8" => Type::U8,
            "u16" => Type::U16,
            "u32" => Type::U32,
            "u64" => Type::U64,
            "u128" => Type::U128,
            "f16" => Type::F16,
            "f32" => Type::F32,
            "f64" => Type::F64,
            "ptr" => {
                self.expect("<")?;
                let inner = self.ty(depth + 1)?;
                self.expect(">")?;
                Type::ptr_to(inner)
            }
            "fnptr" => {
                self.expect("<")?;
                let sig = self.signature(depth + 1)?;
                self.expect(">")?;
                Type::FnPtr(Box::new(sig))
            }
            "array" => {
                self.expect("<")?;
                let inner = self.ty(depth + 1)?;
                self.comma()?;
                let n = self.number()?;
                self.expect(">")?;
                Type::Array(Box::new(inner), n)
            }
            "struct" => {
                self.expect("{")?;
                Type::Struct(self.types("}", depth + 1)?)
            }
            "tuple" => {
                self.expect("<")?;
                Type::Tuple(self.types(">", depth + 1)?)
            }
            _ => return Err(self.previous_error("unknown type")),
        })
    }
    pub fn literal(&mut self, ty: &Type) -> Result<ConstValue, ParseError> {
        let s = self.peek();
        let value = match ty {
            Type::Bool => match s {
                "true" => ConstValue::Bool(true),
                "false" => ConstValue::Bool(false),
                _ => return Err(self.error("expected boolean literal")),
            },
            Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 => {
                let digits = s.strip_prefix('-').unwrap_or(s);
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(self.error("expected signed decimal literal"));
                }
                ConstValue::I(
                    s.parse()
                        .map_err(|_| self.error("signed literal out of range"))?,
                )
            }
            Type::U1 | Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 => {
                if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(self.error("expected unsigned decimal literal"));
                }
                ConstValue::U(
                    s.parse()
                        .map_err(|_| self.error("unsigned literal out of range"))?,
                )
            }
            Type::F16 | Type::F32 | Type::F64 => ConstValue::F(
                FloatBits::parse(
                    match ty {
                        Type::F16 => 16,
                        Type::F32 => 32,
                        _ => 64,
                    },
                    s,
                )
                .map_err(|e| self.error(&e))?,
            ),
            _ => return Err(self.error("type has no scalar literal")),
        };
        if !crate::constant::valid_constant(ty, &value) {
            return Err(self.error("literal outside type range"));
        }
        self.at += 1;
        Ok(value)
    }
    pub fn equal_type(&self, left: &Type, right: &Type) -> Result<(), ParseError> {
        if left == right {
            Ok(())
        } else {
            Err(self.error("conflicting type annotations"))
        }
    }
    pub fn expression(&mut self, depth: usize) -> Result<ConstExpr, ParseError> {
        if depth > self.limits.max_const_depth {
            return Err(self.limit(Resource::ConstantDepth, self.limits.max_const_depth));
        }
        self.node()?;
        let op = self.peek();
        if matches!(
            op,
            "add" | "sub" | "mul" | "eq" | "ne" | "lt" | "le" | "gt" | "ge"
        ) {
            let op = self.word()?;
            self.expect("(")?;
            let left = Box::new(self.expression(depth + 1)?);
            self.comma()?;
            let right = Box::new(self.expression(depth + 1)?);
            self.expect(")")?;
            let ty = left.ty.clone();
            Ok(match op {
                "add" | "sub" | "mul" => ConstExpr {
                    ty,
                    kind: ConstExprKind::Binary {
                        op: match op {
                            "add" => ConstBinaryOp::Add,
                            "sub" => ConstBinaryOp::Sub,
                            _ => ConstBinaryOp::Mul,
                        },
                        left,
                        right,
                    },
                },
                _ => ConstExpr {
                    ty: Type::Bool,
                    kind: ConstExprKind::Compare {
                        op: match op {
                            "eq" => ConstCompareOp::Eq,
                            "ne" => ConstCompareOp::Ne,
                            "lt" => ConstCompareOp::Lt,
                            "le" => ConstCompareOp::Le,
                            "gt" => ConstCompareOp::Gt,
                            _ => ConstCompareOp::Ge,
                        },
                        left,
                        right,
                    },
                },
            })
        } else {
            let ty = self.ty(0)?;
            let kind = if self.peek().starts_with("@g") {
                ConstExprKind::Reference(ConstRef::Global(GlobalId(self.id("@g")?)))
            } else if self.peek().starts_with("%v") {
                ConstExprKind::Reference(ConstRef::Local(self.value()?))
            } else {
                ConstExprKind::Literal(self.literal(&ty)?)
            };
            Ok(ConstExpr { ty, kind })
        }
    }
    pub fn module(&mut self) -> Result<Module, ParseError> {
        self.expect("module")?;
        self.expect("{")?;
        self.expect("format_version")?;
        let version: u32 = self.number()?;
        if version != 3 && version != IR_FORMAT_VERSION {
            return Err(self.previous_error("unsupported IR format version"));
        }
        self.format_version = version;
        self.expect("semantics_version")?;
        let semantics: u32 = self.number()?;
        if semantics != SEMANTICS_VERSION {
            return Err(self.previous_error("unsupported semantics version"));
        }
        self.expect("target")?;
        let target = self.string()?;
        self.expect("datalayout")?;
        self.expect("{")?;
        self.expect("ptr")?;
        self.expect("=")?;
        let ptr_bits = self.number()?;
        self.comma()?;
        self.expect("endian")?;
        self.expect("=")?;
        let endian = match self.word()? {
            "little" => Endian::Little,
            "big" => Endian::Big,
            _ => return Err(self.previous_error("unknown byte order")),
        };
        self.expect("}")?;
        let mut module = Module::new(target, DataLayout { ptr_bits, endian });
        while !self.eat("}") {
            self.node()?;
            match self.word()? {
                "global" => {
                    let id = GlobalId(self.id("@g")?);
                    let location = self.tokens[self.at].location;
                    let name = self.string()?;
                    self.locations.entry(name.clone()).or_insert(location);
                    self.expect(":")?;
                    let ty = self.ty(0)?;
                    self.expect("=")?;
                    self.expect("const")?;
                    let annotation = self.ty(0)?;
                    self.equal_type(&ty, &annotation)?;
                    let init = self.literal(&ty)?;
                    let align = self.align()?;
                    self.comma()?;
                    self.expect("init_expr")?;
                    let init_expr = self.expression(0)?;
                    module.globals.push(Global {
                        id,
                        name,
                        ty,
                        init,
                        init_expr,
                        align,
                    });
                }
                "declare" => {
                    let id = FunctionId(self.id("@f")?);
                    let location = self.tokens[self.at].location;
                    let name = self.string()?;
                    self.locations.entry(name.clone()).or_insert(location);
                    self.expect(":")?;
                    let signature = self.signature(0)?;
                    self.comma()?;
                    self.expect("linkage")?;
                    let linkage = match self.word()? {
                        "internal" => Linkage::Internal,
                        "external" => Linkage::External,
                        _ => return Err(self.previous_error("unknown linkage")),
                    };
                    let link_name = if self.eat(",") {
                        self.expect("link_name")?;
                        Some(self.string()?)
                    } else {
                        None
                    };
                    module.declarations.push(FunctionDecl {
                        id,
                        name,
                        signature,
                        linkage,
                        link_name,
                    });
                }
                "fn" => module.functions.push(self.function()?),
                _ => return Err(self.previous_error("unknown module field")),
            }
        }
        Ok(module)
    }
    fn function(&mut self) -> Result<Function, ParseError> {
        let id = FunctionId(self.id("@f")?);
        let location = self.tokens[self.at].location;
        let name = self.string()?;
        self.locations.entry(name.clone()).or_insert(location);
        self.expect("(")?;
        let mut params = Vec::new();
        let mut value_types = Vec::new();
        if !self.eat(")") {
            loop {
                self.node()?;
                let id = self.value()?;
                let name = self.string()?;
                self.expect(":")?;
                let ty = self.ty(0)?;
                value_types.push((id, ty.clone()));
                params.push(Param { id, name, ty });
                if self.eat(")") {
                    break;
                }
                self.comma()?;
            }
        }
        self.expect("->")?;
        let ret_ty = self.ty(0)?;
        self.comma()?;
        self.expect("entry")?;
        let entry = self.block_id()?;
        self.expect("{")?;
        let mut blocks = Vec::new();
        while !self.eat("}") {
            self.node()?;
            let id = self.block_id()?;
            let name = self.string()?;
            self.expect(":")?;
            let mut b = BasicBlock::new(id, name);
            while !self.peek().starts_with("%b") && self.peek() != "}" {
                if b.terminator.is_some() {
                    return Err(self.error("instruction after terminator"));
                }
                self.node()?;
                if matches!(self.peek(), "br" | "cbr" | "switch" | "ret" | "trap") {
                    b.terminator = Some(self.terminator()?);
                } else {
                    let (ins, result) = self.instruction()?;
                    if let Some((id, ty)) = result {
                        value_types.push((id, ty));
                    }
                    b.instructions.push(ins);
                }
            }
            blocks.push(b);
        }
        Ok(Function {
            id,
            name,
            params,
            ret_ty,
            entry,
            blocks,
            value_types,
        })
    }
}
