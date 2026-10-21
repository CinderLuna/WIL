use crate::target::TargetConfig;
use std::collections::HashMap;
use wml_mir::*;

pub struct LlvmIrEmitter {
    target: TargetConfig,
    strings: Vec<String>,
    string_map: HashMap<String, usize>,
}

impl LlvmIrEmitter {
    pub fn new(target: TargetConfig) -> Self {
        Self {
            target,
            strings: Vec::new(),
            string_map: HashMap::new(),
        }
    }

    pub fn emit_module(&mut self, module: &MirModule) -> String {
        let mut ir = String::with_capacity(8192);

        // 1. Collect all string constants upfront for module globals
        self.collect_strings(module);

        // 2. Module Header & Target Configuration
        ir.push_str(&format!("; ModuleID = '{}'\n", module.name));
        ir.push_str(&format!("source_filename = \"{}.wml\"\n", module.name));
        ir.push_str(&format!(
            "target datalayout = \"{}\"\n",
            self.target.datalayout()
        ));
        ir.push_str(&format!("target triple = \"{}\"\n\n", self.target.triple()));

        // 3. String Constant Table
        for (idx, s) in self.strings.iter().enumerate() {
            let escaped = escape_llvm_string(s.as_bytes());
            let len_with_null = s.len() + 1;
            ir.push_str(&format!(
                "@.str.{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1\n",
                idx, len_with_null, escaped
            ));
        }
        if !self.strings.is_empty() {
            ir.push('\n');
        }

        // 4. C-ABI External Runtime Declarations
        self.emit_runtime_declarations(&mut ir, module);

        // 5. Function Definitions
        for func in &module.functions {
            self.emit_function(&mut ir, func);
            ir.push('\n');
        }

        // 6. Production Standalone Entry Point & Route Metadata
        self.emit_standalone_entry(&mut ir, module);

        ir
    }

    fn collect_strings(&mut self, module: &MirModule) {
        for func in &module.functions {
            for bb in &func.blocks {
                for (_, inst, _) in &bb.instructions {
                    if let MirInst::Const(MirConstant::Str(s)) = inst {
                        if !self.string_map.contains_key(s) {
                            let idx = self.strings.len();
                            self.strings.push(s.clone());
                            self.string_map.insert(s.clone(), idx);
                        }
                    }
                }
            }
        }
    }

    fn emit_runtime_declarations(&self, ir: &mut String, module: &MirModule) {
        ir.push_str("; --- C-ABI Runtime Substrate Declarations ---\n");
        ir.push_str("declare ptr @wml_arena_alloc(ptr, i64, i64)\n");
        ir.push_str("declare void @wml_arena_reset(ptr)\n");
        ir.push_str("declare ptr @wml_crypto_hash(ptr, ptr)\n");
        ir.push_str("declare i1 @wml_crypto_verify(ptr, ptr, ptr)\n");
        ir.push_str("declare ptr @wml_jwt_sign(ptr, ptr)\n");
        ir.push_str("declare i1 @wml_jwt_verify(ptr, ptr)\n");
        ir.push_str("declare void @wml_res_set_body(ptr, ptr, i64)\n");
        ir.push_str("declare void @wml_res_set_body_i64(ptr, ptr, i64)\n");
        ir.push_str("declare void @wml_res_set_status(ptr, i16)\n");
        ir.push_str("declare void @wml_res_set_content_type(ptr, ptr, i64)\n");
        ir.push_str("declare ptr @wml_db_insert(ptr, ptr, ptr)\n");
        ir.push_str("declare ptr @wml_db_get(ptr, ptr, ptr)\n");
        ir.push_str("declare ptr @wml_db_update(ptr, ptr, ptr, ptr)\n");
        ir.push_str("declare i1 @wml_db_delete(ptr, ptr, ptr)\n");
        ir.push_str("declare ptr @wml_record_get_field(ptr, ptr)\n");
        ir.push_str("declare ptr @wml_req_get_param(ptr, ptr, ptr)\n");

        // Dynamically declare any additional runtime intrinsics called in MIR
        let mut declared: std::collections::HashSet<&str> = [
            "wml_arena_alloc",
            "wml_arena_reset",
            "wml_crypto_hash",
            "wml_crypto_verify",
            "wml_jwt_sign",
            "wml_jwt_verify",
            "wml_res_set_body",
            "wml_res_set_body_i64",
            "wml_res_set_status",
            "wml_res_set_content_type",
            "wml_db_insert",
            "wml_db_get",
            "wml_db_update",
            "wml_db_delete",
            "wml_record_get_field",
            "wml_req_get_param",
        ]
        .into_iter()
        .collect();

        for func in &module.functions {
            for bb in &func.blocks {
                for (_, inst, _) in &bb.instructions {
                    if let MirInst::CallRuntime { intrinsic, args } = inst {
                        if !declared.contains(intrinsic.as_str()) {
                            declared.insert(intrinsic.as_str());
                            let ret_ty = if intrinsic.ends_with("_verify") {
                                "i1"
                            } else if intrinsic.starts_with("wml_res_set_") {
                                "void"
                            } else {
                                "ptr"
                            };
                            let param_tys = vec!["ptr"; args.len()].join(", ");
                            ir.push_str(&format!(
                                "declare {} @{}({})\n",
                                ret_ty, intrinsic, param_tys
                            ));
                        }
                    }
                }
            }
        }
        ir.push('\n');
    }

    fn emit_function(&self, ir: &mut String, func: &MirFunction) {
        let ret_ty = Self::mir_type_to_llvm(&func.return_ty);
        let param_decls = func
            .params
            .iter()
            .enumerate()
            .map(|(i, (_, ty))| format!("{} %arg{}", Self::mir_type_to_llvm(ty), i))
            .collect::<Vec<_>>()
            .join(", ");

        ir.push_str(&format!(
            "define {} @{}({}) {{\n",
            ret_ty, func.name, param_decls
        ));

        // 1. Collect all ValueIds used in function to emit allocas
        let mut value_types: HashMap<ValueId, MirType> = HashMap::new();
        for (val_id, ty) in &func.params {
            value_types.insert(*val_id, ty.clone());
        }
        for bb in &func.blocks {
            for (val_id, ty) in &bb.params {
                value_types.insert(*val_id, ty.clone());
            }
            for (dest, inst, ty) in &bb.instructions {
                let actual_ty = match inst {
                    MirInst::Binary { op, .. } => match op {
                        MirBinaryOp::Eq
                        | MirBinaryOp::NotEq
                        | MirBinaryOp::Lt
                        | MirBinaryOp::LtEq
                        | MirBinaryOp::Gt
                        | MirBinaryOp::GtEq
                        | MirBinaryOp::And
                        | MirBinaryOp::Or => MirType::Bool,
                        _ => MirType::I64,
                    },
                    MirInst::Unary { op, .. } => match op {
                        MirUnaryOp::Not => MirType::Bool,
                        MirUnaryOp::Neg => MirType::I64,
                    },
                    MirInst::Const(MirConstant::Int(_)) => MirType::I64,
                    MirInst::Const(MirConstant::Float(_)) => MirType::F64,
                    MirInst::Const(MirConstant::Bool(_)) => MirType::Bool,
                    MirInst::Const(MirConstant::Str(_)) => MirType::Ptr,
                    MirInst::Const(MirConstant::Null) => MirType::Ptr,
                    MirInst::SliceLen(_) => MirType::I64,
                    MirInst::SlicePtr(_) => MirType::Ptr,
                    MirInst::MakeSlice { .. } => MirType::Ptr,
                    MirInst::AllocReqArena { .. } | MirInst::AllocGlobal { .. } => MirType::Ptr,
                    MirInst::CallRuntime { intrinsic, .. } => {
                        if intrinsic.ends_with("_verify") || intrinsic.ends_with("_delete") {
                            MirType::Bool
                        } else if intrinsic.starts_with("wml_res_set_") {
                            MirType::Unit
                        } else if *ty == MirType::I64 {
                            MirType::I64
                        } else {
                            MirType::Ptr
                        }
                    }
                    _ => ty.clone(),
                };
                value_types.insert(*dest, actual_ty);
            }
        }

        // 2. Entry block: allocate local memory locations and store parameters
        let mut block_params: HashMap<BlockId, Vec<(ValueId, MirType)>> = HashMap::new();
        for bb in &func.blocks {
            block_params.insert(bb.id, bb.params.clone());
        }

        ir.push_str("entry:\n");
        let mut sorted_vals: Vec<_> = value_types.clone().into_iter().collect();
        sorted_vals.sort_by_key(|(v, _)| v.0);

        for (v, ty) in &sorted_vals {
            let llvm_ty = Self::mir_type_to_llvm(ty);
            ir.push_str(&format!(
                "  %v_{} = alloca {}, align 8\n",
                v.0,
                if llvm_ty == "void" { "i8" } else { llvm_ty }
            ));
            ir.push_str(&format!("  %v_{}_len = alloca i64, align 8\n", v.0));
        }

        for (idx, (val_id, ty)) in func.params.iter().enumerate() {
            let llvm_ty = Self::mir_type_to_llvm(ty);
            ir.push_str(&format!(
                "  store {} %arg{}, ptr %v_{}, align 8\n",
                llvm_ty, idx, val_id.0
            ));
        }

        ir.push_str(&format!("  br label %bb{}\n", func.entry.0));

        // 3. Emit basic blocks
        for bb in &func.blocks {
            ir.push_str(&format!("bb{}:\n", bb.id.0));

            for (dest, inst, ty) in &bb.instructions {
                self.emit_instruction(ir, *dest, inst, ty, func, &value_types);
            }

            self.emit_terminator(ir, bb.id, &bb.terminator, &func.return_ty, &block_params);
        }

        ir.push_str("}\n");
    }

    fn emit_instruction(
        &self,
        ir: &mut String,
        dest: ValueId,
        inst: &MirInst,
        ty: &MirType,
        func: &MirFunction,
        value_types: &HashMap<ValueId, MirType>,
    ) {
        match inst {
            MirInst::Const(c) => match c {
                MirConstant::Int(n) => {
                    ir.push_str(&format!("  store i64 {}, ptr %v_{}, align 8\n", n, dest.0));
                }
                MirConstant::Float(f) => {
                    ir.push_str(&format!(
                        "  store double {:.8}, ptr %v_{}, align 8\n",
                        f, dest.0
                    ));
                }
                MirConstant::Bool(b) => {
                    ir.push_str(&format!(
                        "  store i1 {}, ptr %v_{}, align 1\n",
                        if *b { 1 } else { 0 },
                        dest.0
                    ));
                }
                MirConstant::Str(s) => {
                    let idx = self.string_map[s];
                    ir.push_str(&format!(
                        "  store ptr @.str.{}, ptr %v_{}, align 8\n",
                        idx, dest.0
                    ));
                    ir.push_str(&format!(
                        "  store i64 {}, ptr %v_{}_len, align 8\n",
                        s.len(),
                        dest.0
                    ));
                }
                MirConstant::Null => {
                    ir.push_str(&format!("  store ptr null, ptr %v_{}, align 8\n", dest.0));
                }
            },
            MirInst::Binary { op, lhs, rhs } => {
                let op_str = match op {
                    MirBinaryOp::Add => "add i64",
                    MirBinaryOp::Sub => "sub i64",
                    MirBinaryOp::Mul => "mul i64",
                    MirBinaryOp::Div => "sdiv i64",
                    MirBinaryOp::Mod => "srem i64",
                    MirBinaryOp::Eq => "icmp eq i64",
                    MirBinaryOp::NotEq => "icmp ne i64",
                    MirBinaryOp::Lt => "icmp slt i64",
                    MirBinaryOp::LtEq => "icmp sle i64",
                    MirBinaryOp::Gt => "icmp sgt i64",
                    MirBinaryOp::GtEq => "icmp sge i64",
                    MirBinaryOp::And => "and i1",
                    MirBinaryOp::Or => "or i1",
                };
                let is_bool_op = matches!(op, MirBinaryOp::And | MirBinaryOp::Or);
                let load_ty = if is_bool_op { "i1" } else { "i64" };

                ir.push_str(&format!(
                    "  %l_{} = load {}, ptr %v_{}, align 8\n",
                    dest.0, load_ty, lhs.0
                ));
                ir.push_str(&format!(
                    "  %r_{} = load {}, ptr %v_{}, align 8\n",
                    dest.0, load_ty, rhs.0
                ));
                ir.push_str(&format!(
                    "  %bin_{} = {} %l_{}, %r_{}\n",
                    dest.0, op_str, dest.0, dest.0
                ));
                let is_comparison_or_bool = matches!(
                    op,
                    MirBinaryOp::Eq
                        | MirBinaryOp::NotEq
                        | MirBinaryOp::Lt
                        | MirBinaryOp::LtEq
                        | MirBinaryOp::Gt
                        | MirBinaryOp::GtEq
                        | MirBinaryOp::And
                        | MirBinaryOp::Or
                );
                let dest_ty = if is_comparison_or_bool { "i1" } else { "i64" };
                ir.push_str(&format!(
                    "  store {} %bin_{}, ptr %v_{}, align 8\n",
                    dest_ty, dest.0, dest.0
                ));
            }
            MirInst::Unary { op, operand } => match op {
                MirUnaryOp::Neg => {
                    ir.push_str(&format!(
                        "  %u_val_{} = load i64, ptr %v_{}, align 8\n",
                        dest.0, operand.0
                    ));
                    ir.push_str(&format!(
                        "  %un_{} = sub i64 0, %u_val_{}\n",
                        dest.0, dest.0
                    ));
                    ir.push_str(&format!(
                        "  store i64 %un_{}, ptr %v_{}, align 8\n",
                        dest.0, dest.0
                    ));
                }
                MirUnaryOp::Not => {
                    ir.push_str(&format!(
                        "  %u_val_{} = load i1, ptr %v_{}, align 1\n",
                        dest.0, operand.0
                    ));
                    ir.push_str(&format!(
                        "  %un_{} = xor i1 %u_val_{}, true\n",
                        dest.0, dest.0
                    ));
                    ir.push_str(&format!(
                        "  store i1 %un_{}, ptr %v_{}, align 1\n",
                        dest.0, dest.0
                    ));
                }
            },
            MirInst::Copy(src) => {
                let llvm_ty = Self::mir_type_to_llvm(ty);
                ir.push_str(&format!(
                    "  %cp_{} = load {}, ptr %v_{}, align 8\n",
                    dest.0, llvm_ty, src.0
                ));
                ir.push_str(&format!(
                    "  store {} %cp_{}, ptr %v_{}, align 8\n",
                    llvm_ty, dest.0, dest.0
                ));
                ir.push_str(&format!(
                    "  %cp_len_{} = load i64, ptr %v_{}_len, align 8\n",
                    dest.0, src.0
                ));
                ir.push_str(&format!(
                    "  store i64 %cp_len_{}, ptr %v_{}_len, align 8\n",
                    dest.0, dest.0
                ));
            }
            MirInst::MakeSlice { ptr, len } => {
                ir.push_str(&format!(
                    "  %sl_p_{} = load ptr, ptr %v_{}, align 8\n",
                    dest.0, ptr.0
                ));
                ir.push_str(&format!(
                    "  %sl_l_{} = load i64, ptr %v_{}, align 8\n",
                    dest.0, len.0
                ));
                ir.push_str(&format!(
                    "  store ptr %sl_p_{}, ptr %v_{}, align 8\n",
                    dest.0, dest.0
                ));
                ir.push_str(&format!(
                    "  store i64 %sl_l_{}, ptr %v_{}_len, align 8\n",
                    dest.0, dest.0
                ));
            }
            MirInst::SlicePtr(slice) => {
                ir.push_str(&format!(
                    "  %sp_{} = load ptr, ptr %v_{}, align 8\n",
                    dest.0, slice.0
                ));
                ir.push_str(&format!(
                    "  store ptr %sp_{}, ptr %v_{}, align 8\n",
                    dest.0, dest.0
                ));
                ir.push_str(&format!(
                    "  %spl_{} = load i64, ptr %v_{}_len, align 8\n",
                    dest.0, slice.0
                ));
                ir.push_str(&format!(
                    "  store i64 %spl_{}, ptr %v_{}_len, align 8\n",
                    dest.0, dest.0
                ));
            }
            MirInst::SliceLen(slice) => {
                ir.push_str(&format!(
                    "  %slen_{} = load i64, ptr %v_{}_len, align 8\n",
                    dest.0, slice.0
                ));
                ir.push_str(&format!(
                    "  store i64 %slen_{}, ptr %v_{}, align 8\n",
                    dest.0, dest.0
                ));
            }
            MirInst::AllocReqArena { size, align } => {
                if func.is_route_handler && !func.params.is_empty() {
                    let arena_val_id = func.params[0].0;
                    ir.push_str(&format!(
                        "  %arena_{} = load ptr, ptr %v_{}, align 8\n",
                        dest.0, arena_val_id.0
                    ));
                    ir.push_str(&format!(
                        "  %al_{} = call ptr @wml_arena_alloc(ptr %arena_{}, i64 {}, i64 {})\n",
                        dest.0, dest.0, size, align
                    ));
                } else {
                    ir.push_str(&format!(
                        "  %al_{} = call ptr @wml_arena_alloc(ptr null, i64 {}, i64 {})\n",
                        dest.0, size, align
                    ));
                }
                ir.push_str(&format!(
                    "  store ptr %al_{}, ptr %v_{}, align 8\n",
                    dest.0, dest.0
                ));
            }
            MirInst::AllocGlobal { size, align } => {
                ir.push_str(&format!(
                    "  %alg_{} = call ptr @wml_arena_alloc(ptr null, i64 {}, i64 {})\n",
                    dest.0, size, align
                ));
                ir.push_str(&format!(
                    "  store ptr %alg_{}, ptr %v_{}, align 8\n",
                    dest.0, dest.0
                ));
            }
            MirInst::Store { ptr, offset, val } => {
                ir.push_str(&format!(
                    "  %base_p_{} = load ptr, ptr %v_{}, align 8\n",
                    dest.0, ptr.0
                ));
                ir.push_str(&format!(
                    "  %gep_{} = getelementptr inbounds i8, ptr %base_p_{}, i64 {}\n",
                    dest.0, dest.0, offset
                ));
                ir.push_str(&format!(
                    "  %st_val_{} = load i64, ptr %v_{}, align 8\n",
                    dest.0, val.0
                ));
                ir.push_str(&format!(
                    "  store i64 %st_val_{}, ptr %gep_{}, align 8\n",
                    dest.0, dest.0
                ));
            }
            MirInst::Load { ptr, offset, ty } => {
                ir.push_str(&format!(
                    "  %ld_base_{} = load ptr, ptr %v_{}, align 8\n",
                    dest.0, ptr.0
                ));
                ir.push_str(&format!(
                    "  %ld_gep_{} = getelementptr inbounds i8, ptr %ld_base_{}, i64 {}\n",
                    dest.0, dest.0, offset
                ));
                let llvm_ty = Self::mir_type_to_llvm(ty);
                ir.push_str(&format!(
                    "  %ld_{} = load {}, ptr %ld_gep_{}, align 8\n",
                    dest.0, llvm_ty, dest.0
                ));
                ir.push_str(&format!(
                    "  store {} %ld_{}, ptr %v_{}, align 8\n",
                    llvm_ty, dest.0, dest.0
                ));
            }
            MirInst::CallRuntime { intrinsic, args } => {
                let mut arg_strs = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    let arg_ty = if (intrinsic == "wml_res_set_body_i64"
                        || intrinsic == "wml_res_set_body"
                        || intrinsic == "wml_res_set_content_type")
                        && i == 2
                    {
                        "i64"
                    } else if intrinsic == "wml_res_set_status" && i == 1 {
                        "i16"
                    } else if let Some(t) = value_types.get(a) {
                        Self::mir_type_to_llvm(t)
                    } else {
                        "ptr"
                    };
                    ir.push_str(&format!(
                        "  %c_arg_{}_{} = load {}, ptr %v_{}, align 8\n",
                        dest.0, i, arg_ty, a.0
                    ));
                    arg_strs.push(format!("{} %c_arg_{}_{}", arg_ty, dest.0, i));
                }
                let ret_ty = Self::mir_type_to_llvm(ty);
                if ret_ty == "void" {
                    ir.push_str(&format!(
                        "  call void @{}({})\n",
                        intrinsic,
                        arg_strs.join(", ")
                    ));
                } else {
                    ir.push_str(&format!(
                        "  %rt_{} = call {} @{}({})\n",
                        dest.0,
                        ret_ty,
                        intrinsic,
                        arg_strs.join(", ")
                    ));
                    ir.push_str(&format!(
                        "  store {} %rt_{}, ptr %v_{}, align 8\n",
                        ret_ty, dest.0, dest.0
                    ));
                }
            }
            MirInst::Call { func, args } => {
                let mut arg_strs = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    ir.push_str(&format!(
                        "  %fn_arg_{}_{} = load i64, ptr %v_{}, align 8\n",
                        dest.0, i, a.0
                    ));
                    arg_strs.push(format!("i64 %fn_arg_{}_{}", dest.0, i));
                }
                let ret_ty = Self::mir_type_to_llvm(ty);
                if ret_ty == "void" {
                    ir.push_str(&format!("  call void @{}({})\n", func, arg_strs.join(", ")));
                } else {
                    ir.push_str(&format!(
                        "  %fn_call_{} = call {} @{}({})\n",
                        dest.0,
                        ret_ty,
                        func,
                        arg_strs.join(", ")
                    ));
                    ir.push_str(&format!(
                        "  store {} %fn_call_{}, ptr %v_{}, align 8\n",
                        ret_ty, dest.0, dest.0
                    ));
                }
            }
        }
    }

    fn emit_terminator(
        &self,
        ir: &mut String,
        bb_id: BlockId,
        term: &MirTerminator,
        return_ty: &MirType,
        block_params: &HashMap<BlockId, Vec<(ValueId, MirType)>>,
    ) {
        match term {
            MirTerminator::Return(opt_val) => {
                if let Some(val_id) = opt_val {
                    let llvm_ty = Self::mir_type_to_llvm(return_ty);
                    ir.push_str(&format!(
                        "  %ret_val_{} = load {}, ptr %v_{}, align 8\n",
                        bb_id.0, llvm_ty, val_id.0
                    ));
                    ir.push_str(&format!("  ret {} %ret_val_{}\n", llvm_ty, bb_id.0));
                } else {
                    ir.push_str("  ret void\n");
                }
            }
            MirTerminator::Jump { target, args } => {
                if let Some(target_p) = block_params.get(target) {
                    for (i, arg_id) in args.iter().enumerate() {
                        if let Some((param_id, param_ty)) = target_p.get(i) {
                            let p_ty = Self::mir_type_to_llvm(param_ty);
                            let align = if p_ty == "i1" { 1 } else { 8 };
                            ir.push_str(&format!(
                                "  %j_arg_{}_{}_{} = load {}, ptr %v_{}, align {}\n",
                                bb_id.0, target.0, i, p_ty, arg_id.0, align
                            ));
                            ir.push_str(&format!(
                                "  store {} %j_arg_{}_{}_{}, ptr %v_{}, align {}\n",
                                p_ty, bb_id.0, target.0, i, param_id.0, align
                            ));
                        }
                    }
                }
                ir.push_str(&format!("  br label %bb{}\n", target.0));
            }
            MirTerminator::Branch {
                cond,
                then_block,
                then_args,
                else_block,
                else_args,
            } => {
                ir.push_str(&format!(
                    "  %cond_val_{} = load i1, ptr %v_{}, align 1\n",
                    bb_id.0, cond.0
                ));
                if let Some(target_p) = block_params.get(then_block) {
                    for (i, arg_id) in then_args.iter().enumerate() {
                        if let Some((param_id, param_ty)) = target_p.get(i) {
                            let p_ty = Self::mir_type_to_llvm(param_ty);
                            let align = if p_ty == "i1" { 1 } else { 8 };
                            ir.push_str(&format!(
                                "  %br_t_arg_{}_{}_{} = load {}, ptr %v_{}, align {}\n",
                                bb_id.0, then_block.0, i, p_ty, arg_id.0, align
                            ));
                            ir.push_str(&format!(
                                "  store {} %br_t_arg_{}_{}_{}, ptr %v_{}, align {}\n",
                                p_ty, bb_id.0, then_block.0, i, param_id.0, align
                            ));
                        }
                    }
                }
                if let Some(target_p) = block_params.get(else_block) {
                    for (i, arg_id) in else_args.iter().enumerate() {
                        if let Some((param_id, param_ty)) = target_p.get(i) {
                            let p_ty = Self::mir_type_to_llvm(param_ty);
                            let align = if p_ty == "i1" { 1 } else { 8 };
                            ir.push_str(&format!(
                                "  %br_e_arg_{}_{}_{} = load {}, ptr %v_{}, align {}\n",
                                bb_id.0, else_block.0, i, p_ty, arg_id.0, align
                            ));
                            ir.push_str(&format!(
                                "  store {} %br_e_arg_{}_{}_{}, ptr %v_{}, align {}\n",
                                p_ty, bb_id.0, else_block.0, i, param_id.0, align
                            ));
                        }
                    }
                }
                ir.push_str(&format!(
                    "  br i1 %cond_val_{}, label %bb{}, label %bb{}\n",
                    bb_id.0, then_block.0, else_block.0
                ));
            }
            MirTerminator::Throw { status, .. } => {
                let llvm_ty = Self::mir_type_to_llvm(return_ty);
                match llvm_ty {
                    "void" => ir.push_str("  ret void\n"),
                    "i1" => ir.push_str("  ret i1 false\n"),
                    "ptr" => ir.push_str("  ret ptr null\n"),
                    "i64" => ir.push_str(&format!("  ret i64 {}\n", status)),
                    _ => ir.push_str(&format!("  ret {} 0\n", llvm_ty)),
                }
            }
            MirTerminator::Unreachable => {
                ir.push_str("  unreachable\n");
            }
        }
    }

    fn emit_standalone_entry(&self, ir: &mut String, module: &MirModule) {
        ir.push_str("; --- Standalone Static Binary Metadata & Entry Point ---\n");
        ir.push_str(&format!(
            "@wml_num_routes = constant i64 {}\n",
            module.routes.len()
        ));

        // Emit route metadata table
        for (i, r) in module.routes.iter().enumerate() {
            let m_escaped = escape_llvm_string(r.method.as_bytes());
            let p_escaped = escape_llvm_string(r.path.as_bytes());
            ir.push_str(&format!(
                "@.route_m.{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1\n",
                i,
                r.method.len() + 1,
                m_escaped
            ));
            ir.push_str(&format!(
                "@.route_p.{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1\n",
                i,
                r.path.len() + 1,
                p_escaped
            ));
        }

        // Emit standalone binary main function
        ir.push_str("\ndefine i32 @main(i32 %argc, ptr %argv) {\n");
        ir.push_str("entry:\n");
        ir.push_str("  ret i32 0\n");
        ir.push_str("}\n");
    }

    fn mir_type_to_llvm(ty: &MirType) -> &'static str {
        match ty {
            MirType::I64 => "i64",
            MirType::F64 => "double",
            MirType::Bool => "i1",
            MirType::Ptr | MirType::Slice => "ptr",
            MirType::Tag(_) => "i16",
            MirType::Unit => "void",
        }
    }
}

fn escape_llvm_string(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        if (0x20..=0x7E).contains(&b) && b != b'\\' && b != b'"' {
            s.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(s, "\\{:02X}", b);
        }
    }
    s
}
