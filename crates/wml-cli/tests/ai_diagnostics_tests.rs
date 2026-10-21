use std::fs;
use std::process::Command;

fn create_test_temp_dir(test_name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wml_test_{}_{}_{}",
        test_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("Failed to create tempdir");
    dir
}

#[test]
fn test_wml_check_ai_format_valid_auth() {
    let auth_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("auth.wml");

    let output = Command::new(env!("CARGO_BIN_EXE_wml"))
        .arg("check")
        .arg(&auth_path)
        .arg("--ai")
        .output()
        .expect("Failed to execute wml check");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Failed to parse JSON: {}\nOutput: {}", e, stdout));

    assert_eq!(json["protocol_version"], "1.0.0");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["summary"]["total_errors"], 0);
    assert!(json["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(json["compilation_metrics"]["target"], "check-only");
    assert!(
        json["compilation_metrics"]["lines_parsed"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        json["compilation_metrics"]["tokens_evaluated"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn test_wml_check_ai_format_type_mismatch_with_patch() {
    let temp_dir = create_test_temp_dir("type_mismatch");
    let test_file = temp_dir.join("mutated_auth.wml");

    let source = r#"
type User { id: int, email: str, hash: str }

route /auth/signup (post: {email: str, pass: str}) -> {token: str} {
    let hash = crypto.hash(pass)
    let user = db.insert(User, {email: 12345, hash})
    return {token: jwt.sign({sub: user.id})}
}
"#;
    fs::write(&test_file, source).expect("Failed to write test file");

    let output = Command::new(env!("CARGO_BIN_EXE_wml"))
        .arg("check")
        .arg(&test_file)
        .arg("--ai")
        .output()
        .expect("Failed to execute wml check");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Failed to parse JSON: {}\nOutput: {}", e, stdout));

    assert_eq!(json["protocol_version"], "1.0.0");
    assert_eq!(json["status"], "error");
    assert_eq!(json["summary"]["total_errors"], 1);
    assert_eq!(json["summary"]["auto_fixable"], 1);

    let diags = json["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 1);
    let diag = &diags[0];
    assert_eq!(diag["code"], "WML-TYP-2041");
    assert_eq!(diag["category"], "TypeMismatch");
    assert_eq!(diag["severity"], "error");
    assert!(diag["id"].as_str().unwrap().starts_with("diag_"));

    // Context slice
    assert_eq!(diag["context_slice"]["focus_line"], 6);

    // Type info
    assert_eq!(diag["type_info"]["expected"], "str");
    assert_eq!(diag["type_info"]["actual"], "int");

    // Suggested patch
    let patch = &diag["suggested_patch"];
    assert_eq!(patch["patch_type"], "AUTO_APPLY");
    assert_eq!(patch["confidence"], 0.98);
    assert_eq!(patch["replacement"]["new_text"], "email");
    assert!(patch["patch_unified"]
        .as_str()
        .unwrap()
        .contains("-    let user = db.insert(User, {email: 12345, hash})"));
    assert!(patch["patch_unified"]
        .as_str()
        .unwrap()
        .contains("+    let user = db.insert(User, {email: email, hash})"));
}

#[test]
fn test_wml_fix_dry_run_and_autonomous_repair() {
    let temp_dir = create_test_temp_dir("counter_typo");
    let test_file = temp_dir.join("counter_typo.wml");

    // Introduce an undefined variable typo `cout` instead of `count`
    let source = r#"
fn calculate() -> int {
    let count = 42
    return cout
}
"#;
    fs::write(&test_file, source).expect("Failed to write test file");

    // 1. Dry run: should NOT modify file
    let dry_run_output = Command::new(env!("CARGO_BIN_EXE_wml"))
        .arg("fix")
        .arg(&test_file)
        .arg("--dry-run")
        .arg("--ai")
        .output()
        .expect("Failed to execute wml fix dry-run");

    assert!(dry_run_output.status.success());
    let dry_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&dry_run_output.stdout)).unwrap();
    assert_eq!(dry_json["dry_run"], true);
    assert_eq!(dry_json["total_applied"], 1);

    // Confirm file was NOT modified
    let read_back_original = fs::read_to_string(&test_file).unwrap();
    assert_eq!(read_back_original, source);

    // 2. Real fix: autonomous 1-turn repair
    let fix_output = Command::new(env!("CARGO_BIN_EXE_wml"))
        .arg("fix")
        .arg(&test_file)
        .arg("--ai")
        .output()
        .expect("Failed to execute wml fix");

    assert!(fix_output.status.success());
    let fix_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&fix_output.stdout)).unwrap();
    assert_eq!(fix_json["status"], "ok");
    assert_eq!(fix_json["total_applied"], 1);
    assert_eq!(fix_json["remaining_errors"], 0);

    // Confirm file was modified and `cout` replaced with `count`
    let fixed_source = fs::read_to_string(&test_file).unwrap();
    assert!(!fixed_source.contains("return cout"));
    assert!(fixed_source.contains("return count"));

    // 3. Re-verify with check --ai
    let check_output = Command::new(env!("CARGO_BIN_EXE_wml"))
        .arg("check")
        .arg(&test_file)
        .arg("--ai")
        .output()
        .expect("Failed to re-check fixed file");

    assert!(check_output.status.success());
    let check_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&check_output.stdout)).unwrap();
    assert_eq!(check_json["status"], "ok");
    assert_eq!(check_json["summary"]["total_errors"], 0);
}

#[test]
fn test_wml_fix_syntax_jsx_tag_self_healing() {
    let temp_dir = create_test_temp_dir("syntax_tag");
    let test_file = temp_dir.join("syntax_tag.wml");

    // Introduce a mismatched closing tag </section> for <main>
    let source = r#"
page /test {
    <main>
        <h1>"Header"</h1>
    </section>
}
"#;
    fs::write(&test_file, source).expect("Failed to write test file");

    // Run wml fix --ai
    let fix_output = Command::new(env!("CARGO_BIN_EXE_wml"))
        .arg("fix")
        .arg(&test_file)
        .arg("--ai")
        .output()
        .expect("Failed to execute wml fix on syntax error");

    assert!(fix_output.status.success());
    let fix_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&fix_output.stdout)).unwrap();
    assert_eq!(fix_json["status"], "ok");
    assert_eq!(fix_json["total_applied"], 1);
    assert_eq!(fix_json["remaining_errors"], 0);

    // Confirm file now contains </main>
    let fixed_source = fs::read_to_string(&test_file).unwrap();
    assert!(fixed_source.contains("</main>"));
    assert!(!fixed_source.contains("</section>"));

    // Re-check passes
    let check_output = Command::new(env!("CARGO_BIN_EXE_wml"))
        .arg("check")
        .arg(&test_file)
        .arg("--ai")
        .output()
        .expect("Failed to check fixed syntax file");
    assert!(check_output.status.success());
}
