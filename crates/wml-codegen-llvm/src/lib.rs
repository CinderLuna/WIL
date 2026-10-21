use wml_mir::MirModule;

pub struct LlvmAotBackend;

impl Default for LlvmAotBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl LlvmAotBackend {
    pub fn new() -> Self {
        Self
    }

    pub fn emit_object(&self, _module: &MirModule) -> Result<Vec<u8>, String> {
        #[cfg(feature = "llvm-backend")]
        {
            // Will hook up inkwell / LLVM code generation
            Ok(Vec::new())
        }
        #[cfg(not(feature = "llvm-backend"))]
        {
            Err("LLVM backend is disabled. Rebuild with `--features llvm-backend`".to_string())
        }
    }
}
