use std::fs;
use wml_client::ClientTranspiler;
use wml_syntax::{ast::Item, parse};

#[test]
fn test_transpile_click_island() {
    let source = fs::read_to_string("F:/DEV/WML/examples/counter.wml").unwrap();
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
    let js = transpiler.transpile_island(ui_item, &arena);

    assert!(js.contains("export function signal"));
    assert!(js.contains("export function mount_ClickIsland"));
    assert!(js.contains("async function syncClicks(delta)"));
}
