pub mod builtins;
pub mod hir;
pub mod scope;
pub mod typecheck;
pub mod types;

pub use builtins::*;
pub use hir::*;
pub use scope::*;
pub use typecheck::{check, TypeChecker};
pub use types::Type;
