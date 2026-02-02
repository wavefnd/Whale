#[derive(Debug, Clone)]
pub struct AST {
    pub items: Vec<ASTNode>,
}

#[derive(Debug, Clone)]
pub enum ASTNode {
    Instruction(Instruction),
    Directive(Directive),
    Label(String),
    Section(String),
    Global(String),
    Extern(String),
}

#[derive(Debug, Clone)]
pub struct Instruction {
    pub mnemonic: String,
    pub operands: Vec<Operand>,
}

#[derive(Debug, Clone)]
pub struct Directive {
    pub name: String,
    pub values: Vec<DirectiveValue>,
}

#[derive(Debug, Clone)]
pub enum DirectiveValue {
    Number(i64),
    StringLiteral(String),
    Identifier(String),
}

#[derive(Debug, Clone)]
pub enum Operand {
    Register(String),
    Immediate(i64),
    Label(String),
    Memory(MemoryOperand)
}

#[derive(Debug, Clone)]
pub struct MemoryOperand {
    pub base: Option<String>,
    pub index: Option<String>,
    pub scale: u8,
    pub disp: i64,
}