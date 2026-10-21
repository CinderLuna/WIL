use crate::ir::*;
use std::collections::HashSet;

pub struct EscapeAnalysis;

#[derive(Debug, Clone, Default)]
pub struct EscapeReport {
    pub arena_allocs: usize,
    pub global_allocs: usize,
    pub promoted_to_global: usize,
}

impl Default for EscapeAnalysis {
    fn default() -> Self {
        Self::new()
    }
}

impl EscapeAnalysis {
    pub fn new() -> Self {
        Self
    }

    /// Analyzes function allocations and promotes escaping values from RequestArena to Global Heap.
    pub fn run_on_module(&self, module: &mut MirModule) -> EscapeReport {
        let mut report = EscapeReport::default();
        for func in &mut module.functions {
            let func_report = self.run_on_function(func);
            report.arena_allocs += func_report.arena_allocs;
            report.global_allocs += func_report.global_allocs;
            report.promoted_to_global += func_report.promoted_to_global;
        }
        report
    }

    pub fn run_on_function(&self, func: &mut MirFunction) -> EscapeReport {
        let mut report = EscapeReport::default();
        let mut escaping_values = HashSet::new();

        // Pass 1: Identify values that escape via global persistence calls
        for bb in &func.blocks {
            for (_, inst, _) in &bb.instructions {
                match inst {
                    MirInst::CallRuntime { intrinsic, args }
                        if intrinsic.contains("global")
                            || intrinsic.contains("persist")
                            || intrinsic.contains("cache_set") =>
                    {
                        for arg in args {
                            escaping_values.insert(*arg);
                        }
                    }
                    MirInst::Store { ptr, val, .. } if escaping_values.contains(ptr) => {
                        escaping_values.insert(*val);
                    }
                    _ => {}
                }
            }

            // In non-route functions, values returned to the caller escape the function frame
            if !func.is_route_handler {
                if let MirTerminator::Return(Some(val)) = bb.terminator {
                    escaping_values.insert(val);
                }
            }
        }

        // Pass 2: Bidirectional propagation across copies, slices, and block arguments
        let block_map: std::collections::HashMap<BlockId, &BasicBlock> =
            func.blocks.iter().map(|b| (b.id, b)).collect();

        let mut changed = true;
        while changed {
            changed = false;
            for bb in &func.blocks {
                for (dest, inst, _) in &bb.instructions {
                    match inst {
                        MirInst::Copy(src) => {
                            if escaping_values.contains(src) && escaping_values.insert(*dest) {
                                changed = true;
                            }
                            if escaping_values.contains(dest) && escaping_values.insert(*src) {
                                changed = true;
                            }
                        }
                        MirInst::MakeSlice { ptr, .. } => {
                            if escaping_values.contains(ptr) && escaping_values.insert(*dest) {
                                changed = true;
                            }
                            if escaping_values.contains(dest) && escaping_values.insert(*ptr) {
                                changed = true;
                            }
                        }
                        MirInst::SlicePtr(slice) => {
                            if escaping_values.contains(slice) && escaping_values.insert(*dest) {
                                changed = true;
                            }
                            if escaping_values.contains(dest) && escaping_values.insert(*slice) {
                                changed = true;
                            }
                        }
                        _ => {}
                    }
                }

                // Propagate across block arguments on branch/jump boundaries
                let mut check_jump_args = |target_id: BlockId, args: &[ValueId]| {
                    if let Some(target_bb) = block_map.get(&target_id) {
                        for (i, (param_val, _)) in target_bb.params.iter().enumerate() {
                            if let Some(&arg_val) = args.get(i) {
                                if escaping_values.contains(param_val)
                                    && escaping_values.insert(arg_val)
                                {
                                    changed = true;
                                }
                                if escaping_values.contains(&arg_val)
                                    && escaping_values.insert(*param_val)
                                {
                                    changed = true;
                                }
                            }
                        }
                    }
                };

                match &bb.terminator {
                    MirTerminator::Jump { target, args } => {
                        check_jump_args(*target, args);
                    }
                    MirTerminator::Branch {
                        then_block,
                        then_args,
                        else_block,
                        else_args,
                        ..
                    } => {
                        check_jump_args(*then_block, then_args);
                        check_jump_args(*else_block, else_args);
                    }
                    _ => {}
                }
            }
        }

        // Pass 3: Transform escaping AllocReqArena into AllocGlobal
        for bb in &mut func.blocks {
            for (dest, inst, _) in &mut bb.instructions {
                match inst {
                    MirInst::AllocReqArena { size, align } => {
                        if escaping_values.contains(dest) {
                            *inst = MirInst::AllocGlobal {
                                size: *size,
                                align: *align,
                            };
                            report.promoted_to_global += 1;
                            report.global_allocs += 1;
                        } else {
                            report.arena_allocs += 1;
                        }
                    }
                    MirInst::AllocGlobal { .. } => {
                        report.global_allocs += 1;
                    }
                    _ => {}
                }
            }
        }

        report
    }
}
