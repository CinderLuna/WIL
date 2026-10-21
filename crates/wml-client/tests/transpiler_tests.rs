use std::fs;
use wml_client::ClientTranspiler;
use wml_syntax::{ast::Item, parse};

#[test]
fn test_transpile_click_island() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/counter.wml"
    ))
    .unwrap();
    let (program, arena) = parse(&source).unwrap();

    let ui_item = program
        .items
        .iter()
        .find_map(|item| match item {
            Item::Ui(ui) => Some(ui),
            _ => None,
        })
        .expect("Ui island not found in counter.wml");

    let transpiler = ClientTranspiler::new();
    let res = transpiler.transpile_island(ui_item, &arena);
    assert!(res.is_ok(), "Transpilation failed: {:?}", res.err());

    let js = res.unwrap();
    assert!(js.contains("export function signal"));
    assert!(js.contains("export function mount_ClickIsland"));
    assert!(js.contains("async function syncClicks(delta)"));
    assert!(js.contains("document.createElement(\"div\")"));
    assert!(js.contains("document.createElement(\"button\")"));
    assert!(js.contains("addEventListener(\"click\""));
}
