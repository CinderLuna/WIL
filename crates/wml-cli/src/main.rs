use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use wml_diagnostics::{
    ByteRange, Diagnostic, DiagnosticReport, DiagnosticSeverity, DiagnosticStatus,
    DiagnosticSummary, Location, Span,
};
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
                                category: "IoError".to_string(),
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
                Ok((program, _arena)) => {
                    if format == "ai-json" {
                        let report = DiagnosticReport {
                            protocol_version: "1.0.0".to_string(),
                            status: DiagnosticStatus::Success,
                            timestamp: current_timestamp,
                            summary: DiagnosticSummary {
                                total_errors: 0,
                                total_warnings: 0,
                                auto_fixable: 0,
                            },
                            diagnostics: vec![],
                        };
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        println!(
                            "Checking {}... OK ({} top-level items parsed, 0 errors)",
                            file.display(),
                            program.items.len()
                        );
                    }
                }
                Err(parse_err) => {
                    let (start_line, start_col) = byte_to_line_col(&source, parse_err.span.start);
                    let (end_line, end_col) = byte_to_line_col(&source, parse_err.span.end);

                    let diagnostic = Diagnostic {
                        id: format!("diag_{:x}", parse_err.span.start),
                        code: "WML-SYN-1001".to_string(),
                        category: "SyntaxError".to_string(),
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
        Commands::Dev { file, port } => {
            println!(
                "Starting WML Cranelift JIT development server for {} on :{}",
                file.display(),
                port
            );
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
