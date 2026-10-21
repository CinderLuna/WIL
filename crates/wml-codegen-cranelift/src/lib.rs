use wml_mir::MirModule;

pub struct CraneliftJitBackend;

impl Default for CraneliftJitBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CraneliftJitBackend {
    pub fn new() -> Self {
        Self
    }

    pub fn compile_module(&self, _module: &MirModule) -> Result<(), String> {
        // Will implement cranelift lowering
        Ok(())
    }
}
