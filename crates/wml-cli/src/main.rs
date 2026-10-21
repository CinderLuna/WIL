use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use wml_client::ClientTranspiler;
use wml_codegen_cranelift::CraneliftJitBackend;
use wml_diagnostics::{
    ByteRange, Diagnostic, DiagnosticCategory, DiagnosticReport, DiagnosticSeverity,
    DiagnosticStatus, DiagnosticSummary, Location, Span,
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
        /// Output directory for production build
        #[arg(short, long, default_value = "dist")]
        out_dir: PathBuf,
        /// Output format (human or ai-json)
        #[arg(long, default_value = "human")]
        format: String,
    },
}

fn byte_to_line_col(source: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (idx, ch) in source.char_indices() {
        if idx >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let current_timestamp = chrono::Utc::now().to_rfc3339();

    match cli.command {
        Commands::Check { file, format } => {
            let source = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(err) => {
                    if format == "ai-json" {
                        let report = DiagnosticReport {
                            protocol_version: "1.0.0".to_string(),
                            status: DiagnosticStatus::Error,
                            timestamp: current_timestamp,
                            summary: DiagnosticSummary {
                                total_errors: 1,
                                total_warnings: 0,
                                auto_fixable: 0,
                            },
                            diagnostics: vec![Diagnostic {
                                id: "diag_io_001".to_string(),
                                code: "WML-IO-1001".to_string(),
                                category: DiagnosticCategory::RuntimePanic,
                                severity: DiagnosticSeverity::Error,
                                message: format!("Failed to read file {}: {}", file.display(), err),
                                primary_location: Location {
                                    file: file.display().to_string(),
                                    span: Span {
                                        start_line: 1,
                                        start_col: 1,
                                        end_line: 1,
                                        end_col: 1,
                                    },
                                    byte_range: ByteRange { start: 0, end: 0 },
                                },
                                suggested_patch: None,
                            }],
                        };
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        eprintln!("Error reading file {}: {}", file.display(), err);
                    }
                    std::process::exit(1);
                }
            };

            match parse(&source) {
                Ok((program, arena)) => {
                    let file_str = file.display().to_string();
                    let (_hir, diags) = wml_hir::check(&program, &arena, &source, &file_str);

                    let total_errors = diags
                        .iter()
                        .filter(|d| d.severity == DiagnosticSeverity::Error)
                        .count();
                    let total_warnings = diags
                        .iter()
                        .filter(|d| d.severity == DiagnosticSeverity::Warning)
                        .count();
                    let auto_fixable = diags.iter().filter(|d| d.suggested_patch.is_some()).count();

                    if format == "ai-json" {
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
                            timestamp: current_timestamp,
                            summary: DiagnosticSummary {
                                total_errors,
                                total_warnings,
                                auto_fixable,
                            },
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
                    let (start_line, start_col) = byte_to_line_col(&source, parse_err.span.start);
                    let (end_line, end_col) = byte_to_line_col(&source, parse_err.span.end);

                    let diagnostic = Diagnostic {
                        id: format!("diag_{:x}", parse_err.span.start),
                        code: "WML-SYN-1001".to_string(),
                        category: DiagnosticCategory::SyntaxError,
                        severity: DiagnosticSeverity::Error,
                        message: parse_err.message.clone(),
                        primary_location: Location {
                            file: file.display().to_string(),
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
                        suggested_patch: None,
                    };

                    if format == "ai-json" {
                        let report = DiagnosticReport {
                            protocol_version: "1.0.0".to_string(),
                            status: DiagnosticStatus::Error,
                            timestamp: current_timestamp,
                            summary: DiagnosticSummary {
                                total_errors: 1,
                                total_warnings: 0,
                                auto_fixable: 0,
                            },
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
                        let diagnostic = Diagnostic {
                            id: "diag_io_001".to_string(),
                            code: "WML-IO-1001".to_string(),
                            category: DiagnosticCategory::SyntaxError,
                            severity: DiagnosticSeverity::Error,
                            message: format!("Failed to read file '{}': {}", file_str, e),
                            primary_location: Location {
                                file: file_str.clone(),
                                span: Span {
                                    start_line: 1,
                                    start_col: 1,
                                    end_line: 1,
                                    end_col: 1,
                                },
                                byte_range: ByteRange { start: 0, end: 0 },
                            },
                            suggested_patch: None,
                        };
                        let report = DiagnosticReport {
                            protocol_version: "1.0.0".to_string(),
                            status: DiagnosticStatus::Error,
                            timestamp: current_timestamp,
                            summary: DiagnosticSummary {
                                total_errors: 1,
                                total_warnings: 0,
                                auto_fixable: 0,
                            },
                            diagnostics: vec![diagnostic],
                        };
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

                    let diagnostic = Diagnostic {
                        id: format!("diag_{:x}", parse_err.span.start),
                        code: "WML-SYN-1001".to_string(),
                        category: DiagnosticCategory::SyntaxError,
                        severity: DiagnosticSeverity::Error,
                        message: parse_err.message.clone(),
                        primary_location: Location {
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
                        suggested_patch: None,
                    };

                    if format == "ai-json" {
                        let report = DiagnosticReport {
                            protocol_version: "1.0.0".to_string(),
                            status: DiagnosticStatus::Error,
                            timestamp: current_timestamp,
                            summary: DiagnosticSummary {
                                total_errors: 1,
                                total_warnings: 0,
                                auto_fixable: 0,
                            },
                            diagnostics: vec![diagnostic],
                        };
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
                    let report = DiagnosticReport {
                        protocol_version: "1.0.0".to_string(),
                        status: DiagnosticStatus::Error,
                        timestamp: current_timestamp,
                        summary: DiagnosticSummary {
                            total_errors: diags.len(),
                            total_warnings: 0,
                            auto_fixable: 0,
                        },
                        diagnostics: diags,
                    };
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
            out_dir,
            format,
        } => {
            let start_time = std::time::Instant::now();
            let file_str = file.display().to_string();

            let source = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    if format == "ai-json" {
                        let diagnostic = Diagnostic {
                            id: "diag_io_001".to_string(),
                            code: "WML-IO-1001".to_string(),
                            category: DiagnosticCategory::SyntaxError,
                            severity: DiagnosticSeverity::Error,
                            message: format!("Failed to read file '{}': {}", file_str, e),
                            primary_location: Location {
                                file: file_str.clone(),
                                span: Span {
                                    start_line: 1,
                                    start_col: 1,
                                    end_line: 1,
                                    end_col: 1,
                                },
                                byte_range: ByteRange { start: 0, end: 0 },
                            },
                            suggested_patch: None,
                        };
                        let report = DiagnosticReport {
                            protocol_version: "1.0.0".to_string(),
                            status: DiagnosticStatus::Error,
                            timestamp: current_timestamp.clone(),
                            summary: DiagnosticSummary {
                                total_errors: 1,
                                total_warnings: 0,
                                auto_fixable: 0,
                            },
                            diagnostics: vec![diagnostic],
                        };
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

                    let diagnostic = Diagnostic {
                        id: format!("diag_{:x}", parse_err.span.start),
                        code: "WML-SYN-1001".to_string(),
                        category: DiagnosticCategory::SyntaxError,
                        severity: DiagnosticSeverity::Error,
                        message: parse_err.message.clone(),
                        primary_location: Location {
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
                        suggested_patch: None,
                    };

                    if format == "ai-json" {
                        let report = DiagnosticReport {
                            protocol_version: "1.0.0".to_string(),
                            status: DiagnosticStatus::Error,
                            timestamp: current_timestamp.clone(),
                            summary: DiagnosticSummary {
                                total_errors: 1,
                                total_warnings: 0,
                                auto_fixable: 0,
                            },
                            diagnostics: vec![diagnostic],
                        };
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
                    let report = DiagnosticReport {
                        protocol_version: "1.0.0".to_string(),
                        status: DiagnosticStatus::Error,
                        timestamp: current_timestamp.clone(),
                        summary: DiagnosticSummary {
                            total_errors: diags.len(),
                            total_warnings: 0,
                            auto_fixable: 0,
                        },
                        diagnostics: diags,
                    };
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

            // 7. Write manifest.json
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
                "routes": mir_module.routes.iter().map(|r| serde_json::json!({
                    "method": r.method,
                    "path": r.path,
                })).collect::<Vec<_>>(),
                "pages": pages_meta,
                "islands": islands_meta,
                "assets": assets_meta,
                "timestamp": current_timestamp,
            });
            fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

            let build_duration = start_time.elapsed().as_secs_f64() * 1000.0;

            if format == "ai-json" {
                let build_event = serde_json::json!({
                    "event": "build_complete",
                    "target": out_dir.display().to_string(),
                    "release": release,
                    "pages": pages_meta,
                    "islands": islands_meta,
                    "assets": assets_meta,
                    "build_duration_ms": build_duration,
                    "timestamp": current_timestamp,
                });
                println!("{}", serde_json::to_string(&build_event)?);
            } else {
                println!("⚡ [WML Build] Successfully compiled {}", file_str);
                println!("📦 Target directory: {}", out_dir.display());
                println!("📄 Pages: {}", pages_meta.len());
                println!("🏝️  Client islands: {}", islands_meta.len());
                println!("⏱️  Build duration: {:.2}ms", build_duration);
            }
        }
    }

    Ok(())
}
