use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use wml_client::ClientTranspiler;
use wml_codegen_cranelift::CraneliftJitBackend;
use wml_codegen_llvm::{LlvmCompiler, TargetConfig, TargetOs};
use wml_diagnostics::{
    apply_replacements_batch, byte_to_line_col, compute_context_slice, generate_diagnostic_id,
    generate_unified_diff, ByteRange, CompilationMetrics, Diagnostic, DiagnosticCategory,
    DiagnosticReport, DiagnosticSeverity, DiagnosticStatus, DiagnosticSummary, Location, PatchType,
    Replacement, Span, SuggestedPatch,
};
use wml_hir::check;
use wml_mir::{EscapeAnalysis, MirLowerer, MirOptimizer};
use wml_runtime::abi::{WmlHttpRequest, WmlHttpResponse, WmlSlice};
use wml_runtime::arena::RequestArena;
use wml_runtime::{HttpReactor, RouteHandlerFn};
use wml_syntax::ast::Item;
use wml_syntax::parse;

static ISLAND_JS: OnceLock<&'static [u8]> = OnceLock::new();

unsafe extern "C" fn islands_asset_handler(
    _arena: *mut RequestArena,
    _req: *const WmlHttpRequest,
    res: *mut WmlHttpResponse,
) -> i64 {
    if let Some(&bytes) = ISLAND_JS.get() {
        (*res).status = 200;
        (*res).content_type = WmlSlice::from_slice(b"application/javascript; charset=utf-8");
        (*res).body = WmlSlice::from_slice(bytes);
    }
    0
}

#[derive(Parser, Debug)]
#[command(
    name = "wml",
    version = "0.1.0",
    about = "Web Minimalistic Language Compiler & Runtime"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Typecheck and validate a WML file
    Check {
        /// Path to the .wml file
        file: PathBuf,
        /// Output format (human or ai-json)
        #[arg(long, default_value = "human")]
        format: String,
        /// Alias for --format=ai-json
        #[arg(long)]
        ai: bool,
    },
    /// Automatically fix diagnostic errors using suggested AI patches
    Fix {
        /// Path to the .wml file
        file: PathBuf,
        /// Minimum confidence threshold to apply patches (default: 0.8)
        #[arg(long, default_value_t = 0.8)]
        min_confidence: f64,
        /// Show proposed patches and unified diffs without modifying file
        #[arg(long)]
        dry_run: bool,
        /// Output format (human or ai-json)
        #[arg(long, default_value = "human")]
        format: String,
        /// Alias for --format=ai-json
        #[arg(long)]
        ai: bool,
    },
    /// Start JIT dev server with instant live reload
    Dev {
        /// Path to the entry .wml file
        file: PathBuf,
        /// Port to bind to
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
        /// Output format (human or ai-json)
        #[arg(long, default_value = "human")]
        format: String,
        /// Exit immediately after compiling and binding (useful for agent verification)
        #[arg(long)]
        dry_run: bool,
    },
    /// Compile a static standalone binary or production bundle
    Build {
        /// Path to the entry .wml file
        file: PathBuf,
        /// Release mode with LTO
        #[arg(long)]
        release: bool,
        /// Target architecture (e.g. x86_64, aarch64, arm64)
        #[arg(long)]
        target: Option<String>,
        /// Output directory for production build
        #[arg(short, long, default_value = "dist")]
        out_dir: PathBuf,
        /// Output format (human or ai-json)
        #[arg(long, default_value = "human")]
        format: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let current_timestamp = chrono::Utc::now().to_rfc3339();

    match cli.command {
        Commands::Check { file, format, ai } => {
            let start_instant = std::time::Instant::now();
            let is_ai = ai || format == "ai-json";
            let file_str = file.display().to_string();

            let source = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(err) => {
                    let duration_ms = (start_instant.elapsed().as_nanos() as f64) / 1_000_000.0;
                    if is_ai {
                        let report = DiagnosticReport {
                            protocol_version: "1.0.0".to_string(),
                            status: DiagnosticStatus::Error,
                            timestamp: current_timestamp.clone(),
                            summary: Some(DiagnosticSummary {
                                total_errors: 1,
                                total_warnings: 0,
                                auto_fixable: 0,
                            }),
                            compilation_metrics: Some(CompilationMetrics {
                                duration_ms,
                                lines_parsed: 0,
                                tokens_evaluated: 0,
                                target: "check-only".to_string(),
                            }),
                            diagnostics: vec![Diagnostic::new(
                                "diag_io_001".to_string(),
                                "WML-IO-1001".to_string(),
                                DiagnosticCategory::RuntimePanic,
                                DiagnosticSeverity::Error,
                                format!("Failed to read file {}: {}", file.display(), err),
                                Location {
                                    file: file_str.clone(),
                                    span: Span {
                                        start_line: 1,
                                        start_col: 1,
                                        end_line: 1,
                                        end_col: 1,
                                    },
                                    byte_range: ByteRange { start: 0, end: 0 },
                                },
                            )],
                        };
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        eprintln!("Error reading file {}: {}", file.display(), err);
                    }
                    std::process::exit(1);
                }
            };

            let lines_parsed = source.lines().count();
            let tokens_evaluated = wml_syntax::tokenize(&source).map(|t| t.len()).unwrap_or(0);

            match parse(&source) {
                Ok((program, arena)) => {
                    let (_hir, diags) = wml_hir::check(&program, &arena, &source, &file_str);
                    let duration_ms = (start_instant.elapsed().as_nanos() as f64) / 1_000_000.0;

                    let total_errors = diags
                        .iter()
                        .filter(|d| d.severity == DiagnosticSeverity::Error)
                        .count();
                    let total_warnings = diags
                        .iter()
                        .filter(|d| d.severity == DiagnosticSeverity::Warning)
                        .count();
                    let auto_fixable = diags.iter().filter(|d| d.suggested_patch.is_some()).count();

                    if is_ai {
                        let status = if total_errors > 0 {
                            DiagnosticStatus::Error
                        } else if total_warnings > 0 {
                            DiagnosticStatus::Warning
                        } else {
                            DiagnosticStatus::Success
                        };

                        let report = DiagnosticReport {
                            protocol_version: "1.0.0".to_string(),
                            status,
                            timestamp: current_timestamp.clone(),
                            summary: Some(DiagnosticSummary {
                                total_errors,
                                total_warnings,
                                auto_fixable,
                            }),
                            compilation_metrics: Some(CompilationMetrics {
                                duration_ms,
                                lines_parsed,
                                tokens_evaluated,
                                target: "check-only".to_string(),
                            }),
                            diagnostics: diags,
                        };
                        println!("{}", serde_json::to_string_pretty(&report)?);
                        if total_errors > 0 {
                            std::process::exit(1);
                        }
                    } else {
                        if total_errors > 0 {
                            for d in &diags {
                                eprintln!(
                                    "Error [{}] in {}:{}:{}: {}",
                                    d.code,
                                    d.primary_location.file,
                                    d.primary_location.span.start_line,
                                    d.primary_location.span.start_col,
                                    d.message
                                );
                                if let Some(slice) = &d.context_slice {
                                    for line in &slice.lines {
                                        let prefix = if line.is_focus == Some(true) {
                                            ">"
                                        } else {
                                            " "
                                        };
                                        eprintln!(
                                            "  {} {:4} | {}",
                                            prefix, line.line_number, line.content
                                        );
                                    }
                                }
                                if let Some(patch) = &d.suggested_patch {
                                    eprintln!("  Suggested fix: {}", patch.explanation);
                                }
                            }
                            std::process::exit(1);
                        } else {
                            println!(
                                "Checking {}... OK ({} top-level items parsed and typechecked, 0 errors)",
                                file.display(),
                                program.items.len()
                            );
                        }
                    }
                }
                Err(parse_err) => {
                    let duration_ms = (start_instant.elapsed().as_nanos() as f64) / 1_000_000.0;
                    let (start_line, start_col) = byte_to_line_col(&source, parse_err.span.start);
                    let (end_line, end_col) = byte_to_line_col(&source, parse_err.span.end);
                    let byte_range = ByteRange {
                        start: parse_err.span.start,
                        end: parse_err.span.end,
                    };

                    let diag_id = generate_diagnostic_id(&file_str, &byte_range, "WML-SYN-1001");
                    let context_slice = compute_context_slice(&source, start_line, 1);

                    // Check for auto-fixable syntax errors
                    let mut patch = None;
                    if parse_err
                        .message
                        .starts_with("Mismatched JSX closing tag: expected `</")
                    {
                        if let Some(expected_tag) = parse_err
                            .message
                            .split("expected `</")
                            .nth(1)
                            .and_then(|s| s.split('>').next())
                        {
                            patch = Some(SuggestedPatch {
                                patch_type: PatchType::AutoApply,
                                confidence: 1.0,
                                explanation: format!("Change closing tag to `</{}>`", expected_tag),
                                replacement: Replacement {
                                    file: file_str.clone(),
                                    byte_range,
                                    new_text: expected_tag.to_string(),
                                },
                                patch_unified: Some(generate_unified_diff(
                                    &file_str,
                                    &source,
                                    &byte_range,
                                    expected_tag,
                                )),
                            });
                        }
                    }

                    let diagnostic = Diagnostic::new(
                        diag_id,
                        "WML-SYN-1001".to_string(),
                        DiagnosticCategory::SyntaxError,
                        DiagnosticSeverity::Error,
                        parse_err.message.clone(),
                        Location {
                            file: file_str.clone(),
                            span: Span {
                                start_line,
                                start_col,
                                end_line,
                                end_col,
                            },
                            byte_range,
                        },
                    )
                    .with_context_slice(context_slice);

                    let diagnostic = if let Some(p) = patch {
                        diagnostic.with_patch(p)
                    } else {
                        diagnostic
                    };

                    if is_ai {
                        let report = DiagnosticReport {
                            protocol_version: "1.0.0".to_string(),
                            status: DiagnosticStatus::Error,
                            timestamp: current_timestamp.clone(),
                            summary: Some(DiagnosticSummary {
                                total_errors: 1,
                                total_warnings: 0,
                                auto_fixable: if diagnostic.suggested_patch.is_some() {
                                    1
                                } else {
                                    0
                                },
                            }),
                            compilation_metrics: Some(CompilationMetrics {
                                duration_ms,
                                lines_parsed,
                                tokens_evaluated,
                                target: "check-only".to_string(),
                            }),
                            diagnostics: vec![diagnostic],
                        };
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        eprintln!(
                            "Error in {}:{}:{}: {}",
                            file.display(),
                            start_line,
                            start_col,
                            parse_err.message
                        );
                    }
                    std::process::exit(1);
                }
            }
        }
        Commands::Fix {
            file,
            min_confidence,
            dry_run,
            format,
            ai,
        } => {
            let is_ai = ai || format == "ai-json";
            let file_str = file.display().to_string();
            let mut current_source = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(err) => {
                    if is_ai {
                        let report = serde_json::json!({
                            "protocol_version": "1.0.0",
                            "status": "error",
                            "error": format!("Failed to read file {}: {}", file.display(), err),
                            "timestamp": current_timestamp.clone(),
                        });
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        eprintln!("Error reading file {}: {}", file.display(), err);
                    }
                    std::process::exit(1);
                }
            };

            let mut all_applied_patches: Vec<serde_json::Value> = Vec::new();
            let mut iteration = 0;
            let max_iterations = 5;

            loop {
                iteration += 1;
                if iteration > max_iterations {
                    break;
                }

                // Parse and check current source
                let diags = match parse(&current_source) {
                    Ok((program, arena)) => {
                        let (_hir, d) =
                            wml_hir::check(&program, &arena, &current_source, &file_str);
                        d
                    }
                    Err(parse_err) => {
                        let (start_line, start_col) =
                            byte_to_line_col(&current_source, parse_err.span.start);
                        let (end_line, end_col) =
                            byte_to_line_col(&current_source, parse_err.span.end);
                        let byte_range = ByteRange {
                            start: parse_err.span.start,
                            end: parse_err.span.end,
                        };
                        let diag_id =
                            generate_diagnostic_id(&file_str, &byte_range, "WML-SYN-1001");
                        let context_slice = compute_context_slice(&current_source, start_line, 1);

                        let mut patch = None;
                        if parse_err
                            .message
                            .starts_with("Mismatched JSX closing tag: expected `</")
                        {
                            if let Some(expected_tag) = parse_err
                                .message
                                .split("expected `</")
                                .nth(1)
                                .and_then(|s| s.split('>').next())
                            {
                                patch = Some(SuggestedPatch {
                                    patch_type: PatchType::AutoApply,
                                    confidence: 1.0,
                                    explanation: format!(
                                        "Change closing tag to `</{}>`",
                                        expected_tag
                                    ),
                                    replacement: Replacement {
                                        file: file_str.clone(),
                                        byte_range,
                                        new_text: expected_tag.to_string(),
                                    },
                                    patch_unified: Some(generate_unified_diff(
                                        &file_str,
                                        &current_source,
                                        &byte_range,
                                        expected_tag,
                                    )),
                                });
                            }
                        }

                        let diag = Diagnostic::new(
                            diag_id,
                            "WML-SYN-1001".to_string(),
                            DiagnosticCategory::SyntaxError,
                            DiagnosticSeverity::Error,
                            parse_err.message,
                            Location {
                                file: file_str.clone(),
                                span: Span {
                                    start_line,
                                    start_col,
                                    end_line,
                                    end_col,
                                },
                                byte_range,
                            },
                        )
                        .with_context_slice(context_slice);

                        let diag = if let Some(p) = patch {
                            diag.with_patch(p)
                        } else {
                            diag
                        };

                        vec![diag]
                    }
                };

                // Find candidate patches
                let candidate_patches: Vec<SuggestedPatch> = diags
                    .into_iter()
                    .filter_map(|d| d.suggested_patch)
                    .filter(|p| {
                        p.patch_type == PatchType::AutoApply && p.confidence >= min_confidence
                    })
                    .collect();

                if candidate_patches.is_empty() {
                    break;
                }

                // If dry_run, just record proposed patches and stop
                if dry_run {
                    for p in &candidate_patches {
                        all_applied_patches.push(serde_json::json!({
                            "patch_type": "AUTO_APPLY",
                            "confidence": p.confidence,
                            "explanation": p.explanation,
                            "replacement": {
                                "byte_range": p.replacement.byte_range,
                                "new_text": p.replacement.new_text,
                            },
                            "patch_unified": p.patch_unified,
                        }));
                    }
                    break;
                }

                // Collect replacements and apply batch
                let replacements: Vec<Replacement> = candidate_patches
                    .iter()
                    .map(|p| p.replacement.clone())
                    .collect();
                match apply_replacements_batch(&current_source, &replacements) {
                    Ok(new_source) => {
                        for p in &candidate_patches {
                            all_applied_patches.push(serde_json::json!({
                                "patch_type": "AUTO_APPLY",
                                "confidence": p.confidence,
                                "explanation": p.explanation,
                                "replacement": {
                                    "byte_range": p.replacement.byte_range,
                                    "new_text": p.replacement.new_text,
                                },
                                "patch_unified": p.patch_unified,
                            }));
                        }
                        current_source = new_source;
                    }
                    Err(err) => {
                        if is_ai {
                            let report = serde_json::json!({
                                "protocol_version": "1.0.0",
                                "status": "error",
                                "error": format!("Failed to apply patch batch: {}", err),
                                "timestamp": current_timestamp.clone(),
                            });
                            println!("{}", serde_json::to_string_pretty(&report)?);
                        } else {
                            eprintln!("Error applying patches: {}", err);
                        }
                        std::process::exit(1);
                    }
                }
            }

            // If not dry_run and patches were applied, write to disk
            if !dry_run && !all_applied_patches.is_empty() {
                if let Err(e) = fs::write(&file, &current_source) {
                    eprintln!(
                        "Failed to write updated source to {}: {}",
                        file.display(),
                        e
                    );
                    std::process::exit(1);
                }
            }

            // Final re-check to determine remaining errors
            let final_errors = match parse(&current_source) {
                Ok((prog, ar)) => {
                    let (_, d) = wml_hir::check(&prog, &ar, &current_source, &file_str);
                    d.iter()
                        .filter(|x| x.severity == DiagnosticSeverity::Error)
                        .count()
                }
                Err(_) => 1,
            };

            if is_ai {
                let report = serde_json::json!({
                    "protocol_version": "1.0.0",
                    "status": if final_errors == 0 { "ok" } else { "error" },
                    "file": file_str,
                    "dry_run": dry_run,
                    "total_applied": all_applied_patches.len(),
                    "remaining_errors": final_errors,
                    "applied_patches": all_applied_patches,
                    "timestamp": current_timestamp.clone(),
                });
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                if dry_run {
                    println!(
                        "Dry-run mode: {} auto-fixable patch(es) found for {}:",
                        all_applied_patches.len(),
                        file.display()
                    );
                    for (i, p) in all_applied_patches.iter().enumerate() {
                        let exp = p["explanation"].as_str().unwrap_or("");
                        let conf = p["confidence"].as_f64().unwrap_or(0.0) * 100.0;
                        println!("  {}. {} (confidence: {:.0}%)", i + 1, exp, conf);
                        if let Some(diff) = p["patch_unified"].as_str() {
                            println!("\n{}\n", diff);
                        }
                    }
                } else if all_applied_patches.is_empty() {
                    println!(
                        "No auto-fixable patches available for {} (remaining errors: {}).",
                        file.display(),
                        final_errors
                    );
                } else {
                    println!(
                        "Applied {} fix(es) to {}:",
                        all_applied_patches.len(),
                        file.display()
                    );
                    for (i, p) in all_applied_patches.iter().enumerate() {
                        let exp = p["explanation"].as_str().unwrap_or("");
                        let conf = p["confidence"].as_f64().unwrap_or(0.0) * 100.0;
                        println!("  {}. {} (confidence: {:.0}%)", i + 1, exp, conf);
                    }
                    if final_errors == 0 {
                        println!("Re-checking {}... OK (0 errors remaining)", file.display());
                    } else {
                        println!(
                            "Re-checking {}... {} error(s) remaining",
                            file.display(),
                            final_errors
                        );
                    }
                }
            }
        }
        Commands::Dev {
            file,
            port,
            format,
            dry_run,
        } => {
            let start_time = std::time::Instant::now();
            let file_str = file.display().to_string();

            let source = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    if format == "ai-json" {
                        let diagnostic = Diagnostic::new(
                            "diag_io_001".to_string(),
                            "WML-IO-1001".to_string(),
                            DiagnosticCategory::SyntaxError,
                            DiagnosticSeverity::Error,
                            format!("Failed to read file '{}': {}", file_str, e),
                            Location {
                                file: file_str.clone(),
                                span: Span {
                                    start_line: 1,
                                    start_col: 1,
                                    end_line: 1,
                                    end_col: 1,
                                },
                                byte_range: ByteRange { start: 0, end: 0 },
                            },
                        );
                        let report = DiagnosticReport::new(
                            DiagnosticStatus::Error,
                            current_timestamp.clone(),
                            vec![diagnostic],
                        );
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        eprintln!("Error reading file {}: {}", file_str, e);
                    }
                    std::process::exit(1);
                }
            };

            // 1. Parse
            let (program, arena) = match parse(&source) {
                Ok(res) => res,
                Err(parse_err) => {
                    let (start_line, start_col) = byte_to_line_col(&source, parse_err.span.start);
                    let (end_line, end_col) = byte_to_line_col(&source, parse_err.span.end);

                    let diagnostic = Diagnostic::new(
                        format!("diag_{:x}", parse_err.span.start),
                        "WML-SYN-1001".to_string(),
                        DiagnosticCategory::SyntaxError,
                        DiagnosticSeverity::Error,
                        parse_err.message.clone(),
                        Location {
                            file: file_str.clone(),
                            span: Span {
                                start_line,
                                start_col,
                                end_line,
                                end_col,
                            },
                            byte_range: ByteRange {
                                start: parse_err.span.start,
                                end: parse_err.span.end,
                            },
                        },
                    );

                    if format == "ai-json" {
                        let report = DiagnosticReport::new(
                            DiagnosticStatus::Error,
                            current_timestamp.clone(),
                            vec![diagnostic],
                        );
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        eprintln!(
                            "Error in {}:{}:{}: {}",
                            file_str, start_line, start_col, parse_err.message
                        );
                    }
                    std::process::exit(1);
                }
            };

            // 2. Typecheck
            let (hir, diags) = check(&program, &arena, &source, &file_str);
            let has_errors = diags
                .iter()
                .any(|d| d.severity == DiagnosticSeverity::Error);
            if has_errors {
                if format == "ai-json" {
                    let report = DiagnosticReport::new(
                        DiagnosticStatus::Error,
                        current_timestamp.clone(),
                        diags,
                    );
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    for d in &diags {
                        eprintln!("[{}] {}", d.code, d.message);
                    }
                }
                std::process::exit(1);
            }

            // 3. Lower HIR to SSA MIR
            let lowerer = MirLowerer::new();
            let mut mir_module = lowerer.lower_program(&hir);

            // 4. Escape Analysis & Optimizations
            let escape = EscapeAnalysis::new();
            escape.run_on_module(&mut mir_module);

            let opt = MirOptimizer::new();
            opt.optimize_module(&mut mir_module);

            // 5. Cranelift JIT compilation
            let mut jit_backend = CraneliftJitBackend::new()
                .map_err(|e| anyhow::anyhow!("Cranelift JIT init error: {}", e))?;
            let compiled = jit_backend
                .compile_module(&mir_module)
                .map_err(|e| anyhow::anyhow!("JIT compilation error: {}", e))?;

            // 6. Setup HTTP Reactor
            let mut reactor = HttpReactor::new();
            let mut routes_meta = Vec::new();

            for route in &compiled.routes {
                let handler_fn: RouteHandlerFn = unsafe { std::mem::transmute(route.fn_ptr) };
                reactor.add_route(&route.method, &route.path, handler_fn);
                routes_meta.push(serde_json::json!({
                    "method": route.method,
                    "path": route.path,
                }));
            }

            // 7. Transpile UI islands into bundled JavaScript & register asset route
            let uis: Vec<_> = program
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Ui(ui) => Some(ui),
                    _ => None,
                })
                .collect();

            if !uis.is_empty() {
                let transpiler = ClientTranspiler::new();
                if let Ok(bundle) = transpiler.bundle_islands(&uis, &arena) {
                    let static_bytes: &'static [u8] =
                        Box::leak(bundle.into_bytes().into_boxed_slice());
                    let _ = ISLAND_JS.set(static_bytes);
                    reactor.add_route("GET", "/_wml/assets/islands.js", islands_asset_handler);
                    routes_meta.push(serde_json::json!({
                        "method": "GET",
                        "path": "/_wml/assets/islands.js",
                    }));
                }
            }

            let bind_addr = format!("127.0.0.1:{}", port);
            let listener = std::net::TcpListener::bind(&bind_addr)
                .map_err(|e| anyhow::anyhow!("Failed to bind {}: {}", bind_addr, e))?;

            let compile_duration = start_time.elapsed().as_secs_f64() * 1000.0;

            if format == "ai-json" {
                let ready_event = serde_json::json!({
                    "event": "server_ready",
                    "port": port,
                    "host": "127.0.0.1",
                    "routes": routes_meta,
                    "compile_duration_ms": compile_duration,
                    "timestamp": current_timestamp,
                });
                println!("{}", serde_json::to_string(&ready_event)?);
            } else {
                println!(
                    "⚡ [WML JIT] Compiled {} routes in {:.2}ms",
                    routes_meta.len(),
                    compile_duration
                );
                println!("🚀 [WML Server] Listening on http://{}", bind_addr);
            }

            if !dry_run {
                reactor.serve(listener)?;
            }
        }
        Commands::Build {
            file,
            release,
            target,
            out_dir,
            format,
        } => {
            let start_time = std::time::Instant::now();
            let file_str = file.display().to_string();

            let source = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    if format == "ai-json" {
                        let diagnostic = Diagnostic::new(
                            "diag_io_001".to_string(),
                            "WML-IO-1001".to_string(),
                            DiagnosticCategory::SyntaxError,
                            DiagnosticSeverity::Error,
                            format!("Failed to read file '{}': {}", file_str, e),
                            Location {
                                file: file_str.clone(),
                                span: Span {
                                    start_line: 1,
                                    start_col: 1,
                                    end_line: 1,
                                    end_col: 1,
                                },
                                byte_range: ByteRange { start: 0, end: 0 },
                            },
                        );
                        let report = DiagnosticReport::new(
                            DiagnosticStatus::Error,
                            current_timestamp.clone(),
                            vec![diagnostic],
                        );
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        eprintln!("Error reading file {}: {}", file_str, e);
                    }
                    std::process::exit(1);
                }
            };

            // 1. Parse AST
            let (program, arena) = match parse(&source) {
                Ok(res) => res,
                Err(parse_err) => {
                    let (start_line, start_col) = byte_to_line_col(&source, parse_err.span.start);
                    let (end_line, end_col) = byte_to_line_col(&source, parse_err.span.end);

                    let diagnostic = Diagnostic::new(
                        format!("diag_{:x}", parse_err.span.start),
                        "WML-SYN-1001".to_string(),
                        DiagnosticCategory::SyntaxError,
                        DiagnosticSeverity::Error,
                        parse_err.message.clone(),
                        Location {
                            file: file_str.clone(),
                            span: Span {
                                start_line,
                                start_col,
                                end_line,
                                end_col,
                            },
                            byte_range: ByteRange {
                                start: parse_err.span.start,
                                end: parse_err.span.end,
                            },
                        },
                    );

                    if format == "ai-json" {
                        let report = DiagnosticReport::new(
                            DiagnosticStatus::Error,
                            current_timestamp.clone(),
                            vec![diagnostic],
                        );
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        eprintln!(
                            "Error in {}:{}:{}: {}",
                            file_str, start_line, start_col, parse_err.message
                        );
                    }
                    std::process::exit(1);
                }
            };

            // 2. Typecheck
            let (hir, diags) = check(&program, &arena, &source, &file_str);
            let has_errors = diags
                .iter()
                .any(|d| d.severity == DiagnosticSeverity::Error);
            if has_errors {
                if format == "ai-json" {
                    let report = DiagnosticReport::new(
                        DiagnosticStatus::Error,
                        current_timestamp.clone(),
                        diags,
                    );
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    for d in &diags {
                        eprintln!("[{}] {}", d.code, d.message);
                    }
                }
                std::process::exit(1);
            }

            // 3. Lower HIR to SSA MIR
            let lowerer = MirLowerer::new();
            let mut mir_module = lowerer.lower_program(&hir);

            // 4. Escape Analysis & Optimizations
            let escape = EscapeAnalysis::new();
            escape.run_on_module(&mut mir_module);

            let opt = MirOptimizer::new();
            opt.optimize_module(&mut mir_module);

            // 5. Create output directories
            fs::create_dir_all(&out_dir)?;
            let assets_dir = out_dir.join("assets");
            fs::create_dir_all(&assets_dir)?;

            // 6. Transpile UI islands into bundle
            let uis: Vec<_> = program
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Ui(ui) => Some(ui),
                    _ => None,
                })
                .collect();

            let mut assets_meta = Vec::new();
            let mut islands_meta = Vec::new();

            if !uis.is_empty() {
                let transpiler = ClientTranspiler::new();
                let bundle = transpiler
                    .bundle_islands(&uis, &arena)
                    .map_err(|e| anyhow::anyhow!("Island bundling error: {}", e))?;
                let bundle_path = assets_dir.join("islands.js");
                fs::write(&bundle_path, &bundle)?;
                assets_meta.push(serde_json::json!({
                    "name": "islands.js",
                    "path": bundle_path.display().to_string(),
                    "size_bytes": bundle.len(),
                    "type": "javascript",
                }));
                for ui in &uis {
                    islands_meta.push(ui.name.clone());
                }
            }

            // 7. LLVM AOT Compilation for Production Release
            let target_config = if let Some(t) = &target {
                match TargetConfig::parse(t) {
                    Some(cfg) => cfg,
                    None => {
                        anyhow::bail!(
                            "Unrecognized target '{}'. Supported architectures: x86_64, aarch64 (arm64)",
                            t
                        );
                    }
                }
            } else {
                TargetConfig::host()
            };

            let mut llvm_ir_meta = None;
            let mut binary_meta = None;

            if release {
                let llvm_compiler = LlvmCompiler::new();
                let ir = llvm_compiler
                    .compile_to_ir(&mir_module, &target_config)
                    .map_err(|e| anyhow::anyhow!("LLVM IR generation failed: {}", e))?;

                let ir_path = out_dir.join("app.ll");
                fs::write(&ir_path, &ir)?;
                llvm_ir_meta = Some(serde_json::json!({
                    "path": ir_path.display().to_string(),
                    "size_bytes": ir.len(),
                    "target_triple": target_config.triple(),
                }));

                if LlvmCompiler::is_clang_available() {
                    let bin_dir = out_dir.join("bin");
                    fs::create_dir_all(&bin_dir)?;
                    let obj_path = bin_dir.join(format!(
                        "app.{}",
                        if target_config.os == TargetOs::Windows {
                            "obj"
                        } else {
                            "o"
                        }
                    ));
                    let compiled_path = llvm_compiler
                        .compile_to_object(&ir, &target_config, &obj_path)
                        .map_err(|e| anyhow::anyhow!("Clang compilation failed: {}", e))?;

                    let sz = fs::metadata(&compiled_path).map(|m| m.len()).unwrap_or(0);
                    binary_meta = Some(serde_json::json!({
                        "path": compiled_path.display().to_string(),
                        "size_bytes": sz,
                        "target_triple": target_config.triple(),
                        "status": "compiled_object",
                    }));
                }
            }

            // 8. Write manifest.json
            let pages_meta: Vec<String> = mir_module
                .routes
                .iter()
                .filter(|r| r.method == "GET" && !r.path.starts_with("/_wml"))
                .map(|r| r.path.clone())
                .collect();

            let manifest_path = out_dir.join("manifest.json");
            let manifest = serde_json::json!({
                "version": "1.0.0",
                "entry": file_str,
                "release": release,
                "target_triple": target_config.triple(),
                "routes": mir_module.routes.iter().map(|r| serde_json::json!({
                    "method": r.method,
                    "path": r.path,
                })).collect::<Vec<_>>(),
                "pages": pages_meta,
                "islands": islands_meta,
                "assets": assets_meta,
                "llvm_ir": llvm_ir_meta,
                "binary": binary_meta,
                "timestamp": current_timestamp,
            });
            fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

            let build_duration = start_time.elapsed().as_secs_f64() * 1000.0;

            if format == "ai-json" {
                let build_event = serde_json::json!({
                    "event": "build_complete",
                    "target": out_dir.display().to_string(),
                    "release": release,
                    "target_triple": target_config.triple(),
                    "pages": pages_meta,
                    "islands": islands_meta,
                    "assets": assets_meta,
                    "llvm_ir": llvm_ir_meta,
                    "binary": binary_meta,
                    "build_duration_ms": build_duration,
                    "timestamp": current_timestamp,
                });
                println!("{}", serde_json::to_string(&build_event)?);
            } else {
                println!("⚡ [WML Build] Successfully compiled {}", file_str);
                println!("📦 Target directory: {}", out_dir.display());
                if release {
                    println!("🚀 Release Target Triple: {}", target_config.triple());
                    if let Some(ir_m) = &llvm_ir_meta {
                        println!("📝 LLVM IR: {}", ir_m["path"]);
                    }
                    if let Some(bin_m) = &binary_meta {
                        println!("📦 Native Binary: {}", bin_m["path"]);
                    }
                }
                println!("📄 Pages: {}", pages_meta.len());
                println!("🏝️  Client islands: {}", islands_meta.len());
                println!("⏱️  Build duration: {:.2}ms", build_duration);
            }
        }
    }

    Ok(())
}
