use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ValueId(pub usize);

impl fmt::Display for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub usize);

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirType {
    I64,
    F64,
    Bool,
    Ptr,
    Slice,
    Tag(u16),
    Unit,
}

impl fmt::Display for MirType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MirType::I64 => write!(f, "i64"),
            MirType::F64 => write!(f, "f64"),
            MirType::Bool => write!(f, "bool"),
            MirType::Ptr => write!(f, "ptr"),
            MirType::Slice => write!(f, "slice"),
            MirType::Tag(t) => write!(f, "tag({})", t),
            MirType::Unit => write!(f, "unit"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirConstant {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Null,
}

impl fmt::Display for MirConstant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MirConstant::Int(i) => write!(f, "{}", i),
            MirConstant::Float(fl) => write!(f, "{}", fl),
            MirConstant::Bool(b) => write!(f, "{}", b),
            MirConstant::Str(s) => write!(f, "\"{}\"", s),
            MirConstant::Null => write!(f, "null"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirBinaryOp {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirUnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirInst {
    Const(MirConstant),
    Binary {
        op: MirBinaryOp,
        lhs: ValueId,
        rhs: ValueId,
    },
    Unary {
        op: MirUnaryOp,
        operand: ValueId,
    },
    AllocReqArena {
        size: usize,
        align: usize,
    },
    AllocGlobal {
        size: usize,
        align: usize,
    },
    Load {
        ptr: ValueId,
        offset: u32,
        ty: MirType,
    },
    Store {
        ptr: ValueId,
        offset: u32,
        val: ValueId,
    },
    MakeSlice {
        ptr: ValueId,
        len: ValueId,
    },
    SlicePtr(ValueId),
    SliceLen(ValueId),
    CallRuntime {
        intrinsic: String,
        args: Vec<ValueId>,
    },
    Call {
        func: String,
        args: Vec<ValueId>,
    },
    Copy(ValueId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirTerminator {
    Return(Option<ValueId>),
    Jump {
        target: BlockId,
        args: Vec<ValueId>,
    },
    Branch {
        cond: ValueId,
        then_block: BlockId,
        then_args: Vec<ValueId>,
        else_block: BlockId,
        else_args: Vec<ValueId>,
    },
    Throw {
        status: u16,
        message: Option<ValueId>,
    },
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BasicBlock {
    pub id: BlockId,
    pub params: Vec<(ValueId, MirType)>,
    pub instructions: Vec<(ValueId, MirInst, MirType)>,
    pub terminator: MirTerminator,
}

impl BasicBlock {
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            params: Vec::new(),
            instructions: Vec::new(),
            terminator: MirTerminator::Unreachable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirFunction {
    pub name: String,
    pub params: Vec<(ValueId, MirType)>,
    pub return_ty: MirType,
    pub blocks: Vec<BasicBlock>,
    pub entry: BlockId,
    pub is_route_handler: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirRoute {
    pub method: String,
    pub path: String,
    pub handler_fn: String,
    pub auth: bool,
    pub rate_limit: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirModule {
    pub name: String,
    pub functions: Vec<MirFunction>,
    pub routes: Vec<MirRoute>,
}

impl MirModule {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            functions: Vec::new(),
            routes: Vec::new(),
        }
    }
}

impl fmt::Display for MirFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let params: Vec<String> = self
            .params
            .iter()
            .map(|(v, ty)| format!("{}: {}", v, ty))
            .collect();
        writeln!(
            f,
            "fn {}({}) -> {} {{",
            self.name,
            params.join(", "),
            self.return_ty
        )?;
        for bb in &self.blocks {
            let bb_params: Vec<String> = bb
                .params
                .iter()
                .map(|(v, ty)| format!("{}: {}", v, ty))
                .collect();
            let param_str = if bb_params.is_empty() {
                String::new()
            } else {
                format!("({})", bb_params.join(", "))
            };
            writeln!(f, "  {}{}:", bb.id, param_str)?;
            for (dest, inst, ty) in &bb.instructions {
                writeln!(f, "    {}: {} = {:?}", dest, ty, inst)?;
            }
            writeln!(f, "    {:?}", bb.terminator)?;
        }
        writeln!(f, "}}")
    }
}
