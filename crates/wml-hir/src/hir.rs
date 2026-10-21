use crate::types::Type;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Range;
pub use wml_syntax::ast::{BinaryOp, HttpMethod, Literal, UnaryOp};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirProgram {
    pub types: HashMap<String, HirTypeDecl>,
    pub schemas: HashMap<String, HirSchemaDecl>,
    pub routes: Vec<HirRouteDecl>,
    pub pages: Vec<HirPageDecl>,
    pub uis: Vec<HirUiDecl>,
    pub fns: Vec<HirFnDecl>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirTypeDecl {
    pub name: String,
    pub fields: Vec<HirFieldDef>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirFieldDef {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirSchemaDecl {
    pub name: String,
    pub fields: Vec<HirSchemaField>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirSchemaField {
    pub name: String,
    pub ty: Type,
    pub is_pk: bool,
    pub is_unique: bool,
    pub is_indexed: bool,
    pub is_auto: bool,
    pub has_default: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirRouteDecl {
    pub path: String,
    pub method: Option<HttpMethod>,
    pub payload_type: Option<Type>,
    pub auth: bool,
    pub rate_limit: Option<i64>,
    pub return_type: Type,
    pub body: Vec<HirStmt>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirPageDecl {
    pub path: String,
    pub auth: bool,
    pub rate_limit: Option<i64>,
    pub body: Vec<HirStmt>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirUiDecl {
    pub name: String,
    pub params: Vec<HirParam>,
    pub body: Vec<HirStmt>,
    pub actions: Vec<HirActionDecl>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirActionDecl {
    pub name: String,
    pub params: Vec<HirParam>,
    pub return_type: Type,
    pub body: Vec<HirStmt>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirFnDecl {
    pub name: String,
    pub params: Vec<HirParam>,
    pub return_type: Type,
    pub body: Vec<HirStmt>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirParam {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirStmt {
    Let {
        name: String,
        ty: Type,
        val: HirTypedExpr,
        span: Range<usize>,
    },
    Return {
        val: Option<HirTypedExpr>,
        span: Range<usize>,
    },
    Throw {
        code: i64,
        message: Option<HirTypedExpr>,
        span: Range<usize>,
    },
    If {
        cond: HirTypedExpr,
        then_branch: Vec<HirStmt>,
        else_branch: Option<Vec<HirStmt>>,
        span: Range<usize>,
    },
    For {
        var: String,
        iter: HirTypedExpr,
        body: Vec<HirStmt>,
        span: Range<usize>,
    },
    Expr(HirTypedExpr),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirTypedExpr {
    pub kind: HirExpr,
    pub ty: Type,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirExpr {
    Literal(Literal),
    Var(String),
    Binary {
        op: BinaryOp,
        lhs: Box<HirTypedExpr>,
        rhs: Box<HirTypedExpr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<HirTypedExpr>,
    },
    Call {
        callee: Box<HirTypedExpr>,
        args: Vec<HirCallArg>,
    },
    MemberAccess {
        object: Box<HirTypedExpr>,
        field: String,
    },
    ObjectLit(Vec<(String, HirTypedExpr)>),
    ArrayLit(Vec<HirTypedExpr>),
    Coalesce {
        lhs: Box<HirTypedExpr>,
        rhs: Box<HirTypedExpr>,
    },
    Throw {
        code: i64,
        message: Option<Box<HirTypedExpr>>,
    },
    Jsx(HirJsxElement),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirCallArg {
    Positional(HirTypedExpr),
    Named { name: String, val: HirTypedExpr },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirJsxElement {
    pub tag: String,
    pub attrs: Vec<(String, Option<HirTypedExpr>)>,
    pub children: Vec<HirJsxChild>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirJsxChild {
    Element(HirJsxElement),
    Expr(HirTypedExpr),
    Text(String),
}
