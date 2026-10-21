use std::fs;
use std::path::PathBuf;
use wml_client::ClientTranspiler;
use wml_codegen_cranelift::CraneliftJitBackend;
use wml_codegen_llvm::*;
use wml_hir::check;
use wml_mir::{EscapeAnalysis, MirLowerer, MirOptimizer};
use wml_runtime::abi::*;
use wml_runtime::arena::RequestArena;
use wml_runtime::{HttpReactor, RouteHandlerFn};
use wml_syntax::ast::Item;
use wml_syntax::parse;

#[test]
fn test_edgeflag_syntax_and_typecheck() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/edgeflag.wml"
    ))
    .expect("Failed to read examples/edgeflag.wml");

    let (program, arena) = parse(&source).expect("Failed to parse edgeflag.wml");
    let (hir, diags) = check(&program, &arena, &source, "edgeflag.wml");

    assert!(
        diags.is_empty(),
        "Expected 0 typecheck diagnostics, found: {:?}",
        diags
    );

    // Verify AST item counts
    assert_eq!(
        hir.schemas.len(),
        3,
        "Expected 3 schemas (Flag, Metric, EdgeKey)"
    );
    assert_eq!(hir.pages.len(), 1, "Expected 1 page (/)");
    assert_eq!(hir.uis.len(), 1, "Expected 1 UI island (FlagControlIsland)");
    assert_eq!(
        hir.uis[0].actions.len(),
        3,
        "Expected 3 actions (toggleFlag, updateRollout, simulateEval)"
    );
    assert_eq!(hir.routes.len(), 3, "Expected 3 API routes");
}

#[test]
fn test_edgeflag_build_and_island_bundle() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/edgeflag.wml"
    ))
    .unwrap();

    let (program, arena) = parse(&source).unwrap();
    let (hir, diags) = check(&program, &arena, &source, "edgeflag.wml");
    assert!(diags.is_empty(), "Typecheck errors: {:?}", diags);

    let lowerer = MirLowerer::new();
    let mut mir_module = lowerer.lower_program(&hir);

    let escape = EscapeAnalysis::new();
    escape.run_on_module(&mut mir_module);

    let opt = MirOptimizer::new();
    opt.optimize_module(&mut mir_module);

    let out_dir = PathBuf::from("target/test_dist_edgeflag");
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).unwrap();
    let assets_dir = out_dir.join("assets");
    fs::create_dir_all(&assets_dir).unwrap();

    let uis: Vec<_> = program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Ui(ui) => Some(ui),
            _ => None,
        })
        .collect();

    assert_eq!(uis.len(), 1);
    assert_eq!(uis[0].name, "FlagControlIsland");

    let transpiler = ClientTranspiler::new();
    let bundle = transpiler.bundle_islands(&uis, &arena).unwrap();
    let bundle_path = assets_dir.join("islands.js");
    fs::write(&bundle_path, &bundle).unwrap();

    // Verify micro-signals client bundle
    assert!(
        bundle.len() < 5500,
        "Bundle size {} bytes exceeds target limit",
        bundle.len()
    );
    assert!(bundle.contains("export function signal"));
    assert!(bundle.contains("export function mount_FlagControlIsland"));
    assert!(bundle.contains("toggleFlag"));
    assert!(bundle.contains("updateRollout"));
    assert!(bundle.contains("simulateEval"));
    assert!(bundle.contains("auto_mount()"));

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_edgeflag_dual_target_llvm_compilation() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/edgeflag.wml"
    ))
    .unwrap();

    let (program, arena) = parse(&source).unwrap();
    let (hir, diags) = check(&program, &arena, &source, "edgeflag.wml");
    assert!(diags.is_empty(), "Typecheck errors: {:?}", diags);

    let lowerer = MirLowerer::new();
    let mut mir_module = lowerer.lower_program(&hir);

    let escape = EscapeAnalysis::new();
    escape.run_on_module(&mut mir_module);

    let opt = MirOptimizer::new();
    opt.optimize_module(&mut mir_module);

    let out_dir = PathBuf::from("target/test_dist_edgeflag_llvm");
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).unwrap();

    // 1. Compile x86_64 target
    let x86_target = TargetConfig::x86_64_linux();
    let compiler = LlvmCompiler::new();
    let x86_ir = compiler.compile_to_ir(&mir_module, &x86_target).unwrap();
    assert!(x86_ir.contains("target triple = \"x86_64-unknown-linux-gnu\""));
    assert!(x86_ir.contains("define i32 @main("));

    // 2. Compile aarch64 (ARM64) target
    let arm_target = TargetConfig::aarch64_linux();
    let arm_ir = compiler.compile_to_ir(&mir_module, &arm_target).unwrap();
    assert!(arm_ir.contains("target triple = \"aarch64-unknown-linux-gnu\""));
    assert!(arm_ir.contains("define i32 @main("));

    // 3. Verify object compilation if Clang is available
    if LlvmCompiler::is_clang_available() {
        let x86_obj = out_dir.join("edgeflag_x86_64.o");
        compiler
            .compile_to_object(&x86_ir, &x86_target, &x86_obj)
            .unwrap();
        assert!(x86_obj.exists());
        assert!(fs::metadata(&x86_obj).unwrap().len() > 0);

        let arm_obj = out_dir.join("edgeflag_aarch64.o");
        compiler
            .compile_to_object(&arm_ir, &arm_target, &arm_obj)
            .unwrap();
        assert!(arm_obj.exists());
        assert!(fs::metadata(&arm_obj).unwrap().len() > 0);
    }

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_edgeflag_jit_runtime_dispatch() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/edgeflag.wml"
    ))
    .unwrap();

    let (program, arena) = parse(&source).unwrap();
    let (hir, diags) = check(&program, &arena, &source, "edgeflag.wml");
    assert!(diags.is_empty(), "Typecheck errors: {:?}", diags);

    let lowerer = MirLowerer::new();
    let mut mir_module = lowerer.lower_program(&hir);

    let escape = EscapeAnalysis::new();
    escape.run_on_module(&mut mir_module);

    let opt = MirOptimizer::new();
    opt.optimize_module(&mut mir_module);

    let mut jit_backend = CraneliftJitBackend::new().unwrap();
    let compiled = jit_backend.compile_module(&mir_module).unwrap();

    let mut reactor = HttpReactor::new();
    for route in &compiled.routes {
        let handler_fn: RouteHandlerFn = unsafe { std::mem::transmute(route.fn_ptr) };
        reactor.add_route(&route.method, &route.path, handler_fn);
    }

    // 1. Dispatch GET / (SSR Page)
    let req_arena = RequestArena::new();
    let arena_ptr = &req_arena as *const RequestArena as *mut RequestArena;
    let page_req = WmlHttpRequest {
        method: WmlSlice::from_slice(b"GET"),
        path: WmlSlice::from_slice(b"/"),
        body: WmlSlice::empty(),
        headers: std::ptr::null(),
        num_headers: 0,
    };
    let mut page_res = WmlHttpResponse::new();

    let page_route = compiled
        .routes
        .iter()
        .find(|r| r.method == "GET" && r.path == "/")
        .expect("Root page route `/` not registered");

    let page_handler: RouteHandlerFn = unsafe { std::mem::transmute(page_route.fn_ptr) };
    let code = unsafe { page_handler(arena_ptr, &page_req, &mut page_res) };
    assert_eq!(code, 0);
    assert_eq!(page_res.status, 200);

    let body_str = unsafe { page_res.body.as_str() };
    assert!(body_str.contains("<!DOCTYPE html>"));
    assert!(body_str.contains("EdgeFlag Engine"));
    assert!(body_str.contains("data-wml-island=\"FlagControlIsland\""));
    assert!(body_str.contains("<script type=\"module\" src=\"/_wml/assets/islands.js\"></script>"));

    // 2. Dispatch POST /_wml/rpc/FlagControlIsland/toggleFlag (Island Action RPC)
    let rpc_route = compiled
        .routes
        .iter()
        .find(|r| r.method == "POST" && r.path == "/_wml/rpc/FlagControlIsland/toggleFlag")
        .expect("RPC route `/_wml/rpc/FlagControlIsland/toggleFlag` not registered");

    let rpc_req = WmlHttpRequest {
        method: WmlSlice::from_slice(b"POST"),
        path: WmlSlice::from_slice(b"/_wml/rpc/FlagControlIsland/toggleFlag"),
        body: WmlSlice::from_slice(b"{\"flag_id\": 1, \"state\": true}"),
        headers: std::ptr::null(),
        num_headers: 0,
    };
    let mut rpc_res = WmlHttpResponse::new();
    let rpc_handler: RouteHandlerFn = unsafe { std::mem::transmute(rpc_route.fn_ptr) };
    let rpc_code = unsafe { rpc_handler(arena_ptr, &rpc_req, &mut rpc_res) };
    assert_eq!(rpc_code, 0);
    assert_eq!(rpc_res.status, 200);
}
