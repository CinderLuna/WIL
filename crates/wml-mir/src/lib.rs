#[derive(Debug, Clone)]
pub struct MirModule {
    pub name: String,
    pub functions: Vec<MirFunction>,
}

#[derive(Debug, Clone)]
pub struct MirFunction {
    pub name: String,
    pub blocks: Vec<BasicBlock>,
}

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: usize,
    pub instructions: Vec<MirInst>,
}

#[derive(Debug, Clone)]
pub enum MirInst {
    AllocReqArena { size: usize, dest: usize },
    AllocGlobal { size: usize, dest: usize },
    HttpWriteChunk { stream_id: usize, buffer_id: usize },
    Return { val: Option<usize> },
}
