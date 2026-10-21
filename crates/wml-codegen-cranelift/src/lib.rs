use wml_mir::MirModule;

pub struct CraneliftJitBackend;

impl CraneliftJitBackend {
    pub fn new() -> Self {
        Self
    }

    pub fn compile_module(&self, _module: &MirModule) -> Result<(), String> {
        // Will implement cranelift lowering
        Ok(())
    }
}
