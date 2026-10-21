use la_arena::{Arena, Idx};

pub type ExprId = Idx<Expr>;
pub type StmtId = Idx<Stmt>;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Ident(String),
    Binary { op: BinaryOp, lhs: ExprId, rhs: ExprId },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    String(String),
    Int(i64),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Coalesce,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let { name: String, val: ExprId },
    Return(Option<ExprId>),
}

#[derive(Default, Debug)]
pub struct AstArena {
    pub exprs: Arena<Expr>,
    pub stmts: Arena<Stmt>,
}
