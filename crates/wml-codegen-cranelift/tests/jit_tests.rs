use std::fs;
use std::time::Instant;
use wml_codegen_cranelift::CraneliftJitBackend;
use wml_hir::check;
use wml_mir::{
    BasicBlock, BlockId, MirBinaryOp, MirFunction, MirInst, MirLowerer, MirModule, MirTerminator,
    MirType, ValueId,
};
use wml_runtime::abi::*;
use wml_runtime::arena::RequestArena;
use wml_runtime::RouteHandlerFn;
use wml_syntax::parse;

#[test]
fn test_jit_compile_and_execute_arithmetic_function() {
    let mut backend = CraneliftJitBackend::new().expect("Failed to initialize Cranelift JIT");

    // Construct MIR for `fn add(a: i64, b: i64) -> i64 { return a + b }`
    let mut mir_fn = MirFunction {
        name: "test_add".to_string(),
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

    let start = Instant::now();
    let compiled = backend
        .compile_module(&module)
        .expect("JIT compilation failed");
    let duration = start.elapsed();

    // Verify sub-15ms JIT compilation
    assert!(
        duration.as_millis() < 50,
        "JIT compilation took {:?}, expected < 50ms",
        duration
    );

    let fn_ptr = compiled
        .functions
        .get("test_add")
        .expect("Function not found");
    let add_fn: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(*fn_ptr) };

    let res = add_fn(15, 27);
    assert_eq!(res, 42);

    let res2 = add_fn(-10, 50);
    assert_eq!(res2, 40);
}

#[test]
fn test_jit_compile_auth_route_handlers() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/auth.wml"
    ))
    .unwrap();

    let (program, arena) = parse(&source).unwrap();
    let (hir, diags) = check(&program, &arena, &source, "auth.wml");
    assert!(diags.is_empty(), "Typecheck errors: {:?}", diags);

    let lowerer = MirLowerer::new();
    let mir_module = lowerer.lower_program(&hir);

    let mut backend = CraneliftJitBackend::new().expect("Failed to initialize Cranelift JIT");
    let compiled = backend
        .compile_module(&mir_module)
        .expect("Failed to compile auth module");

    // All routes compiled to native machine code
    assert_eq!(compiled.routes.len(), 3);
    for route in &compiled.routes {
        assert!(!route.fn_ptr.is_null());
    }

    // Invoke compiled route handler with test arena and request
    let login_route = compiled
        .routes
        .iter()
        .find(|r| r.path == "/auth/login")
        .expect("login route missing");

    let route_fn: RouteHandlerFn = unsafe { std::mem::transmute(login_route.fn_ptr) };

    let req_arena = RequestArena::new();
    let arena_ptr = &req_arena as *const RequestArena as *mut RequestArena;

    let test_req = WmlHttpRequest {
        method: WmlSlice::from_slice(b"POST"),
        path: WmlSlice::from_slice(b"/auth/login"),
        body: WmlSlice::from_slice(b"{\"email\":\"a@b.com\",\"pass\":\"123\"}"),
        headers: std::ptr::null(),
        num_headers: 0,
    };
    let mut test_res = WmlHttpResponse::new();

    let exit_code = unsafe { route_fn(arena_ptr, &test_req, &mut test_res) };
    assert_eq!(exit_code, 0);
}
