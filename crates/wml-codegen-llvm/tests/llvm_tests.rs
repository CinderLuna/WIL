use std::fs;
use std::path::PathBuf;
use wml_codegen_llvm::*;
use wml_hir::check;
use wml_mir::{
    BasicBlock, BlockId, MirBinaryOp, MirFunction, MirInst, MirLowerer, MirModule, MirTerminator,
    MirType, ValueId,
};
use wml_syntax::parse;

#[test]
fn test_llvm_target_config_and_triples() {
    let host = TargetConfig::host();
    assert!(!host.triple().is_empty());
    assert!(!host.datalayout().is_empty());

    let x86 = TargetConfig::parse("x86_64-unknown-linux-gnu").unwrap();
    assert_eq!(x86.arch, TargetArch::X86_64);
    assert_eq!(x86.os, TargetOs::Linux);
    assert_eq!(x86.triple(), "x86_64-unknown-linux-gnu");
    assert!(x86.datalayout().contains("e-m:e"));

    let arm64 = TargetConfig::parse("aarch64-unknown-linux-gnu").unwrap();
    assert_eq!(arm64.arch, TargetArch::AArch64);
    assert_eq!(arm64.os, TargetOs::Linux);
    assert_eq!(arm64.triple(), "aarch64-unknown-linux-gnu");
    assert!(arm64.datalayout().contains("e-m:e"));

    let win_x64 = TargetConfig::parse("x86_64-pc-windows-msvc").unwrap();
    assert_eq!(win_x64.arch, TargetArch::X86_64);
    assert_eq!(win_x64.os, TargetOs::Windows);
    assert_eq!(win_x64.triple(), "x86_64-pc-windows-msvc");
    assert!(win_x64.datalayout().contains("e-m:w"));
}

#[test]
fn test_llvm_ir_emission_arithmetic() {
    let mut mir_fn = MirFunction {
        name: "test_calc".to_string(),
        params: vec![(ValueId(0), MirType::I64), (ValueId(1), MirType::I64)],
        return_ty: MirType::I64,
        blocks: Vec::new(),
        entry: BlockId(0),
        is_route_handler: false,
    };

    let mut bb0 = BasicBlock::new(BlockId(0));
    bb0.params = vec![(ValueId(0), MirType::I64), (ValueId(1), MirType::I64)];
    let sum_val = ValueId(2);
    bb0.instructions.push((
        sum_val,
        MirInst::Binary {
            op: MirBinaryOp::Add,
            lhs: ValueId(0),
            rhs: ValueId(1),
        },
        MirType::I64,
    ));
    bb0.terminator = MirTerminator::Return(Some(sum_val));
    mir_fn.blocks.push(bb0);

    let mut module = MirModule::new("test_mod");
    module.functions.push(mir_fn);

    let target = TargetConfig::x86_64_linux();
    let backend = LlvmAotBackend::with_target(target);
    let ir = backend.emit_ir(&module).unwrap();

    assert!(ir.contains("target triple = \"x86_64-unknown-linux-gnu\""));
    assert!(ir.contains("define i64 @test_calc(i64 %arg0, i64 %arg1)"));
    assert!(ir.contains("%v_0 = alloca i64"));
    assert!(ir.contains("%v_1 = alloca i64"));
    assert!(ir.contains("%v_2 = alloca i64"));
    assert!(ir.contains("add i64"));
    assert!(ir.contains("ret i64"));
}

#[test]
fn test_llvm_ir_emission_counter_and_auth_both_targets() {
    // 1. Counter Example
    let counter_source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/counter.wml"
    ))
    .unwrap();

    let (program, arena) = parse(&counter_source).unwrap();
    let (hir, diags) = check(&program, &arena, &counter_source, "counter.wml");
    assert!(diags.is_empty(), "Typecheck errors: {:?}", diags);

    let lowerer = MirLowerer::new();
    let mir_module = lowerer.lower_program(&hir);

    // Test x86_64 target
    let x86_backend = LlvmAotBackend::with_target(TargetConfig::x86_64_linux());
    let x86_ir = x86_backend.emit_ir(&mir_module).unwrap();
    assert!(x86_ir.contains("target triple = \"x86_64-unknown-linux-gnu\""));
    assert!(x86_ir.contains("declare void @wml_res_set_body_i64(ptr, ptr, i64)"));
    assert!(x86_ir.contains("declare ptr @wml_req_get_param(ptr, ptr, ptr)"));
    assert!(x86_ir.contains("@wml_num_routes = constant i64"));
    assert!(x86_ir.contains("define i32 @main("));

    // Test aarch64 (ARM64) target
    let arm_backend = LlvmAotBackend::with_target(TargetConfig::aarch64_linux());
    let arm_ir = arm_backend.emit_ir(&mir_module).unwrap();
    assert!(arm_ir.contains("target triple = \"aarch64-unknown-linux-gnu\""));
    assert!(arm_ir.contains("define i32 @main("));

    // 2. Auth Example
    let auth_source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/auth.wml"
    ))
    .unwrap();

    let (auth_program, auth_arena) = parse(&auth_source).unwrap();
    let (auth_hir, auth_diags) = check(&auth_program, &auth_arena, &auth_source, "auth.wml");
    assert!(auth_diags.is_empty());

    let auth_mir = MirLowerer::new().lower_program(&auth_hir);
    let auth_ir = x86_backend.emit_ir(&auth_mir).unwrap();
    assert!(auth_ir.contains("declare ptr @wml_crypto_hash(ptr, ptr)"));
    assert!(auth_ir.contains("declare i1 @wml_crypto_verify(ptr, ptr, ptr)"));
    assert!(auth_ir.contains("declare ptr @wml_jwt_sign(ptr, ptr)"));
}

#[test]
fn test_llvm_clang_compilation_if_available() {
    if !LlvmCompiler::is_clang_available() {
        println!("Clang not installed, skipping direct object compilation test");
        return;
    }

    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/counter.wml"
    ))
    .unwrap();
    let (program, arena) = parse(&source).unwrap();
    let (hir, diags) = check(&program, &arena, &source, "counter.wml");
    assert!(diags.is_empty());

    let mir = MirLowerer::new().lower_program(&hir);
    let host_target = TargetConfig::host();
    let compiler = LlvmCompiler::new();
    let ir = compiler.compile_to_ir(&mir, &host_target).unwrap();

    let out_obj = PathBuf::from("target/test_counter.obj");
    let result = compiler.compile_to_object(&ir, &host_target, &out_obj);
    assert!(
        result.is_ok(),
        "Clang object compilation failed: {:?}",
        result.err()
    );

    let obj_bytes = fs::read(&out_obj).unwrap();
    assert!(!obj_bytes.is_empty(), "Generated object file is empty");
    let _ = fs::remove_file(&out_obj);

    // Cross-compilation test to aarch64
    let arm_target = TargetConfig::aarch64_linux();
    let arm_ir = compiler.compile_to_ir(&mir, &arm_target).unwrap();
    let arm_obj = PathBuf::from("target/test_counter_arm64.o");
    let arm_res = compiler.compile_to_object(&arm_ir, &arm_target, &arm_obj);
    assert!(
        arm_res.is_ok(),
        "Clang aarch64 cross compilation failed: {:?}",
        arm_res.err()
    );

    let arm_bytes = fs::read(&arm_obj).unwrap();
    assert!(
        !arm_bytes.is_empty(),
        "Generated aarch64 object file is empty"
    );
    let _ = fs::remove_file(&arm_obj);
}
