use cranelift_codegen::ir::{
    condcodes::IntCC, types, AbiParam, Block, BlockArg, Function, InstBuilder, MemFlagsData, Value,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::collections::HashMap;
use thiserror::Error;
use wml_mir::*;
use wml_runtime::abi::*;

#[derive(Error, Debug)]
pub enum CodegenError {
    #[error("Cranelift native builder error: {0}")]
    NativeBuilder(String),
    #[error("Cranelift module error: {0}")]
    Module(String),
    #[error("Unknown function symbol: {0}")]
    UnknownFunction(String),
}

impl From<cranelift_module::ModuleError> for CodegenError {
    fn from(e: cranelift_module::ModuleError) -> Self {
        CodegenError::Module(e.to_string())
    }
}

pub struct CompiledRoute {
    pub method: String,
    pub path: String,
    pub fn_ptr: *const u8,
}

pub struct CompiledModule {
    pub routes: Vec<CompiledRoute>,
    pub functions: HashMap<String, *const u8>,
}

pub struct CraneliftJitBackend {
    module: JITModule,
    fn_builder_ctx: FunctionBuilderContext,
    string_pool: Vec<Box<[u8]>>,
}

impl Default for CraneliftJitBackend {
    fn default() -> Self {
        Self::new().expect("Failed to create CraneliftJitBackend")
    }
}

unsafe extern "C" fn wml_generic_runtime_fallback() -> *mut u8 {
    std::ptr::null_mut()
}

impl CraneliftJitBackend {
    pub fn new() -> Result<Self, CodegenError> {
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        flag_builder.set("opt_level", "speed").unwrap();
        let flags = settings::Flags::new(flag_builder);

        let isa_builder =
            cranelift_native::builder().map_err(|e| CodegenError::NativeBuilder(e.to_string()))?;
        let isa = isa_builder
            .finish(flags)
            .map_err(|e| CodegenError::NativeBuilder(e.to_string()))?;

        let mut jit_builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        // Register runtime C-ABI entry points
        jit_builder.symbol("wml_arena_alloc", wml_arena_alloc as *const u8);
        jit_builder.symbol("wml_arena_reset", wml_arena_reset as *const u8);
        jit_builder.symbol("wml_crypto_hash", wml_crypto_hash as *const u8);
        jit_builder.symbol("wml_crypto_verify", wml_crypto_verify as *const u8);
        jit_builder.symbol("wml_jwt_sign", wml_jwt_sign as *const u8);
        jit_builder.symbol("wml_jwt_verify", wml_jwt_verify as *const u8);
        jit_builder.symbol("wml_res_set_body", wml_res_set_body as *const u8);
        jit_builder.symbol("wml_res_set_status", wml_res_set_status as *const u8);
        jit_builder.symbol(
            "wml_res_set_content_type",
            wml_res_set_content_type as *const u8,
        );
        jit_builder.symbol("wml_res_set_body_i64", wml_res_set_body_i64 as *const u8);
        jit_builder.symbol("wml_db_insert", wml_db_insert as *const u8);
        jit_builder.symbol("wml_db_get", wml_db_get as *const u8);
        jit_builder.symbol("wml_record_get_field", wml_record_get_field as *const u8);
        jit_builder.symbol("wml_req_get_param", wml_req_get_param as *const u8);
        jit_builder.symbol("wml_db_update", wml_db_update as *const u8);
        jit_builder.symbol("wml_db_delete", wml_db_delete as *const u8);

        jit_builder.symbol_lookup_fn(Box::new(|name| {
            if name.starts_with("wml_") {
                Some(wml_generic_runtime_fallback as *const u8)
            } else {
                None
            }
        }));

        let module = JITModule::new(jit_builder);

        Ok(Self {
            module,
            fn_builder_ctx: FunctionBuilderContext::new(),
            string_pool: Vec::new(),
        })
    }

    pub fn compile_module(&mut self, mir: &MirModule) -> Result<CompiledModule, CodegenError> {
        let mut declared_funcs: HashMap<String, FuncId> = HashMap::new();
        let ptr_type = self.module.target_config().pointer_type();

        // Register runtime C-ABI intrinsic declarations
        let runtime_decls: &[(&str, &[types::Type], Option<types::Type>)] = &[
            (
                "wml_arena_alloc",
                &[ptr_type, types::I64, types::I64],
                Some(ptr_type),
            ),
            ("wml_arena_reset", &[ptr_type], None),
            ("wml_crypto_hash", &[ptr_type, ptr_type], Some(ptr_type)),
            (
                "wml_crypto_verify",
                &[ptr_type, ptr_type, ptr_type],
                Some(types::I8),
            ),
            ("wml_jwt_sign", &[ptr_type, ptr_type], Some(ptr_type)),
            ("wml_jwt_verify", &[ptr_type, ptr_type], Some(types::I8)),
            ("wml_res_set_body", &[ptr_type, ptr_type, types::I64], None),
            ("wml_res_set_status", &[ptr_type, types::I16], None),
            (
                "wml_res_set_content_type",
                &[ptr_type, ptr_type, types::I64],
                None,
            ),
            (
                "wml_db_insert",
                &[ptr_type, ptr_type, ptr_type],
                Some(ptr_type),
            ),
            (
                "wml_db_get",
                &[ptr_type, ptr_type, ptr_type],
                Some(ptr_type),
            ),
            (
                "wml_record_get_field",
                &[ptr_type, ptr_type],
                Some(ptr_type),
            ),
            (
                "wml_req_get_param",
                &[ptr_type, ptr_type, ptr_type],
                Some(ptr_type),
            ),
            (
                "wml_res_set_body_i64",
                &[ptr_type, ptr_type, types::I64],
                None,
            ),
        ];

        for &(name, param_types, ret_type) in runtime_decls {
            let mut sig = self.module.make_signature();
            for &pt in param_types {
                sig.params.push(AbiParam::new(pt));
            }
            if let Some(rt) = ret_type {
                sig.returns.push(AbiParam::new(rt));
            }
            let fid = self.module.declare_function(name, Linkage::Import, &sig)?;
            declared_funcs.insert(name.to_string(), fid);
        }

        // Also dynamically declare any additional runtime intrinsics from MIR
        for func in &mir.functions {
            for bb in &func.blocks {
                for (_, inst, _) in &bb.instructions {
                    if let MirInst::CallRuntime { intrinsic, args } = inst {
                        if !declared_funcs.contains_key(intrinsic) {
                            let mut sig = self.module.make_signature();
                            for _ in args {
                                sig.params.push(AbiParam::new(ptr_type));
                            }
                            let ret_ty = if intrinsic.ends_with("_verify") {
                                Some(types::I8)
                            } else if intrinsic.starts_with("wml_res_set_") {
                                None
                            } else {
                                Some(ptr_type)
                            };
                            if let Some(rt) = ret_ty {
                                sig.returns.push(AbiParam::new(rt));
                            }
                            let fid =
                                self.module
                                    .declare_function(intrinsic, Linkage::Import, &sig)?;
                            declared_funcs.insert(intrinsic.clone(), fid);
                        }
                    }
                }
            }
        }

        // 1. Declare all functions first so forward calls resolve
        for func in &mir.functions {
            let mut sig = self.module.make_signature();
            for (_, p_ty) in &func.params {
                sig.params
                    .push(AbiParam::new(Self::mir_type_to_clif(p_ty, ptr_type)));
            }
            if func.return_ty != MirType::Unit {
                sig.returns.push(AbiParam::new(Self::mir_type_to_clif(
                    &func.return_ty,
                    ptr_type,
                )));
            }

            let func_id = self
                .module
                .declare_function(&func.name, Linkage::Export, &sig)?;
            declared_funcs.insert(func.name.clone(), func_id);
        }

        // 2. Define each function
        let mut ctx = self.module.make_context();
        for func in &mir.functions {
            let func_id = declared_funcs[&func.name];

            let mut sig = self.module.make_signature();
            for (_, p_ty) in &func.params {
                sig.params
                    .push(AbiParam::new(Self::mir_type_to_clif(p_ty, ptr_type)));
            }
            if func.return_ty != MirType::Unit {
                sig.returns.push(AbiParam::new(Self::mir_type_to_clif(
                    &func.return_ty,
                    ptr_type,
                )));
            }
            ctx.func.signature = sig;

            self.build_function(func, &mut ctx.func, &declared_funcs, ptr_type)?;
            self.module.define_function(func_id, &mut ctx)?;
            self.module.clear_context(&mut ctx);
        }

        // 3. Finalize definitions in memory
        self.module.finalize_definitions()?;

        // 4. Collect finalized function pointers
        let mut compiled_fns = HashMap::new();
        for func in &mir.functions {
            let func_id = declared_funcs[&func.name];
            let ptr = self.module.get_finalized_function(func_id);
            compiled_fns.insert(func.name.clone(), ptr);
        }

        let mut compiled_routes = Vec::new();
        for route in &mir.routes {
            if let Some(&fn_ptr) = compiled_fns.get(&route.handler_fn) {
                compiled_routes.push(CompiledRoute {
                    method: route.method.clone(),
                    path: route.path.clone(),
                    fn_ptr,
                });
            }
        }

        Ok(CompiledModule {
            routes: compiled_routes,
            functions: compiled_fns,
        })
    }

    fn build_function(
        &mut self,
        mir_fn: &MirFunction,
        clif_fn: &mut Function,
        declared: &HashMap<String, FuncId>,
        ptr_type: types::Type,
    ) -> Result<(), CodegenError> {
        let mut builder = FunctionBuilder::new(clif_fn, &mut self.fn_builder_ctx);

        // Order blocks with entry block first
        let mut ordered_bbs: Vec<&BasicBlock> = Vec::new();
        if let Some(entry_bb) = mir_fn.blocks.iter().find(|b| b.id == mir_fn.entry) {
            ordered_bbs.push(entry_bb);
        }
        for bb in &mir_fn.blocks {
            if bb.id != mir_fn.entry {
                ordered_bbs.push(bb);
            }
        }

        let mut block_map: HashMap<BlockId, Block> = HashMap::new();
        for bb in &ordered_bbs {
            let clif_block = builder.create_block();
            block_map.insert(bb.id, clif_block);
        }

        // Add parameters to entry and successor blocks
        for bb in &ordered_bbs {
            let clif_block = block_map[&bb.id];
            for (_, p_ty) in &bb.params {
                let ty = Self::mir_type_to_clif(p_ty, ptr_type);
                builder.append_block_param(clif_block, ty);
            }
        }

        let mut value_map: HashMap<ValueId, Value> = HashMap::new();
        let mut slice_lens: HashMap<ValueId, Value> = HashMap::new();

        for bb in &ordered_bbs {
            let clif_block = block_map[&bb.id];
            builder.switch_to_block(clif_block);

            // Map block parameters to ValueId
            let params = builder.block_params(clif_block);
            for (idx, (val_id, _)) in bb.params.iter().enumerate() {
                value_map.insert(*val_id, params[idx]);
            }

            // Lower instructions
            for (dest, inst, ty) in &bb.instructions {
                let clif_val = match inst {
                    MirInst::Const(MirConstant::Int(i)) => {
                        let clif_ty = Self::mir_type_to_clif(ty, ptr_type);
                        builder.ins().iconst(clif_ty, *i)
                    }
                    MirInst::Const(MirConstant::Float(f)) => builder.ins().f64const(*f),
                    MirInst::Const(MirConstant::Bool(b)) => {
                        builder.ins().iconst(types::I8, if *b { 1 } else { 0 })
                    }
                    MirInst::Const(MirConstant::Str(s)) => {
                        let mut bytes = s.clone().into_bytes();
                        let len_val = bytes.len() as i64;
                        bytes.push(0); // Null-terminate for C-ABI safety
                        let boxed_bytes = bytes.into_boxed_slice();
                        let ptr_val = boxed_bytes.as_ptr() as usize as i64;
                        self.string_pool.push(boxed_bytes);

                        let v_ptr = builder.ins().iconst(ptr_type, ptr_val);
                        let v_len = builder.ins().iconst(types::I64, len_val);
                        slice_lens.insert(*dest, v_len);
                        v_ptr
                    }
                    MirInst::Const(MirConstant::Null) => builder.ins().iconst(ptr_type, 0),
                    MirInst::Binary { op, lhs, rhs } => {
                        let l = value_map[lhs];
                        let r = value_map[rhs];
                        let ty_l = builder.func.dfg.value_type(l);
                        let ty_r = builder.func.dfg.value_type(r);
                        let (l, r) = if ty_l != ty_r && ty_l.is_int() && ty_r.is_int() {
                            if ty_l.bits() < ty_r.bits() {
                                (builder.ins().uextend(ty_r, l), r)
                            } else {
                                (l, builder.ins().uextend(ty_l, r))
                            }
                        } else {
                            (l, r)
                        };

                        match op {
                            MirBinaryOp::Add => builder.ins().iadd(l, r),
                            MirBinaryOp::Sub => builder.ins().isub(l, r),
                            MirBinaryOp::Mul => builder.ins().imul(l, r),
                            MirBinaryOp::Div => builder.ins().sdiv(l, r),
                            MirBinaryOp::Mod => builder.ins().srem(l, r),
                            MirBinaryOp::Eq => builder.ins().icmp(IntCC::Equal, l, r),
                            MirBinaryOp::NotEq => builder.ins().icmp(IntCC::NotEqual, l, r),
                            MirBinaryOp::Lt => builder.ins().icmp(IntCC::SignedLessThan, l, r),
                            MirBinaryOp::LtEq => {
                                builder.ins().icmp(IntCC::SignedLessThanOrEqual, l, r)
                            }
                            MirBinaryOp::Gt => builder.ins().icmp(IntCC::SignedGreaterThan, l, r),
                            MirBinaryOp::GtEq => {
                                builder.ins().icmp(IntCC::SignedGreaterThanOrEqual, l, r)
                            }
                            MirBinaryOp::And => builder.ins().band(l, r),
                            MirBinaryOp::Or => builder.ins().bor(l, r),
                        }
                    }
                    MirInst::Unary { op, operand } => {
                        let v = value_map[operand];
                        match op {
                            MirUnaryOp::Not => builder.ins().bnot(v),
                            MirUnaryOp::Neg => builder.ins().ineg(v),
                        }
                    }
                    MirInst::Copy(src) => {
                        if let Some(&l) = slice_lens.get(src) {
                            slice_lens.insert(*dest, l);
                        }
                        value_map[src]
                    }
                    MirInst::MakeSlice { ptr, len } => {
                        let p = value_map[ptr];
                        let l = value_map[len];
                        slice_lens.insert(*dest, l);
                        p
                    }
                    MirInst::SlicePtr(slice) => {
                        if let Some(&l) = slice_lens.get(slice) {
                            slice_lens.insert(*dest, l);
                        }
                        value_map[slice]
                    }
                    MirInst::SliceLen(slice) => slice_lens
                        .get(slice)
                        .copied()
                        .unwrap_or_else(|| builder.ins().iconst(types::I64, 0)),
                    MirInst::AllocReqArena { size, align } => {
                        if let Some(&callee_id) = declared.get("wml_arena_alloc") {
                            let callee_ref =
                                self.module.declare_func_in_func(callee_id, builder.func);
                            let arena_val = if let Some(first_block) =
                                mir_fn.blocks.iter().find(|b| b.id == mir_fn.entry)
                            {
                                if let Some(&(val_id, _)) = first_block.params.first() {
                                    value_map.get(&val_id).copied()
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            let arena_val =
                                arena_val.unwrap_or_else(|| builder.ins().iconst(ptr_type, 0));
                            let sz = builder.ins().iconst(types::I64, *size as i64);
                            let al = builder.ins().iconst(types::I64, *align as i64);
                            let call_inst = builder.ins().call(callee_ref, &[arena_val, sz, al]);
                            let results = builder.inst_results(call_inst);
                            if results.is_empty() {
                                builder.ins().iconst(ptr_type, 0)
                            } else {
                                results[0]
                            }
                        } else {
                            builder.ins().iconst(ptr_type, 0)
                        }
                    }
                    MirInst::AllocGlobal { size, align } => {
                        if let Some(&callee_id) = declared.get("wml_arena_alloc") {
                            let callee_ref =
                                self.module.declare_func_in_func(callee_id, builder.func);
                            let null_arena = builder.ins().iconst(ptr_type, 0);
                            let sz = builder.ins().iconst(types::I64, *size as i64);
                            let al = builder.ins().iconst(types::I64, *align as i64);
                            let call_inst = builder.ins().call(callee_ref, &[null_arena, sz, al]);
                            let results = builder.inst_results(call_inst);
                            if results.is_empty() {
                                builder.ins().iconst(ptr_type, 0)
                            } else {
                                results[0]
                            }
                        } else {
                            builder.ins().iconst(ptr_type, 0)
                        }
                    }
                    MirInst::Load {
                        ptr,
                        offset,
                        ty: load_ty,
                    } => {
                        let p = value_map[ptr];
                        let clif_ty = Self::mir_type_to_clif(load_ty, ptr_type);
                        builder
                            .ins()
                            .load(clif_ty, MemFlagsData::trusted(), p, *offset as i32)
                    }
                    MirInst::Store { ptr, offset, val } => {
                        let p = value_map[ptr];
                        let v = value_map[val];
                        builder
                            .ins()
                            .store(MemFlagsData::trusted(), v, p, *offset as i32);
                        builder.ins().iconst(types::I64, 0)
                    }
                    MirInst::CallRuntime { intrinsic, args } => {
                        if let Some(&callee_id) = declared.get(intrinsic) {
                            let callee_ref =
                                self.module.declare_func_in_func(callee_id, builder.func);
                            let arg_vals: Vec<Value> = args.iter().map(|a| value_map[a]).collect();
                            let call_inst = builder.ins().call(callee_ref, &arg_vals);
                            let results = builder.inst_results(call_inst);
                            if results.is_empty() {
                                builder.ins().iconst(types::I64, 0)
                            } else {
                                results[0]
                            }
                        } else {
                            builder.ins().iconst(types::I64, 0)
                        }
                    }
                    MirInst::Call { func, args } => {
                        if let Some(&callee_id) = declared.get(func) {
                            let callee_ref =
                                self.module.declare_func_in_func(callee_id, builder.func);
                            let arg_vals: Vec<Value> = args.iter().map(|a| value_map[a]).collect();
                            let call_inst = builder.ins().call(callee_ref, &arg_vals);
                            let results = builder.inst_results(call_inst);
                            if results.is_empty() {
                                builder.ins().iconst(types::I64, 0)
                            } else {
                                results[0]
                            }
                        } else {
                            builder.ins().iconst(types::I64, 0)
                        }
                    }
                };
                value_map.insert(*dest, clif_val);
            }

            // Lower terminator
            match &bb.terminator {
                MirTerminator::Return(val) => {
                    if builder.func.signature.returns.is_empty() {
                        builder.ins().return_(&[]);
                    } else if let Some(v) = val {
                        let ret_val = value_map[v];
                        builder.ins().return_(&[ret_val]);
                    } else {
                        let zero = builder.ins().iconst(types::I64, 0);
                        builder.ins().return_(&[zero]);
                    }
                }
                MirTerminator::Jump { target, args } => {
                    let target_block = block_map[target];
                    let arg_vals: Vec<BlockArg> =
                        args.iter().map(|a| value_map[a].into()).collect();
                    builder.ins().jump(target_block, &arg_vals);
                }
                MirTerminator::Branch {
                    cond,
                    then_block,
                    then_args,
                    else_block,
                    else_args,
                } => {
                    let c = value_map[cond];
                    let then_b = block_map[then_block];
                    let else_b = block_map[else_block];
                    let t_args: Vec<BlockArg> =
                        then_args.iter().map(|a| value_map[a].into()).collect();
                    let e_args: Vec<BlockArg> =
                        else_args.iter().map(|a| value_map[a].into()).collect();
                    builder.ins().brif(c, then_b, &t_args, else_b, &e_args);
                }
                MirTerminator::Throw { status, .. } => {
                    if builder.func.signature.returns.is_empty() {
                        builder.ins().return_(&[]);
                    } else {
                        let ret_status = builder.ins().iconst(types::I64, *status as i64);
                        builder.ins().return_(&[ret_status]);
                    }
                }
                MirTerminator::Unreachable => {
                    builder
                        .ins()
                        .trap(cranelift_codegen::ir::TrapCode::unwrap_user(1));
                }
            }
        }

        builder.seal_all_blocks();
        builder.finalize(self.module.target_config());
        Ok(())
    }

    pub fn mir_type_to_clif(ty: &MirType, ptr_type: types::Type) -> types::Type {
        match ty {
            MirType::I64 => types::I64,
            MirType::F64 => types::F64,
            MirType::Bool => types::I8,
            MirType::Ptr | MirType::Slice => ptr_type,
            MirType::Tag(_) => types::I16,
            MirType::Unit => types::I64,
        }
    }
}
