// SPDX-License-Identifier: MPL-2.0
use super::{
    lexer::{Kind, Token},
    ParseError, ParseErrorKind,
};

use crate::*;

pub(super) struct Parser<'a> {
    pub(super) tokens: Vec<Token<'a>>,
    #[cfg(whale_wave_ir)]
    pub(super) raw_tokens: Vec<super::wave_text::RawToken>,
    #[cfg(whale_wave_ir)]
    pub(super) grammar_work: usize,
    #[cfg(all(test, whale_wave_ir))]
    pub(super) reference_types: bool,
    pub(super) at: usize,
    pub limits: IrLimits,
    pub(super) nodes: usize,
    pub(super) format_version: u32,
    pub(super) function_locations: std::collections::HashMap<String, super::SourceLocation>,
    pub(super) declaration_locations: std::collections::HashMap<String, super::SourceLocation>,
    pub(super) global_locations: std::collections::HashMap<String, super::SourceLocation>,
    pub(super) global_constant_locations:
        std::collections::HashMap<GlobalId, super::SourceLocation>,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: Vec<Token<'a>>, limits: IrLimits) -> Self {
        #[cfg(whale_wave_ir)]
        let raw_tokens = tokens.iter().map(super::wave_text::RawToken::from).collect();
        Self {
            tokens,
            #[cfg(whale_wave_ir)]
            raw_tokens,
            #[cfg(whale_wave_ir)]
            grammar_work: 0,
            #[cfg(all(test, whale_wave_ir))]
            reference_types: false,
            at: 0,
            limits,
            nodes: 0,
            format_version: 0,
            function_locations: std::collections::HashMap::new(),
            declaration_locations: std::collections::HashMap::new(),
            global_locations: std::collections::HashMap::new(),
            global_constant_locations: std::collections::HashMap::new(),
        }
    }

    pub fn verification_location(&self, e: &VerifyError) -> super::SourceLocation {
        use VerifyError::*;
        let function = |name: &String| {
            self.function_locations.get(name).or_else(|| self.declaration_locations.get(name))
        };
        let location = match e {
            InvalidMemoryLayout { func, operation: "global storage", .. } => {
                self.global_locations.get(func)
            },
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
            | UseOfUndefinedValue { func, .. } => function(func),
            DuplicateFunction { name } => function(name),
            DuplicateGlobal { name }
            | InvalidGlobalInitializer { name, .. }
            | InvalidGlobalAlignment { name, .. }
            | GlobalStorage { name, .. } => self.global_locations.get(name),
            InvalidConstExpression { declaration: ConstRef::Global(id), .. } => {
                self.global_constant_locations.get(id)
            },
            InvalidConstExpression { scope, declaration: ConstRef::Local(_), .. } => {
                function(scope)
            },
            _ => None,
        };
        location.copied().unwrap_or(self.tokens[0].location)
    }

    pub fn error(&self, s: &str) -> ParseError {
        ParseError {
            location: self.tokens[self.at].location,
            kind: ParseErrorKind::Syntax(s.into()),
        }
    }

    #[cfg(whale_wave_ir)]
    pub(super) fn backend(&self, component: &'static str) -> ParseError {
        ParseError { location: self.location(), kind: ParseErrorKind::Backend(component) }
    }

    #[cfg(any(not(whale_wave_ir), test))]
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

    #[cfg(any(not(whale_wave_ir), test))]
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

    #[cfg(any(not(whale_wave_ir), test))]
    pub fn eat(&mut self, s: &str) -> bool {
        if self.peek() == s {
            self.at += 1;
            true
        } else {
            false
        }
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub fn expect(&mut self, s: &str) -> Result<(), ParseError> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{s}', found '{}'", self.peek())))
        }
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub fn word(&mut self) -> Result<&'a str, ParseError> {
        match self.tokens[self.at].kind {
            Kind::Word(s) => {
                self.at += 1;
                Ok(s)
            },
            _ => Err(self.error("expected word")),
        }
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub fn string(&mut self) -> Result<String, ParseError> {
        match &mut self.tokens[self.at].kind {
            Kind::String(s) => {
                let s = std::mem::take(s);
                self.at += 1;
                Ok(s)
            },
            _ => Err(self.error("expected quoted string")),
        }
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub fn number<T: std::str::FromStr>(&mut self) -> Result<T, ParseError> {
        let s = self.peek();
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(self.error("expected unsigned decimal integer"));
        }
        let value = s.parse().map_err(|_| self.error("integer out of range"))?;
        self.at += 1;
        Ok(value)
    }

    #[cfg(any(not(whale_wave_ir), test))]
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

    #[cfg(any(not(whale_wave_ir), test))]
    pub fn value(&mut self) -> Result<ValueId, ParseError> {
        self.id("%v").map(ValueId)
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub fn block_id(&mut self) -> Result<BlockId, ParseError> {
        self.id("%b").map(BlockId)
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub fn comma(&mut self) -> Result<(), ParseError> {
        self.expect(",")
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub fn align(&mut self) -> Result<u32, ParseError> {
        self.comma()?;
        self.expect("align")?;
        self.number()
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub fn convention(&mut self) -> Result<CallingConvention, ParseError> {
        match self.word()? {
            "whale" => Ok(CallingConvention::Whale),
            "sysv64" => Ok(CallingConvention::SysV64),
            _ => Err(self.previous_error("unknown calling convention")),
        }
    }

    #[cfg(any(not(whale_wave_ir), test))]
    pub fn signature(&mut self, depth: usize) -> Result<FunctionSignature, ParseError> {
        let convention = self.convention()?;
        self.expect("(")?;
        let params = self.types(")", depth)?;
        self.expect("->")?;
        let ret = self.ty(depth)?;
        Ok(FunctionSignature { params, ret, convention, variadic: false })
    }

    #[cfg(any(not(whale_wave_ir), test))]
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
        #[cfg(all(test, whale_wave_ir))]
        if self.reference_types {
            return self.ty_reference(depth);
        }
        #[cfg(whale_wave_ir)]
        {
            let parsed = super::wave_type::parse(
                &self.tokens,
                &self.raw_tokens,
                super::wave_type::Options {
                    start: self.at,
                    depth,
                    limits: self.limits,
                    remaining_nodes: self.limits.max_nodes.saturating_sub(self.nodes),
                    remaining_work: self.limits.max_analysis_work.saturating_sub(self.grammar_work),
                },
            )?;
            self.at = parsed.next;
            self.nodes += parsed.nodes;
            self.grammar_work += parsed.work;
            Ok(parsed.ty)
        }
        #[cfg(not(whale_wave_ir))]
        self.ty_reference(depth)
    }

    #[cfg(any(not(whale_wave_ir), test))]
    fn ty_reference(&mut self, depth: usize) -> Result<Type, ParseError> {
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
            },
            "fnptr" => {
                self.expect("<")?;
                let sig = self.signature(depth + 1)?;
                self.expect(">")?;
                Type::FnPtr(Box::new(sig))
            },
            "array" => {
                self.expect("<")?;
                let inner = self.ty(depth + 1)?;
                self.comma()?;
                let n = self.number()?;
                self.expect(">")?;
                Type::Array(Box::new(inner), n)
            },
            "struct" => {
                self.expect("{")?;
                Type::Struct(self.types("}", depth + 1)?)
            },
            "tuple" => {
                self.expect("<")?;
                Type::Tuple(self.types(">", depth + 1)?)
            },
            _ => return Err(self.previous_error("unknown type")),
        })
    }

    pub fn literal(&mut self, ty: &Type) -> Result<ConstValue, ParseError> {
        #[cfg(whale_wave_ir)]
        {
            #[cfg(test)]
            if self.reference_types {
                return self.literal_reference(ty);
            }
            let (value, work) = super::wave_literal::parse(
                self.peek(),
                ty,
                self.limits.max_analysis_work.saturating_sub(self.grammar_work),
                self.limits.max_analysis_work,
            )
            .map_err(|kind| ParseError { location: self.location(), kind })?;
            self.grammar_work += work;
            self.at += 1;
            Ok(value)
        }
        #[cfg(not(whale_wave_ir))]
        self.literal_reference(ty)
    }

    #[cfg(any(not(whale_wave_ir), test))]
    fn literal_reference(&mut self, ty: &Type) -> Result<ConstValue, ParseError> {
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
                ConstValue::I(s.parse().map_err(|_| self.error("signed literal out of range"))?)
            },
            Type::U1 | Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 => {
                if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(self.error("expected unsigned decimal literal"));
                }
                ConstValue::U(s.parse().map_err(|_| self.error("unsigned literal out of range"))?)
            },
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

    pub fn equal_type(&mut self, left: &Type, right: &Type) -> Result<(), ParseError> {
        #[cfg(whale_wave_ir)]
        let equal = {
            #[cfg(test)]
            if self.reference_types {
                return if left == right {
                    Ok(())
                } else {
                    Err(self.error("conflicting type annotations"))
                };
            }
            let (equal, work) = crate::verifier::wave_types_equal(
                left,
                right,
                self.limits.max_analysis_work.saturating_sub(self.grammar_work),
            )
            .map_err(|_| self.limit(Resource::AnalysisWork, self.limits.max_analysis_work))?;
            self.grammar_work += work;
            equal
        };
        #[cfg(not(whale_wave_ir))]
        let equal = left == right;
        if equal {
            Ok(())
        } else {
            Err(self.error("conflicting type annotations"))
        }
    }

    pub fn expression(&mut self, depth: usize) -> Result<ConstExpr, ParseError> {
        #[cfg(whale_wave_ir)]
        {
            #[cfg(test)]
            if self.reference_types {
                return self.expression_reference(depth);
            }
            let parsed = super::wave_expression::parse(
                &self.tokens,
                &self.raw_tokens,
                super::wave_expression::Options {
                    start: self.at,
                    depth,
                    format: self.format_version,
                    limits: self.limits,
                    remaining_nodes: self.limits.max_nodes.saturating_sub(self.nodes),
                    remaining_work: self.limits.max_analysis_work.saturating_sub(self.grammar_work),
                },
            )?;
            self.at = parsed.next;
            self.nodes += parsed.nodes;
            self.grammar_work += parsed.work;
            Ok(parsed.expression)
        }
        #[cfg(not(whale_wave_ir))]
        self.expression_reference(depth)
    }

    #[cfg(any(not(whale_wave_ir), test))]
    fn expression_reference(&mut self, depth: usize) -> Result<ConstExpr, ParseError> {
        if depth > self.limits.max_const_depth {
            return Err(self.limit(Resource::ConstantDepth, self.limits.max_const_depth));
        }
        self.node()?;
        let op = self.peek();
        if matches!(
            op,
            "bit_not"
                | "bit_and"
                | "bit_or"
                | "bit_xor"
                | "div"
                | "rem"
                | "shift_left"
                | "shift_right"
                | "logical_and"
                | "logical_or"
        ) && self.format_version < 5
        {
            return Err(self.error("extended constant operators require typed IR format 5"));
        }
        if op == "neg" && self.format_version < 8 {
            return Err(self.error("constant neg requires typed IR format 8"));
        }
        if op == "bit_not" || op == "neg" {
            let negative = op == "neg";
            self.word()?;
            self.expect("(")?;
            let value = Box::new(self.expression(depth + 1)?);
            self.expect(")")?;
            Ok(ConstExpr {
                ty: value.ty.clone(),
                kind: if negative { ConstExprKind::Neg(value) } else { ConstExprKind::Not(value) },
            })
        } else if matches!(
            op,
            "add"
                | "sub"
                | "mul"
                | "bit_and"
                | "bit_or"
                | "bit_xor"
                | "div"
                | "rem"
                | "shift_left"
                | "shift_right"
                | "logical_and"
                | "logical_or"
                | "eq"
                | "ne"
                | "lt"
                | "le"
                | "gt"
                | "ge"
        ) {
            let op = self.word()?;
            self.expect("(")?;
            let left = Box::new(self.expression(depth + 1)?);
            self.comma()?;
            let right = Box::new(self.expression(depth + 1)?);
            self.expect(")")?;
            if matches!(op, "eq" | "ne" | "lt" | "le" | "gt" | "ge") {
                Ok(ConstExpr {
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
                })
            } else {
                Ok(ConstExpr {
                    ty: left.ty.clone(),
                    kind: ConstExprKind::Binary {
                        op: match op {
                            "add" => ConstBinaryOp::Add,
                            "sub" => ConstBinaryOp::Sub,
                            "mul" => ConstBinaryOp::Mul,
                            "bit_and" => ConstBinaryOp::BitAnd,
                            "bit_or" => ConstBinaryOp::BitOr,
                            "bit_xor" => ConstBinaryOp::BitXor,
                            "div" => ConstBinaryOp::Div,
                            "rem" => ConstBinaryOp::Rem,
                            "shift_left" => ConstBinaryOp::ShiftLeft,
                            "shift_right" => ConstBinaryOp::ShiftRight,
                            "logical_and" => ConstBinaryOp::LogicalAnd,
                            _ => ConstBinaryOp::LogicalOr,
                        },
                        left,
                        right,
                    },
                })
            }
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
        #[cfg(all(test, whale_wave_ir))]
        if self.reference_types {
            return self.module_reference();
        }
        #[cfg(whale_wave_ir)]
        {
            self.module_wave()
        }
        #[cfg(not(whale_wave_ir))]
        {
            self.module_reference()
        }
    }

    #[cfg(any(not(whale_wave_ir), test))]
    fn module_reference(&mut self) -> Result<Module, ParseError> {
        #[cfg(whale_wave_ir)]
        let (target, datalayout) = {
            let (target, layout, version, next) = super::wave_header::parse(&self.tokens)?;
            self.format_version = version;
            self.at = next;
            (target, layout)
        };
        #[cfg(not(whale_wave_ir))]
        let (target, datalayout) = {
            self.expect("module")?;
            self.expect("{")?;
            self.expect("format_version")?;
            let version: u32 = self.number()?;
            if !(3..=IR_FORMAT_VERSION).contains(&version) {
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
            (target, DataLayout { ptr_bits, endian })
        };
        let mut module = Module::new(target, datalayout);
        while !self.eat("}") {
            self.node()?;
            match self.word()? {
                "source_file" => {
                    if self.format_version < 7 {
                        return Err(self.error("source metadata requires IR format 7"));
                    }
                    let id = self.id("@s")?;
                    let name = self.string()?;
                    module.sources.files.push(SourceFile { id, name });
                },
                "original" => {
                    if self.format_version < 7 {
                        return Err(self.error("source metadata requires IR format 7"));
                    }
                    let id = self.number::<u64>()?;
                    let span = if self.eat("none") {
                        None
                    } else {
                        Some(SourceSpan {
                            file: self.id("@s")?,
                            line: self.number()?,
                            column: self.number()?,
                            end_line: self.number()?,
                            end_column: self.number()?,
                        })
                    };
                    module.sources.originals.push(OriginalNode { id, span });
                },
                "source_map" => {
                    if self.format_version < 7 {
                        return Err(self.error("source metadata requires IR format 7"));
                    }
                    let target = match self.word()? {
                        "instruction" => SourceTarget::Instruction {
                            function: FunctionId(self.id("@f")?),
                            block: self.block_id()?,
                            index: self.number()?,
                        },
                        "terminator" => SourceTarget::Terminator {
                            function: FunctionId(self.id("@f")?),
                            block: self.block_id()?,
                        },
                        "global" => SourceTarget::Global(GlobalId(self.id("@g")?)),
                        _ => return Err(self.previous_error("unknown source mapping target")),
                    };
                    let original = self.number()?;
                    module.sources.mappings.push(SourceMapping { target, original });
                },
                "global" => {
                    let id = GlobalId(self.id("@g")?);
                    let location = self.tokens[self.at].location;
                    let name = self.string()?;
                    self.global_locations.entry(name.clone()).or_insert(location);
                    self.global_constant_locations.entry(id).or_insert(location);
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
                    module.globals.push(Global { id, name, ty, init, init_expr, align });
                },
                "storage" => {
                    if self.format_version < 6 {
                        return Err(
                            self.previous_error("global storage requires typed IR format 6")
                        );
                    }
                    let id = GlobalId(self.id("@g")?);
                    let location = self.location();
                    let name = self.string()?;
                    self.global_locations.entry(name.clone()).or_insert(location);
                    self.expect(":")?;
                    let ty = self.ty(0)?;
                    self.expect("=")?;
                    let init = match self.word()? {
                        "uninitialized" => GlobalStorageInit::Uninitialized,
                        "value" => {
                            let annotation = self.ty(0)?;
                            self.equal_type(&ty, &annotation)?;
                            GlobalStorageInit::Value(self.literal(&ty)?)
                        },
                        _ => {
                            return Err(self
                                .previous_error("expected value or uninitialized global storage"))
                        },
                    };
                    let align = self.align()?;
                    self.comma()?;
                    self.expect("access")?;
                    let readonly = match self.word()? {
                        "readonly" => true,
                        "readwrite" => false,
                        _ => return Err(self.previous_error("expected readonly or readwrite")),
                    };
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
                    module.global_storage.push(GlobalStorage {
                        id,
                        name,
                        ty,
                        init,
                        readonly,
                        align,
                        linkage,
                        link_name,
                    });
                },
                "declare" => {
                    let id = FunctionId(self.id("@f")?);
                    let location = self.tokens[self.at].location;
                    let name = self.string()?;
                    self.declaration_locations.entry(name.clone()).or_insert(location);
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
                },
                "fn" => module.functions.push(self.function()?),
                _ => return Err(self.previous_error("unknown module field")),
            }
        }
        Ok(module)
    }

    #[cfg(any(not(whale_wave_ir), test))]
    fn function(&mut self) -> Result<Function, ParseError> {
        let id = FunctionId(self.id("@f")?);
        let location = self.tokens[self.at].location;
        let name = self.string()?;
        self.function_locations.entry(name.clone()).or_insert(location);
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
        Ok(Function { id, name, params, ret_ty, entry, blocks, value_types })
    }
}
