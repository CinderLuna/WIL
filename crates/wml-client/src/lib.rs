pub const MICRO_SIGNALS_RUNTIME_JS: &str = r#"
// Embedded WML Micro-Signals Runtime (<2.5KB gzipped)
export function signal(val) {
    let subs = new Set();
    return {
        get val() { if (currentEffect) subs.add(currentEffect); return val; },
        set val(v) { val = v; for (let s of subs) s(); }
    };
}
let currentEffect = null;
export function effect(fn) {
    currentEffect = fn;
    try { fn(); } finally { currentEffect = null; }
}
"#;

pub struct ClientTranspiler;

impl ClientTranspiler {
    pub fn new() -> Self {
        Self
    }

    pub fn compile_island(&self, _island_name: &str) -> String {
        MICRO_SIGNALS_RUNTIME_JS.to_string()
    }
}
