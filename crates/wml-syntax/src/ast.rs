use la_arena::{Arena, Idx};
use serde::{Deserialize, Serialize};

pub type ExprId = Idx<Expr>;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Type(TypeDecl),
    Schema(SchemaDecl),
    Route(RouteDecl),
    Page(PageDecl),
    Ui(UiDecl),
    Fn(FnDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,
    pub ty: TypeRef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaDecl {
    pub name: String,
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaField {
    pub name: String,
    pub ty: TypeRef,
    pub modifiers: Vec<FieldModifier>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldModifier {
    Pk,
    Unique,
    Indexed,
    Auto,
    Default(ExprId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteDecl {
    pub path: String,
    pub config: RouteConfig,
    pub return_type: Option<TypeRef>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RouteConfig {
    pub method: Option<HttpMethod>,
    pub payload_type: Option<TypeRef>,
    pub auth: Option<bool>,
    pub rate_limit: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    Post,
    Get,
    Put,
    Delete,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageDecl {
    pub path: String,
    pub config: RouteConfig,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: TypeRef,
    pub default: Option<ExprId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeRef {
    Int,
    Float,
    Str,
    Bool,
    Any,
    Void,
    Custom(String),
    Array(Box<TypeRef>),
    Optional(Box<TypeRef>),
    Record(Vec<FieldDef>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        ty: Option<TypeRef>,
        val: ExprId,
    },
    Return(Option<ExprId>),
    Throw {
        code: i64,
        message: Option<ExprId>,
    },
    Action {
        name: String,
        params: Vec<Param>,
        return_type: Option<TypeRef>,
        body: Block,
    },
    If {
        cond: ExprId,
        then_branch: Block,
        else_branch: Option<ElseBranch>,
    },
    For {
        var: String,
        iter: ExprId,
        body: Block,
    },
    Expr(ExprId),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElseBranch {
    Block(Block),
    If(Box<Stmt>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Ident(String),
    Binary {
        op: BinaryOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    Unary {
        op: UnaryOp,
        expr: ExprId,
    },
    Call {
        callee: ExprId,
        args: Vec<CallArg>,
    },
    MemberAccess {
        object: ExprId,
        field: String,
    },
    ObjectLit(Vec<ObjectField>),
    ArrayLit(Vec<ExprId>),
    Throw {
        code: i64,
        message: Option<ExprId>,
    },
    Jsx(Box<JsxElement>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    Coalesce,
    AddAssign,
    SubAssign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallArg {
    Positional(ExprId),
    Named { name: String, val: ExprId },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectField {
    pub name: String,
    pub val: Option<ExprId>, // None if shorthand { email }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsxElement {
    pub tag: String,
    pub attrs: Vec<JsxAttr>,
    pub children: Vec<JsxChild>,
    pub span: std::ops::Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsxAttr {
    pub name: String,
    pub value: Option<JsxAttrValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JsxAttrValue {
    String(String),
    Expr(ExprId),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub enum JsxChild {
    Element(JsxElement),
    Expr(ExprId),
    Text(String),
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct AstArena {
    pub exprs: Arena<Expr>,
    pub spans: Vec<std::ops::Range<usize>>,
}

impl AstArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc_expr(&mut self, expr: Expr, span: std::ops::Range<usize>) -> ExprId {
        let id = self.exprs.alloc(expr);
        let idx = id.into_raw().into_u32() as usize;
        if idx >= self.spans.len() {
            self.spans.resize(idx + 1, 0..0);
        }
        self.spans[idx] = span;
        id
    }

    pub fn get_expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id]
    }

    pub fn get_span(&self, id: ExprId) -> std::ops::Range<usize> {
        let idx = id.into_raw().into_u32() as usize;
        self.spans.get(idx).cloned().unwrap_or(0..0)
    }
}
