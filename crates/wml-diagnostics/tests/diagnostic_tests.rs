use wml_diagnostics::{
    apply_replacement, apply_replacements_batch, byte_to_line_col, compute_context_slice,
    generate_diagnostic_id, generate_unified_diff, ByteRange, CompilationMetrics, Diagnostic,
    DiagnosticCategory, DiagnosticReport, DiagnosticSeverity, DiagnosticStatus, Location,
    PatchType, Replacement, Span, SuggestedPatch, TypeInfo,
};

#[test]
fn test_deterministic_id_generation() {
    let range = ByteRange { start: 10, end: 25 };
    let id1 = generate_diagnostic_id("examples/auth.wml", &range, "WML-TYP-2001");
    let id2 = generate_diagnostic_id("examples/auth.wml", &range, "WML-TYP-2001");
    let id_windows = generate_diagnostic_id("examples\\auth.wml", &range, "WML-TYP-2001");
    let id3 = generate_diagnostic_id("examples/auth.wml", &range, "WML-TYP-2041");

    assert_eq!(id1, id2);
    assert_eq!(
        id1, id_windows,
        "Windows and Unix paths must produce identical deterministic IDs"
    );
    assert_ne!(id1, id3);
    assert!(id1.starts_with("diag_"));
    assert_eq!(id1.len(), 17); // "diag_" + 12 hex chars
}

#[test]
fn test_byte_to_line_col() {
    let source = "line 1\nline 2\nline 3";
    assert_eq!(byte_to_line_col(source, 0), (1, 1));
    assert_eq!(byte_to_line_col(source, 7), (2, 1));
    assert_eq!(byte_to_line_col(source, 14), (3, 1));
}

#[test]
fn test_compute_context_slice() {
    let source = "line 1\nline 2\nline 3\nline 4\nline 5";
    let slice = compute_context_slice(source, 3, 1);

    assert_eq!(slice.focus_line, 3);
    assert_eq!(slice.lines.len(), 3);
    assert_eq!(slice.lines[0].line_number, 2);
    assert_eq!(slice.lines[0].is_focus, Some(false));
    assert_eq!(slice.lines[1].line_number, 3);
    assert_eq!(slice.lines[1].is_focus, Some(true));
    assert_eq!(slice.lines[2].line_number, 4);
    assert_eq!(slice.lines[2].is_focus, Some(false));
}

#[test]
fn test_unified_diff_generation() {
    let source =
        "fn foo() {\n    let user = db.insert(User, {email: 12345, hash})\n    return user\n}";
    let target_needle = "12345";
    let start = source.find(target_needle).unwrap();
    let end = start + target_needle.len();
    let range = ByteRange { start, end };

    let diff = generate_unified_diff("examples/auth.wml", source, &range, "email");
    assert!(diff.contains("--- examples/auth.wml"));
    assert!(diff.contains("+++ examples/auth.wml"));
    assert!(diff.contains("@@ -2,1 +2,1 @@"));
    assert!(diff.contains("-    let user = db.insert(User, {email: 12345, hash})"));
    assert!(diff.contains("+    let user = db.insert(User, {email: email, hash})"));
}

#[test]
fn test_apply_replacement_and_batch() {
    let source = "let a = 1\nlet b = 2\nlet c = 3";

    // Single replacement
    let rep_b = Replacement {
        file: "test.wml".to_string(),
        byte_range: ByteRange {
            start: source.find('2').unwrap(),
            end: source.find('2').unwrap() + 1,
        },
        new_text: "20".to_string(),
    };
    let single_res = apply_replacement(source, &rep_b).unwrap();
    assert_eq!(single_res, "let a = 1\nlet b = 20\nlet c = 3");

    // Batch replacements (applied in correct descending order without offset skew)
    let rep_a = Replacement {
        file: "test.wml".to_string(),
        byte_range: ByteRange {
            start: source.find('1').unwrap(),
            end: source.find('1').unwrap() + 1,
        },
        new_text: "100".to_string(),
    };
    let rep_c = Replacement {
        file: "test.wml".to_string(),
        byte_range: ByteRange {
            start: source.find('3').unwrap(),
            end: source.find('3').unwrap() + 1,
        },
        new_text: "3000".to_string(),
    };

    let batch_res = apply_replacements_batch(source, &[rep_a, rep_b, rep_c]).unwrap();
    assert_eq!(batch_res, "let a = 100\nlet b = 20\nlet c = 3000");
}

#[test]
fn test_diagnostic_report_json_schema_conformance() {
    let range = ByteRange {
        start: 178,
        end: 183,
    };
    let span = Span {
        start_line: 6,
        start_col: 37,
        end_line: 6,
        end_col: 42,
    };
    let loc = Location {
        file: "F:/DEV/WML/examples/auth.wml".to_string(),
        span,
        byte_range: range,
    };

    let patch = SuggestedPatch {
        patch_type: PatchType::AutoApply,
        confidence: 0.98,
        explanation: "Replace integer literal with variable `email`".to_string(),
        replacement: Replacement {
            file: "F:/DEV/WML/examples/auth.wml".to_string(),
            byte_range: range,
            new_text: "email".to_string(),
        },
        patch_unified: Some("--- diff".to_string()),
    };

    let diag = Diagnostic::new(
        "diag_8f0a2d4e7c1b".to_string(),
        "WML-TYP-2041".to_string(),
        DiagnosticCategory::TypeMismatch,
        DiagnosticSeverity::Error,
        "Mismatched types".to_string(),
        loc,
    )
    .with_patch(patch)
    .with_type_info(TypeInfo {
        expected: "str".to_string(),
        actual: "int".to_string(),
        coercible: Some(true),
    });

    let report = DiagnosticReport::new(
        DiagnosticStatus::Error,
        "2026-10-21T15:05:00Z".to_string(),
        vec![diag],
    )
    .with_metrics(CompilationMetrics {
        duration_ms: 1.42,
        lines_parsed: 19,
        tokens_evaluated: 118,
        target: "check-only".to_string(),
    });

    let json_str = serde_json::to_string_pretty(&report).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed["protocol_version"], "1.0.0");
    assert_eq!(parsed["status"], "error");
    assert_eq!(parsed["diagnostics"][0]["code"], "WML-TYP-2041");
    assert_eq!(
        parsed["diagnostics"][0]["suggested_patch"]["patch_type"],
        "AUTO_APPLY"
    );
    assert_eq!(parsed["diagnostics"][0]["type_info"]["expected"], "str");
    assert_eq!(parsed["compilation_metrics"]["target"], "check-only");
}

#[test]
fn test_utf8_char_boundary_and_collision_detection() {
    let source = "let greeting = \"🚀 hello world\"";
    // The rocket emoji is 4 bytes (0..4 in string literal: " at 15, rocket at 16..20)
    let rocket_start = source.find('🚀').unwrap();

    // Slicing in middle of 4-byte UTF-8 character must fail safely without panic
    let invalid_rep = Replacement {
        file: "test.wml".to_string(),
        byte_range: ByteRange {
            start: rocket_start + 1,
            end: rocket_start + 3,
        },
        new_text: "fail".to_string(),
    };
    let err = apply_replacement(source, &invalid_rep);
    assert!(err.is_err());
    assert!(err.unwrap_err().contains("UTF-8 character boundaries"));

    // Colliding replacements sharing identical start offset must be caught
    let rep1 = Replacement {
        file: "test.wml".to_string(),
        byte_range: ByteRange { start: 10, end: 15 },
        new_text: "a".to_string(),
    };
    let rep2 = Replacement {
        file: "test.wml".to_string(),
        byte_range: ByteRange { start: 10, end: 12 },
        new_text: "b".to_string(),
    };
    let coll_err = apply_replacements_batch(source, &[rep1, rep2]);
    assert!(coll_err.is_err());
    assert!(coll_err
        .unwrap_err()
        .contains("Overlapping replacements detected"));
}
