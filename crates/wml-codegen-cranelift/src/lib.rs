use cranelift_codegen::ir::{
    condcodes::IntCC, types, AbiParam, Block, BlockArg, Function, InstBuilder, Value,
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
}

impl Default for CraneliftJitBackend {
    fn default() -> Self {
        Self::new().expect("Failed to create CraneliftJitBackend")
    }
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

        let module = JITModule::new(jit_builder);

        Ok(Self {
            module,
            fn_builder_ctx: FunctionBuilderContext::new(),
        })
    }

    pub fn compile_module(&mut self, mir: &MirModule) -> Result<CompiledModule, CodegenError> {
        let mut declared_funcs: HashMap<String, FuncId> = HashMap::new();
        let ptr_type = self.module.target_config().pointer_type();

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
        for (name, func_id) in &declared_funcs {
            let ptr = self.module.get_finalized_function(*func_id);
            compiled_fns.insert(name.clone(), ptr);
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

        let mut block_map: HashMap<BlockId, Block> = HashMap::new();
        for bb in &mir_fn.blocks {
            let clif_block = builder.create_block();
            block_map.insert(bb.id, clif_block);
        }

        // Add parameters to entry and successor blocks
        for bb in &mir_fn.blocks {
            let clif_block = block_map[&bb.id];
            for (_, p_ty) in &bb.params {
                let ty = Self::mir_type_to_clif(p_ty, ptr_type);
                builder.append_block_param(clif_block, ty);
            }
        }

        let mut value_map: HashMap<ValueId, Value> = HashMap::new();

        for bb in &mir_fn.blocks {
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
                    MirInst::Const(MirConstant::Str(_)) | MirInst::Const(MirConstant::Null) => {
                        builder.ins().iconst(ptr_type, 0)
                    }
                    MirInst::Binary { op, lhs, rhs } => {
                        let l = value_map[lhs];
                        let r = value_map[rhs];
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
                    MirInst::Copy(src) => value_map[src],
                    MirInst::Call { func, args } => {
                        if let Some(&callee_id) = declared.get(func) {
                            let callee_ref =
                                self.module.declare_func_in_func(callee_id, builder.func);
                            let arg_vals: Vec<Value> = args.iter().map(|a| value_map[a]).collect();
                            let call_inst = builder.ins().call(callee_ref, &arg_vals);
                            builder.inst_results(call_inst)[0]
                        } else {
                            builder.ins().iconst(types::I64, 0)
                        }
                    }
                    _ => {
                        // Memory / Slice / Runtime intrinsics
                        builder.ins().iconst(types::I64, 0)
                    }
                };
                value_map.insert(*dest, clif_val);
            }

            // Lower terminator
            match &bb.terminator {
                MirTerminator::Return(val) => {
                    if let Some(v) = val {
                        let ret_val = value_map[v];
                        builder.ins().return_(&[ret_val]);
                    } else {
                        builder.ins().return_(&[]);
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
                    let ret_status = builder.ins().iconst(types::I64, *status as i64);
                    builder.ins().return_(&[ret_status]);
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
