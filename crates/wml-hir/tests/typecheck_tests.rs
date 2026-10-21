use std::fs;
use wml_diagnostics::{DiagnosticCategory, DiagnosticSeverity, PatchType};
use wml_hir::check;
use wml_syntax::parse;

#[test]
fn test_typecheck_auth_canonical() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/auth.wml"
    ))
    .expect("Failed to read examples/auth.wml");

    let (program, arena) = parse(&source).expect("Failed to parse auth.wml");
    let (hir, diags) = check(&program, &arena, &source, "examples/auth.wml");

    assert!(
        diags.is_empty(),
        "Expected 0 type diagnostics for canonical auth.wml, found: {:?}",
        diags
    );
    assert_eq!(hir.types.len(), 1, "Expected 1 type (User)");
    assert_eq!(hir.routes.len(), 3, "Expected 3 routes");
}

#[test]
fn test_typecheck_counter_canonical() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/counter.wml"
    ))
    .expect("Failed to read examples/counter.wml");

    let (program, arena) = parse(&source).expect("Failed to parse counter.wml");
    let (hir, diags) = check(&program, &arena, &source, "examples/counter.wml");

    assert!(
        diags.is_empty(),
        "Expected 0 type diagnostics for canonical counter.wml, found: {:?}",
        diags
    );
    assert_eq!(hir.schemas.len(), 1, "Expected 1 schema (Stats)");
    assert_eq!(hir.pages.len(), 1, "Expected 1 page (/dashboard)");
    assert_eq!(hir.uis.len(), 1, "Expected 1 UI island (ClickIsland)");
    assert_eq!(
        hir.uis[0].actions.len(),
        1,
        "Expected 1 action (syncClicks)"
    );
}

#[test]
fn test_typecheck_type_mismatch() {
    let source = r#"
    fn test_err() {
        let x: int = "string value"
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "WML-TYP-2041");
    assert_eq!(diags[0].category, DiagnosticCategory::TypeMismatch);
}

#[test]
fn test_typecheck_return_type_mismatch() {
    let source = r#"
    route /api/token () -> {token: str} {
        return {token: 12345}
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "WML-TYP-2042");
    assert_eq!(diags[0].category, DiagnosticCategory::TypeMismatch);
}

#[test]
fn test_typecheck_undefined_variable_suggested_patch() {
    let source = r#"
    fn calculate() -> int {
        let count = 42
        return cout
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "WML-TYP-2001");
    assert_eq!(diags[0].category, DiagnosticCategory::UndefinedIdentifier);
    assert!(diags[0].suggested_patch.is_some());

    let patch = diags[0].suggested_patch.as_ref().unwrap();
    assert_eq!(patch.replacement.new_text, "count");
    assert_eq!(patch.confidence, 0.95);
}

#[test]
fn test_typecheck_db_insert_missing_field() {
    let source = r#"
    type User { id: int, email: str, hash: str }

    route /test () {
        let u = db.insert(User, {email: "a@b.com"})
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "WML-TYP-2043");
    assert_eq!(diags[0].category, DiagnosticCategory::DatabaseConstraint);
}

#[test]
fn test_typecheck_db_insert_optional_field_omitted() {
    let source = r#"
    type Profile { id: int, bio: str? }

    route /test () {
        let p = db.insert(Profile, {})
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert!(
        diags.is_empty(),
        "Optional fields should be omittable on insert: {:?}",
        diags
    );
}

#[test]
fn test_typecheck_client_island_security_violation() {
    let source = r#"
    type User { id: int }

    ui Island() {
        let u = db.get(User, id: 1)
        <div>"Client"</div>
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert!(diags
        .iter()
        .any(|d| d.code == "WML-SEC-5001" && d.category == DiagnosticCategory::SecurityViolation));
}

#[test]
fn test_typecheck_client_island_crypto_violation() {
    let source = r#"
    ui Island() {
        let h = crypto.hash("secret")
        <div>"Client"</div>
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert!(diags
        .iter()
        .any(|d| d.code == "WML-SEC-5001" && d.category == DiagnosticCategory::SecurityViolation));
}

#[test]
fn test_typecheck_ui_action_call_in_event_handler() {
    let source = r#"
    ui Clicker() {
        let count = signal(0)
        action increment(step: int) -> int {
            return step
        }
        <button onclick={
            count += 1;
            increment(1);
        }>"Click"</button>
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert!(
        diags.is_empty(),
        "Expected 0 errors for valid action invocation in event block, found: {:?}",
        diags
    );
}

#[test]
fn test_typecheck_req_user_without_auth() {
    let source = r#"
    route /api/me () {
        let u = req.user
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert!(diags
        .iter()
        .any(|d| d.code == "WML-SEC-5002" && d.category == DiagnosticCategory::SecurityViolation));
}

#[test]
fn test_typecheck_res_methods() {
    let source = r#"
    route /api/response () {
        res.status(200)
        res.json({ok: true})
        res.redirect("/home")
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert!(
        diags.is_empty(),
        "Expected res methods to typecheck without errors, found: {:?}",
        diags
    );
}

#[test]
fn test_typecheck_db_get_named_arg_outer_scope() {
    let source = r#"
    type User { id: int, email: str }

    route /api/find (post: {email: str}) {
        let u = db.get(User, email: email)
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert!(
        diags.is_empty(),
        "Expected db.get to resolve named arg val to outer parameter, found: {:?}",
        diags
    );
}

#[test]
fn test_typecheck_signal_math_and_assignment() {
    let source = r#"
    ui Counter() {
        let count = signal(0)
        let next = count + 1
        count = 10
        count += 2
        <div>{count}</div>
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    assert!(
        diags.is_empty(),
        "Expected signal arithmetic and assignment to typecheck without errors, found: {:?}",
        diags
    );
}

#[test]
fn test_typecheck_sec_5001_deduplicated() {
    let source = r#"
    type User { id: int }

    ui ClientIsland() {
        let u = db.get(User, id: 1)
        <div>"Island"</div>
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "test.wml");

    let sec_diags: Vec<_> = diags.iter().filter(|d| d.code == "WML-SEC-5001").collect();
    assert_eq!(
        sec_diags.len(),
        1,
        "Expected exactly 1 WML-SEC-5001 diagnostic, found: {:?}",
        sec_diags
    );
}

#[test]
fn test_typecheck_field_type_mismatch_with_scope_variable_patch() {
    let source = r#"
    type User { id: int, email: str, hash: str }

    route /api/register (post: {email: str, pass: str}) -> {token: str} {
        let hash = crypto.hash(pass)
        let user = db.insert(User, {email: 12345, hash})
        return {token: jwt.sign({sub: user.id})}
    }
    "#;

    let (program, arena) = parse(source).expect("Parse error");
    let (_hir, diags) = check(&program, &arena, source, "examples/auth.wml");

    assert_eq!(diags.len(), 1);
    let diag = &diags[0];
    assert_eq!(diag.code, "WML-TYP-2041");
    assert_eq!(diag.category, DiagnosticCategory::TypeMismatch);
    assert_eq!(diag.severity, DiagnosticSeverity::Error);
    assert!(diag.context_slice.is_some());
    let slice = diag.context_slice.as_ref().unwrap();
    assert_eq!(slice.focus_line, 6);

    let type_info = diag.type_info.as_ref().expect("Expected type_info");
    assert_eq!(type_info.expected, "str");
    assert_eq!(type_info.actual, "int");

    let patch = diag
        .suggested_patch
        .as_ref()
        .expect("Expected suggested_patch");
    assert_eq!(patch.patch_type, PatchType::AutoApply);
    assert_eq!(patch.confidence, 0.98);
    assert_eq!(patch.replacement.new_text, "email");
    assert!(patch.patch_unified.is_some());
    let diff = patch.patch_unified.as_ref().unwrap();
    assert!(diff.contains("-        let user = db.insert(User, {email: 12345, hash})"));
    assert!(diff.contains("+        let user = db.insert(User, {email: email, hash})"));
}
