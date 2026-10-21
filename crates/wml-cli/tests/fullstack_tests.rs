use std::fs;
use std::path::PathBuf;
use wml_client::ClientTranspiler;
use wml_codegen_cranelift::CraneliftJitBackend;
use wml_hir::check;
use wml_mir::{EscapeAnalysis, MirLowerer, MirOptimizer};
use wml_runtime::abi::*;
use wml_runtime::arena::RequestArena;
use wml_runtime::{HttpReactor, RouteHandlerFn};
use wml_syntax::ast::Item;
use wml_syntax::parse;

#[test]
fn test_wml_build_counter_produces_manifest_and_islands_js() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/counter.wml"
    ))
    .unwrap();

    let (program, arena) = parse(&source).unwrap();
    let (hir, diags) = check(&program, &arena, &source, "counter.wml");
    assert!(diags.is_empty(), "Typecheck errors: {:?}", diags);

    let lowerer = MirLowerer::new();
    let mut mir_module = lowerer.lower_program(&hir);

    let escape = EscapeAnalysis::new();
    escape.run_on_module(&mut mir_module);

    let opt = MirOptimizer::new();
    opt.optimize_module(&mut mir_module);

    let out_dir = PathBuf::from("target/test_dist_counter");
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
    assert_eq!(uis[0].name, "ClickIsland");

    let transpiler = ClientTranspiler::new();
    let bundle = transpiler.bundle_islands(&uis, &arena).unwrap();
    let bundle_path = assets_dir.join("islands.js");
    fs::write(&bundle_path, &bundle).unwrap();

    // Verify micro-signals bundle size constraint (< 2.5KB gzipped, < 3.5KB raw)
    assert!(
        bundle.len() < 3500,
        "Bundle size {} bytes exceeds target limit",
        bundle.len()
    );
    assert!(bundle.contains("export function signal"));
    assert!(bundle.contains("export function mount_ClickIsland"));
    assert!(bundle.contains("export function auto_mount()"));

    // Verify manifest metadata
    let manifest_path = out_dir.join("manifest.json");
    let manifest = serde_json::json!({
        "version": "1.0.0",
        "entry": "examples/counter.wml",
        "routes": mir_module.routes.iter().map(|r| serde_json::json!({
            "method": r.method,
            "path": r.path,
        })).collect::<Vec<_>>(),
        "islands": ["ClickIsland"],
        "assets": [{
            "name": "islands.js",
            "path": bundle_path.display().to_string(),
            "size_bytes": bundle.len(),
            "type": "javascript"
        }]
    });
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    assert!(manifest_path.exists());
    assert!(bundle_path.exists());
    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_fullstack_dev_routes_and_html_dispatch() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/counter.wml"
    ))
    .unwrap();

    let (program, arena) = parse(&source).unwrap();
    let (hir, diags) = check(&program, &arena, &source, "counter.wml");
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

    // Dispatch GET /dashboard
    let req_arena = RequestArena::new();
    let arena_ptr = &req_arena as *const RequestArena as *mut RequestArena;
    let page_req = WmlHttpRequest {
        method: WmlSlice::from_slice(b"GET"),
        path: WmlSlice::from_slice(b"/dashboard"),
        body: WmlSlice::empty(),
        headers: std::ptr::null(),
        num_headers: 0,
    };
    let mut page_res = WmlHttpResponse::new();

    let page_route = compiled
        .routes
        .iter()
        .find(|r| r.method == "GET" && r.path == "/dashboard")
        .expect("Page route not found");
    let page_handler: RouteHandlerFn = unsafe { std::mem::transmute(page_route.fn_ptr) };
    let status_code = unsafe { page_handler(arena_ptr, &page_req, &mut page_res) };

    assert_eq!(status_code, 0);
    unsafe {
        assert_eq!(page_res.status, 200);
        assert_eq!(page_res.content_type.as_str(), "text/html; charset=utf-8");
        let html = page_res.body.as_str();
        assert!(
            html.contains("Live Analytics Dashboard"),
            "Expected Live Analytics Dashboard in HTML: {}",
            html
        );
        assert!(
            html.contains("data-wml-island=\"ClickIsland\""),
            "Expected data-wml-island in HTML: {}",
            html
        );
        assert!(
            html.contains("/_wml/assets/islands.js"),
            "Expected script tag for islands.js: {}",
            html
        );
        assert_eq!(
            html.matches("/_wml/assets/islands.js").count(),
            1,
            "Expected exactly one islands.js script tag in HTML"
        );
        assert!(
            html.contains(
                r#"<script type="module" src="/_wml/assets/islands.js"></script></body>"#
            ),
            "Expected script tag immediately before closing body"
        );
    }

    // Dispatch RPC action POST /_wml/rpc/syncClicks
    let rpc_route = compiled
        .routes
        .iter()
        .find(|r| r.method == "POST" && r.path == "/_wml/rpc/syncClicks")
        .expect("RPC route not found");
    let rpc_handler: RouteHandlerFn = unsafe { std::mem::transmute(rpc_route.fn_ptr) };
    let rpc_req = WmlHttpRequest {
        method: WmlSlice::from_slice(b"POST"),
        path: WmlSlice::from_slice(b"/_wml/rpc/syncClicks"),
        body: WmlSlice::from_slice(b"{\"delta\":1}"),
        headers: std::ptr::null(),
        num_headers: 0,
    };
    let mut rpc_res = WmlHttpResponse::new();
    let rpc_status = unsafe { rpc_handler(arena_ptr, &rpc_req, &mut rpc_res) };
    assert_eq!(rpc_status, 0);
    assert_eq!(rpc_res.status, 200);
    unsafe {
        assert_eq!(rpc_res.content_type.as_str(), "application/json");
        assert_eq!(rpc_res.body.as_str(), "1");
    }

    // Also verify scoped RPC route POST /_wml/rpc/ClickIsland/syncClicks
    let scoped_rpc_route = compiled
        .routes
        .iter()
        .find(|r| r.method == "POST" && r.path == "/_wml/rpc/ClickIsland/syncClicks")
        .expect("Scoped RPC route not found");
    let scoped_rpc_handler: RouteHandlerFn =
        unsafe { std::mem::transmute(scoped_rpc_route.fn_ptr) };
    let mut scoped_rpc_res = WmlHttpResponse::new();
    let scoped_status = unsafe { scoped_rpc_handler(arena_ptr, &rpc_req, &mut scoped_rpc_res) };
    assert_eq!(scoped_status, 0);
    assert_eq!(scoped_rpc_res.status, 200);
    unsafe {
        assert_eq!(scoped_rpc_res.content_type.as_str(), "application/json");
        assert_eq!(scoped_rpc_res.body.as_str(), "1");
    }
}
