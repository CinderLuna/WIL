pub mod compiler;
pub mod emitter;
pub mod target;

pub use compiler::{LlvmCodegenError, LlvmCompiler};
pub use emitter::LlvmIrEmitter;
pub use target::{TargetArch, TargetConfig, TargetOs};
use wml_mir::MirModule;

pub struct LlvmAotBackend {
    target: TargetConfig,
    compiler: LlvmCompiler,
}

impl Default for LlvmAotBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl LlvmAotBackend {
    pub fn new() -> Self {
        Self {
            target: TargetConfig::host(),
            compiler: LlvmCompiler::new(),
        }
    }

    pub fn with_target(target: TargetConfig) -> Self {
        Self {
            target,
            compiler: LlvmCompiler::new(),
        }
    }

    pub fn emit_ir(&self, module: &MirModule) -> Result<String, LlvmCodegenError> {
        self.compiler.compile_to_ir(module, &self.target)
    }

    pub fn emit_object(&self, module: &MirModule) -> Result<Vec<u8>, String> {
        #[cfg(feature = "llvm-backend")]
        {
            let ir = self.emit_ir(module).map_err(|e| e.to_string())?;
            let temp_dir = std::env::temp_dir();
            let temp_obj = temp_dir.join(format!(
                "wml_obj_{}.o",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            self.compiler
                .compile_to_object(&ir, &self.target, &temp_obj)
                .map_err(|e| e.to_string())?;
            let bytes = std::fs::read(&temp_obj).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&temp_obj);
            Ok(bytes)
        }
        #[cfg(not(feature = "llvm-backend"))]
        {
            let _ = module;
            Err("LLVM backend is disabled. Rebuild with `--features llvm-backend`".to_string())
        }
    }
}
