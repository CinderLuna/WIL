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

        // Pass 2: Forward propagation of escaping set across copies/slices
        let mut changed = true;
        while changed {
            changed = false;
            for bb in &func.blocks {
                for (dest, inst, _) in &bb.instructions {
                    match inst {
                        MirInst::Copy(src)
                            if escaping_values.contains(src) && escaping_values.insert(*dest) =>
                        {
                            changed = true;
                        }
                        MirInst::MakeSlice { ptr, .. }
                            if escaping_values.contains(ptr) && escaping_values.insert(*dest) =>
                        {
                            changed = true;
                        }
                        _ => {}
                    }
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
