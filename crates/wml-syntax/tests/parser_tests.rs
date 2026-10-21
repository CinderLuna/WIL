use std::fs;
use wml_syntax::parse;

#[test]
fn test_parse_auth_example() {
    let source = fs::read_to_string("F:/DEV/WML/examples/auth.wml")
        .expect("Failed to read examples/auth.wml");
    let res = parse(&source);
    assert!(res.is_ok(), "Failed to parse auth.wml: {:?}", res.err());

    let (program, _arena) = res.unwrap();
    // In auth.wml: 1 type (User) + 3 routes (/auth/signup, /auth/login, /api/me)
    assert_eq!(program.items.len(), 4);
}

#[test]
fn test_parse_counter_example() {
    let source = fs::read_to_string("F:/DEV/WML/examples/counter.wml")
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
