pub mod escape;
pub mod ir;
pub mod lower;
pub mod opt;

pub use escape::{EscapeAnalysis, EscapeReport};
pub use ir::*;
pub use lower::MirLowerer;
pub use opt::MirOptimizer;
