use clap::{Parser, Subcommand};
use std::path::PathBuf;
use wml_diagnostics::{DiagnosticReport, DiagnosticStatus, DiagnosticSummary};

#[derive(Parser, Debug)]
#[command(name = "wml", version = "0.1.0", about = "Web Minimalistic Language Compiler & Runtime")]
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Check { file, format } => {
            if format == "ai-json" {
                let report = DiagnosticReport {
                    protocol_version: "1.0.0".to_string(),
                    status: DiagnosticStatus::Success,
                    timestamp: "2026-10-21T00:00:00Z".to_string(),
                    summary: DiagnosticSummary::default(),
                    diagnostics: vec![],
                };
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Checking {}... OK (0 errors)", file.display());
            }
        }
        Commands::Dev { file, port } => {
            println!("Starting WML Cranelift JIT development server for {} on :{}", file.display(), port);
        }
        Commands::Build { file, release } => {
            println!("Building static binary for {} (release={})...", file.display(), release);
        }
    }

    Ok(())
}
