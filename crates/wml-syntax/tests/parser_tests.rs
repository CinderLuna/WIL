use std::fs;
use wml_syntax::{
    ast::{BinaryOp, Expr, Item, Stmt},
    parse,
};

#[test]
fn test_parse_auth_example() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/auth.wml"
    ))
    .expect("Failed to read examples/auth.wml");
    let res = parse(&source);
    assert!(res.is_ok(), "Failed to parse auth.wml: {:?}", res.err());

    let (program, _arena) = res.unwrap();
    // In auth.wml: 1 type (User) + 3 routes (/auth/signup, /auth/login, /api/me)
    assert_eq!(program.items.len(), 4);
}

#[test]
fn test_parse_counter_example() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/counter.wml"
    ))
    .expect("Failed to read examples/counter.wml");
    let res = parse(&source);
    assert!(res.is_ok(), "Failed to parse counter.wml: {:?}", res.err());

    let (program, _arena) = res.unwrap();
    // In counter.wml: 1 schema (Stats) + 1 page (/dashboard) + 1 ui (ClickIsland)
    assert_eq!(program.items.len(), 3);
}

#[test]
fn test_parse_nested_expressions() {
    let source = r#"
    fn calculate(x: int, y: int) -> int {
        let res = x + y * 2 ?? throw 500 "Math error"
        return res
    }
    "#;
    let res = parse(source);
    assert!(res.is_ok(), "Failed to parse: {:?}", res.err());
}

#[test]
fn test_prefix_operator_precedence() {
    let source = r#"
    fn test_precedence() {
        let x = -a + b
    }
    "#;
    let (program, arena) = parse(source).expect("Failed to parse -a + b");
    if let Item::Fn(f) = &program.items[0] {
        if let Stmt::Let { val, .. } = &f.body.stmts[0] {
            let root_expr = arena.get_expr(*val);
            match root_expr {
                Expr::Binary { op, .. } => {
                    assert_eq!(*op, BinaryOp::Add, "Expected root operator to be Add");
                }
                other => panic!(
                    "Expected root expression to be Binary Add, found {:?}",
                    other
                ),
            }
        } else {
            panic!("Expected Let statement");
        }
    } else {
        panic!("Expected Fn item");
    }
}

#[test]
fn test_unquoted_jsx_and_assignment() {
    let source = r#"
    ui TestUi() {
        let x = 10
        x = 20
        <button class="btn">Click me here</button>
    }
    "#;
    let res = parse(source);
    assert!(res.is_ok(), "Failed to parse: {:?}", res.err());
}
