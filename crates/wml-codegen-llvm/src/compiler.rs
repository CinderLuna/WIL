use crate::emitter::LlvmIrEmitter;
use crate::target::TargetConfig;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use wml_mir::MirModule;

#[derive(Error, Debug)]
pub enum LlvmCodegenError {
    #[error("I/O error during LLVM codegen: {0}")]
    Io(#[from] std::io::Error),
    #[error("Clang compiler invocation failed (exit code {code:?}): {stderr}")]
    ClangFailed { code: Option<i32>, stderr: String },
    #[error("Clang compiler not found in PATH")]
    ClangNotFound,
    #[error("LLVM Backend error: {0}")]
    Backend(String),
}

pub struct LlvmCompiler;

impl Default for LlvmCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl LlvmCompiler {
    pub fn new() -> Self {
        Self
    }

    pub fn is_clang_available() -> bool {
        Command::new("clang")
            .arg("--version")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    pub fn compile_to_ir(
        &self,
        mir: &MirModule,
        target: &TargetConfig,
    ) -> Result<String, LlvmCodegenError> {
        let mut emitter = LlvmIrEmitter::new(target.clone());
        Ok(emitter.emit_module(mir))
    }

    pub fn compile_to_object(
        &self,
        ir: &str,
        target: &TargetConfig,
        output_path: &Path,
    ) -> Result<PathBuf, LlvmCodegenError> {
        if !Self::is_clang_available() {
            return Err(LlvmCodegenError::ClangNotFound);
        }

        let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;

        let temp_ll = parent.join(format!(
            "temp_{}.ll",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::write(&temp_ll, ir)?;

        let triple_arg = format!("--target={}", target.triple());
        let output = Command::new("clang")
            .arg("-c")
            .arg("-O3")
            .arg(&triple_arg)
            .arg(&temp_ll)
            .arg("-o")
            .arg(output_path)
            .output()?;

        let _ = fs::remove_file(&temp_ll);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(LlvmCodegenError::ClangFailed {
                code: output.status.code(),
                stderr,
            });
        }

        Ok(output_path.to_path_buf())
    }

    pub fn compile_to_executable(
        &self,
        ir: &str,
        target: &TargetConfig,
        output_path: &Path,
    ) -> Result<PathBuf, LlvmCodegenError> {
        if !Self::is_clang_available() {
            return Err(LlvmCodegenError::ClangNotFound);
        }

        let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;

        let temp_ll = parent.join(format!(
            "temp_{}.ll",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::write(&temp_ll, ir)?;

        let triple_arg = format!("--target={}", target.triple());
        let output = Command::new("clang")
            .arg("-O3")
            .arg(&triple_arg)
            .arg(&temp_ll)
            .arg("-o")
            .arg(output_path)
            .output()?;

        let _ = fs::remove_file(&temp_ll);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(LlvmCodegenError::ClangFailed {
                code: output.status.code(),
                stderr,
            });
        }

        Ok(output_path.to_path_buf())
    }
}
