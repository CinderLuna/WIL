use crate::ir::*;
use std::collections::{HashMap, HashSet};

pub struct MirOptimizer;

impl Default for MirOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl MirOptimizer {
    pub fn new() -> Self {
        Self
    }

    pub fn optimize_module(&self, module: &mut MirModule) {
        for func in &mut module.functions {
            self.optimize_function(func);
        }
    }

    pub fn optimize_function(&self, func: &mut MirFunction) {
        self.fold_constants(func);
        self.eliminate_dead_blocks(func);
    }

    fn fold_constants(&self, func: &mut MirFunction) {
        let mut consts: HashMap<ValueId, MirConstant> = HashMap::new();

        for bb in &mut func.blocks {
            for (dest, inst, _) in &mut bb.instructions {
                match inst {
                    MirInst::Const(c) => {
                        consts.insert(*dest, c.clone());
                    }
                    MirInst::Binary { op, lhs, rhs } => {
                        if let (Some(MirConstant::Int(a)), Some(MirConstant::Int(b))) =
                            (consts.get(lhs), consts.get(rhs))
                        {
                            let folded = match op {
                                MirBinaryOp::Add => Some(MirConstant::Int(a.wrapping_add(*b))),
                                MirBinaryOp::Sub => Some(MirConstant::Int(a.wrapping_sub(*b))),
                                MirBinaryOp::Mul => Some(MirConstant::Int(a.wrapping_mul(*b))),
                                MirBinaryOp::Div if *b != 0 => {
                                    Some(MirConstant::Int(a.wrapping_div(*b)))
                                }
                                MirBinaryOp::Mod if *b != 0 => {
                                    Some(MirConstant::Int(a.wrapping_rem(*b)))
                                }
                                MirBinaryOp::Eq => Some(MirConstant::Bool(a == b)),
                                MirBinaryOp::NotEq => Some(MirConstant::Bool(a != b)),
                                MirBinaryOp::Lt => Some(MirConstant::Bool(a < b)),
                                MirBinaryOp::LtEq => Some(MirConstant::Bool(a <= b)),
                                MirBinaryOp::Gt => Some(MirConstant::Bool(a > b)),
                                MirBinaryOp::GtEq => Some(MirConstant::Bool(a >= b)),
                                _ => None,
                            };
                            if let Some(c) = folded {
                                consts.insert(*dest, c.clone());
                                *inst = MirInst::Const(c);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn eliminate_dead_blocks(&self, func: &mut MirFunction) {
        let mut reachable = HashSet::new();
        let mut queue = vec![func.entry];
        reachable.insert(func.entry);

        let block_map: HashMap<BlockId, &BasicBlock> =
            func.blocks.iter().map(|b| (b.id, b)).collect();

        while let Some(current_id) = queue.pop() {
            if let Some(bb) = block_map.get(&current_id) {
                match &bb.terminator {
                    MirTerminator::Jump { target, .. } if reachable.insert(*target) => {
                        queue.push(*target);
                    }
                    MirTerminator::Branch {
                        then_block,
                        else_block,
                        ..
                    } => {
                        if reachable.insert(*then_block) {
                            queue.push(*then_block);
                        }
                        if reachable.insert(*else_block) {
                            queue.push(*else_block);
                        }
                    }
                    _ => {}
                }
            }
        }

        func.blocks.retain(|b| reachable.contains(&b.id));
    }
}
