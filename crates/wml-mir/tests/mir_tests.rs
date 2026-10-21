use std::fs;
use wml_hir::check;
use wml_mir::{EscapeAnalysis, MirInst, MirLowerer, MirOptimizer};
use wml_syntax::parse;

#[test]
fn test_lower_auth_example_to_mir() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/auth.wml"
    ))
    .unwrap();

    let (program, arena) = parse(&source).unwrap();
    let (hir, diags) = check(&program, &arena, &source, "auth.wml");
    assert!(diags.is_empty(), "Typecheck errors: {:?}", diags);

    let lowerer = MirLowerer::new();
    let mut mir_module = lowerer.lower_program(&hir);

    // Verify route functions generated
    assert_eq!(mir_module.routes.len(), 3);
    assert_eq!(mir_module.routes[0].path, "/auth/signup");
    assert_eq!(mir_module.routes[1].path, "/auth/login");
    assert_eq!(mir_module.routes[2].path, "/api/me");

    // Verify SSA functions generated
    assert_eq!(mir_module.functions.len(), 3);
    for func in &mir_module.functions {
        assert!(!func.blocks.is_empty());
        // Entry block has 3 params: (arena, req, res)
        assert_eq!(func.blocks[0].params.len(), 3);
    }

    // Run escape analysis
    let escape = EscapeAnalysis::new();
    let report = escape.run_on_module(&mut mir_module);
    assert_eq!(report.promoted_to_global, 0); // All allocations stay in RequestArena

    // Run optimizer
    let opt = MirOptimizer::new();
    opt.optimize_module(&mut mir_module);
}

#[test]
fn test_lower_counter_example_to_mir() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/counter.wml"
    ))
    .unwrap();

    let (program, arena) = parse(&source).unwrap();
    let (hir, diags) = check(&program, &arena, &source, "counter.wml");
    assert!(diags.is_empty(), "Typecheck errors: {:?}", diags);

    let lowerer = MirLowerer::new();
    let mir_module = lowerer.lower_program(&hir);

    // Page /dashboard + Action RPC routes
    assert_eq!(mir_module.routes.len(), 3);
    assert_eq!(mir_module.routes[0].path, "/dashboard");
    assert_eq!(mir_module.routes[1].path, "/_wml/rpc/syncClicks");
    assert_eq!(
        mir_module.routes[2].path,
        "/_wml/rpc/ClickIsland/syncClicks"
    );
    assert_eq!(mir_module.functions.len(), 2); // page handler + RPC action
}

#[test]
fn test_mir_coalesce_desugaring_and_constant_folding() {
    let source = r#"
    fn compute() -> int {
        let a = 10 + 20
        return a
    }
    "#;

    let (program, arena) = parse(source).unwrap();
    let (hir, diags) = check(&program, &arena, source, "test.wml");
    assert!(diags.is_empty(), "Typecheck errors: {:?}", diags);

    let lowerer = MirLowerer::new();
    let mut mir_module = lowerer.lower_program(&hir);

    let opt = MirOptimizer::new();
    opt.optimize_module(&mut mir_module);

    let func = &mir_module.functions[0];
    let mut has_const_30 = false;
    for bb in &func.blocks {
        for (_, inst, _) in &bb.instructions {
            if let MirInst::Const(wml_mir::MirConstant::Int(30)) = inst {
                has_const_30 = true;
            }
        }
    }
    assert!(
        has_const_30,
        "Constant folding should evaluate 10 + 20 to 30"
    );
}
