use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use wml_codegen_cranelift::CraneliftJitBackend;
use wml_diagnostics::{
    ByteRange, Diagnostic, DiagnosticCategory, DiagnosticReport, DiagnosticSeverity,
    DiagnosticStatus, DiagnosticSummary, Location, Span,
};
use wml_hir::check;
use wml_mir::{EscapeAnalysis, MirLowerer, MirOptimizer};
use wml_runtime::{HttpReactor, RouteHandlerFn};
use wml_syntax::parse;

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
    /// Compile a static standalone binary via LLVM AOT
    Build {
        /// Path to the entry .wml file
        file: PathBuf,
        /// Release mode with LTO
        #[arg(long)]
        release: bool,
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
                    compiled.routes.len(),
                    compile_duration
                );
                println!("🚀 [WML Server] Listening on http://{}", bind_addr);
            }

            if !dry_run {
                reactor.serve(listener)?;
            }
        }
        Commands::Build { file, release } => {
            println!(
                "Building static binary for {} (release={})...",
                file.display(),
                release
            );
        }
    }

    Ok(())
}
