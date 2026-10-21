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
    assert!(js.contains("const initial = props.initial !== undefined ? props.initial : undefined;"));
    assert!(js.contains("async function syncClicks(delta)"));
    assert!(js.contains("/_wml/rpc/ClickIsland/syncClicks"));
    assert!(js.contains("document.createElement(\"div\")"));
    assert!(js.contains("document.createElement(\"button\")"));
    assert!(js.contains("addEventListener(\"click\""));
    // Verify signal access uses .val
    assert!(js.contains("count.val += 1"));
    assert!(js.contains(".textContent = count.val"));
}

#[test]
fn test_escape_js_string() {
    use wml_client::escape_js_string;

    assert_eq!(
        escape_js_string(r#"Hello "world" & <script>alert('xss')</script>"#),
        r#"Hello \"world\" & \x3Cscript\x3Ealert(\'xss\')\x3C/script\x3E"#
    );
    assert_eq!(
        escape_js_string("line1\nline2\rline3\t"),
        "line1\\nline2\\rline3\\t"
    );
    assert_eq!(escape_js_string("\u{2028}\u{2029}"), "\\u2028\\u2029");
}

#[test]
fn test_event_attribute_casing_and_plain_attributes() {
    let source = r#"
ui TestIsland() {
    <div online="true" onClick={ let x = 1; count += x; } @submit={ syncClicks(1); }>
        "Content"
    </div>
}
"#;
    let (program, arena) = parse(source).unwrap();
    let ui_item = match &program.items[0] {
        Item::Ui(ui) => ui,
        _ => panic!("Expected UI item"),
    };

    let transpiler = ClientTranspiler::new();
    let js = transpiler.transpile_island(ui_item, &arena).unwrap();

    // Plain attribute 'online' remains setAttribute and is not converted to an event
    assert!(js.contains(r#".setAttribute("online", "true")"#));
    // onClick and @submit both lowercased to click and submit
    assert!(js.contains(r#".addEventListener("click""#));
    assert!(js.contains(r#".addEventListener("submit""#));
    // Let statement supported in event block
    assert!(js.contains("let x = 1;"));
}

#[test]
fn test_bundle_islands_with_auto_mount() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/counter.wml"
    ))
    .unwrap();
    let (program, arena) = parse(&source).unwrap();

    let uis: Vec<_> = program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Ui(ui) => Some(ui),
            _ => None,
        })
        .collect();

    let transpiler = ClientTranspiler::new();
    let bundle = transpiler.bundle_islands(&uis, &arena).unwrap();

    assert!(bundle.contains("export function signal"));
    assert!(bundle.contains("export function mount_ClickIsland"));
    assert!(bundle.contains("export function auto_mount()"));
    assert!(bundle.contains("data-wml-island"));
    assert!(bundle.contains("mount_ClickIsland(container, props);"));
}
